#!/usr/bin/env bash
set -euo pipefail

api_url="${FEATHERLANE_API_URL:-http://127.0.0.1:8080}"
console_key="${GOVERNANCE_CONSOLE_API_KEY:?Set GOVERNANCE_CONSOLE_API_KEY}"
actor="${FEATHERLANE_SMOKE_ACTOR:-smoke-policy-owner@example.test}"
suffix="$(date +%s)-$$"
headers=(-H "x-featherlane-console-key: ${console_key}" -H "x-featherlane-actor-id: ${actor}")
repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

collection="$(curl --fail-with-body --silent --show-error "${headers[@]}" \
  -H 'content-type: application/json' \
  --data "{\"key\":\"ingestion-smoke-${suffix}\",\"version\":1,\"title\":\"Ingestion smoke policy\",\"idempotency_key\":\"ingestion-smoke-${suffix}\"}" \
  "${api_url}/v1/policy-collections")"
collection_id="$(jq -er '.id' <<<"${collection}")"
key_one="source-one-${suffix}"
key_two="source-two-${suffix}"
manifest="$(jq -cn --arg one "${key_one}" --arg two "${key_two}" '{source_type:"company_policy",jurisdiction:"internal",items:[{client_item_key:$one,title:"Refund approval source",source_url:null},{client_item_key:$two,title:"Prompt injection source",source_url:null}]}')"
batch="$(curl --fail-with-body --silent --show-error "${headers[@]}" \
  -F "manifest=${manifest};type=application/json" \
  -F "file:${key_one}=@${repo_dir}/fixtures/policy-sources/refund-approval-policy.txt;type=text/plain" \
  -F "file:${key_two}=@${repo_dir}/fixtures/policy-sources/prompt-injection-policy.txt;type=text/plain" \
  "${api_url}/v1/policy-collections/${collection_id}/uploads")"
batch_id="$(jq -er '.id' <<<"${batch}")"

detail=""
for _ in $(seq 1 90); do
  detail="$(curl --fail-with-body --silent --show-error "${headers[@]}" "${api_url}/v1/source-ingestion-batches/${batch_id}")"
  status="$(jq -r '.[0].status' <<<"${detail}")"
  [[ "${status}" == "complete" || "${status}" == "partial" || "${status}" == "failed" ]] && break
  sleep 1
done
[[ "$(jq -r '.[0].status' <<<"${detail}")" == "complete" ]] || { echo "ERROR: ingestion batch did not complete" >&2; exit 1; }

while IFS= read -r import_id; do
  curl --fail-with-body --silent --show-error -H 'content-type: application/json' \
    --data "{\"decision\":\"verified\",\"reviewer_id\":\"${actor}\",\"notes\":\"Smoke verified\"}" \
    "${api_url}/v1/policy-imports/${import_id}/verify-source" >/dev/null
  candidates="$(curl --fail-with-body --silent --show-error "${api_url}/v1/policy-imports/${import_id}/candidates")"
  while IFS= read -r candidate; do
    candidate_id="$(jq -r '.id' <<<"${candidate}")"
    payload="$(jq -c --arg actor "${actor}" '{decision:(if .mapping_status == "ready" and .suggested_rule != null then "approved" else "rejected" end),reviewer_id:$actor,notes:"Smoke disposed",expected_updated_at:.updated_at,candidate:{statement:.statement,applicability:.applicability,exceptions:.exceptions,required_evidence:.required_evidence,suggested_severity:.suggested_severity,suggested_rule:.suggested_rule,mapping_status:.mapping_status}}' <<<"${candidate}")"
    curl --fail-with-body --silent --show-error -X PATCH -H 'content-type: application/json' --data "${payload}" \
      "${api_url}/v1/policy-imports/${import_id}/candidates/${candidate_id}" >/dev/null
  done < <(jq -c '.[]' <<<"${candidates}")
done < <(jq -r '.[1][].policy_import_id // empty' <<<"${detail}")

pack="$(curl --fail-with-body --silent --show-error "${headers[@]}" -X POST "${api_url}/v1/policy-collections/${collection_id}/compile")"
policy_pack_id="$(jq -er '.id' <<<"${pack}")"
source_count="$(curl --fail-with-body --silent --show-error "${api_url}/v1/policy-packs" | jq -r --arg id "${policy_pack_id}" '.[] | select(.id == $id) | .source_count')"
[[ "${source_count}" == "2" ]] || { echo "ERROR: expected a two-source pack" >&2; exit 1; }
curl --fail-with-body --silent --show-error -H 'content-type: application/json' \
  --data "{\"reviewer_id\":\"${actor}\",\"notes\":\"Smoke publication approval\"}" \
  "${api_url}/v1/policy-packs/${policy_pack_id}/approve" >/dev/null

EXPECT_VERDICT="${EXPECT_VERDICT:-PASS}" POLICY_PACK_ID="${policy_pack_id}" \
  "${repo_dir}/scripts/smoke-passive-auto-evaluation.sh"
