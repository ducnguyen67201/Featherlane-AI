#!/usr/bin/env bash
set -euo pipefail

api_url="${FEATHERLANE_API_URL:-http://127.0.0.1:8080}"
otlp_url="${FEATHERLANE_OTLP_URL:-http://127.0.0.1:4318/v1/traces}"
console_url="${FEATHERLANE_CONSOLE_URL:-http://localhost:3000}"
policy_pack_id="${POLICY_PACK_ID:?Set POLICY_PACK_ID to an approved database policy UUID}"
expected_verdict="${EXPECT_VERDICT:-}"
suffix="$(date +%s)-$$"
target_key="passive-smoke-${suffix}"
session_id="passive-session-${suffix}"
time_hex="$(printf '%016x' "$(date +%s)")"
process_hex="$(printf '%08x' "$$")"
trace_one="${time_hex}${process_hex}00000001"
trace_two="${time_hex}${process_hex}00000002"
approval_span="${process_hex}00000001"
tool_span="${process_hex}00000002"
terminal_span="${process_hex}00000003"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "${script_dir}/.." && pwd)"
temporary_dir="$(mktemp -d)"
trap 'rm -rf "${temporary_dir}"' EXIT

for command in curl jq; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "ERROR: ${command} is required" >&2
    exit 2
  fi
done

target_payload="$(jq -n \
  --arg key "${target_key}" \
  --arg policy "${policy_pack_id}" \
  '{
    name: "Passive smoke agent",
    key: $key,
    version: "git:smoke",
    environment: "sandbox",
    driver_type: "http_text",
    endpoint: "http://refund-agent:8091/v1/messages",
    reset_endpoint: null,
    auth_secret_ref: null,
    timeout_seconds: 30,
    otlp_required: true,
    telemetry_boundary: {
      boundary_kind: "workflow_execution",
      external_id_attributes: ["featherlane.external_run.id"],
      terminal_attribute: "featherlane.run.terminal",
      default_policy_pack_id: $policy,
      settle_seconds: 1,
      idle_timeout_seconds: 60,
      max_duration_seconds: 300,
      conversation_id_is_task_boundary: false
    }
  }')"

target_response="$(curl --fail-with-body --silent --show-error \
  -H 'content-type: application/json' \
  --data "${target_payload}" \
  "${api_url}/v1/targets")"
target_id="$(jq -er '.id' <<<"${target_response}")"

key_response="$(curl --fail-with-body --silent --show-error \
  -H 'content-type: application/json' \
  --data '{}' \
  "${api_url}/v1/targets/${target_id}/telemetry-key/rotate")"
ingest_token="$(jq -er '.plaintext' <<<"${key_response}")"

for fixture in 01-approval.json 02-execution-terminal.json; do
  jq \
    --arg session "${session_id}" \
    --arg trace_one "${trace_one}" \
    --arg trace_two "${trace_two}" \
    --arg approval_span "${approval_span}" \
    --arg tool_span "${tool_span}" \
    --arg terminal_span "${terminal_span}" '
    walk(
      if type == "string"
      then
        if . == "session-placeholder" then $session
        elif . == "11111111111111111111111111111111" then $trace_one
        elif . == "22222222222222222222222222222222" then $trace_two
        elif . == "1111111111111111" then $approval_span
        elif . == "2222222222222221" then $tool_span
        elif . == "2222222222222222" then $terminal_span
        else .
        end
      else .
      end
    )
  ' "${repo_dir}/fixtures/otlp/passive-session/${fixture}" > "${temporary_dir}/${fixture}"
done

curl --fail-with-body --silent --show-error \
  -H 'content-type: application/json' \
  -H "authorization: Bearer ${ingest_token}" \
  --data-binary "@${temporary_dir}/01-approval.json" \
  "${otlp_url}" >/dev/null
curl --fail-with-body --silent --show-error \
  -H 'content-type: application/json' \
  -H "authorization: Bearer ${ingest_token}" \
  --data-binary "@${temporary_dir}/02-execution-terminal.json" \
  "${otlp_url}" >/dev/null

run_id=""
detail=""
for _ in $(seq 1 60); do
  evaluations="$(curl --fail-with-body --silent --show-error "${api_url}/v1/evaluations")"
  run_id="$(jq -r --arg target "${target_key}" --arg session "${session_id}" \
    '[.[] | select(.target_id == $target and .external_run_id == $session)][0].id // empty' \
    <<<"${evaluations}")"
  if [[ -n "${run_id}" ]]; then
    detail="$(curl --fail-with-body --silent --show-error "${api_url}/v1/evaluations/${run_id}")"
    state="$(jq -r '.run.state' <<<"${detail}")"
    if [[ "${state}" == "completed" || "${state}" == "failed" || "${state}" == "cancelled" ]]; then
      break
    fi
  fi
  sleep 1
done

if [[ -z "${run_id}" || -z "${detail}" ]]; then
  echo "ERROR: automatic evaluation was not created before timeout" >&2
  exit 1
fi

state="$(jq -r '.run.state' <<<"${detail}")"
actual_policy="$(jq -r '.run.policy_pack_id' <<<"${detail}")"
completion_reason="$(jq -r '.run.completion_reason' <<<"${detail}")"
trace_count="$(jq -r '.run.trace_count' <<<"${detail}")"
verdict="$(jq -r '.run.verdict // "pending"' <<<"${detail}")"

[[ "${state}" == "completed" ]] || { echo "ERROR: run ended in state ${state}" >&2; exit 1; }
[[ "${actual_policy}" == "${policy_pack_id}" ]] || { echo "ERROR: run used unexpected policy" >&2; exit 1; }
[[ "${completion_reason}" == "terminal_event" ]] || { echo "ERROR: run did not close from terminal telemetry" >&2; exit 1; }
[[ "${trace_count}" == "2" ]] || { echo "ERROR: expected 2 traces, received ${trace_count}" >&2; exit 1; }
if [[ -n "${expected_verdict}" && "${verdict^^}" != "${expected_verdict^^}" ]]; then
  echo "ERROR: expected verdict ${expected_verdict}, received ${verdict}" >&2
  exit 1
fi

echo "Automatic evaluation completed"
echo "Run: ${run_id}"
echo "Verdict: ${verdict}"
echo "Traces: ${trace_count}"
echo "Detail: ${console_url}/evaluations/${run_id}"
