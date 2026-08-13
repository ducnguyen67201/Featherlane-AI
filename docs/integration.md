# Evaluation-run and OTLP integration

Featherlane integrates at two stable boundaries: a business execution is one
evaluation run, and its telemetry is standard OTLP. A workflow execution, agent
task, voice call, or CI execution may contain many model calls, traces, retries,
and batches; all of them share one `featherlane.eval_run.id`.

## Active CI

Start a run against an approved, immutable policy pack:

```bash
eval "$(cargo run -q -p gov-eval -- start \
  --target refund-agent-staging \
  --target-version git:4e6a9c1 \
  --policy-pack-id "$POLICY_PACK_ID" \
  --boundary workflow-execution \
  --format shell)"
```

The shell output contains correlation IDs, the OTLP endpoint/protocol, and a
merged `OTEL_RESOURCE_ATTRIBUTES` value—never an ingest credential. Configure
the target-scoped `OTEL_EXPORTER_OTLP_HEADERS` secret separately in CI.
Featherlane's target driver propagates `traceparent`, W3C `baggage`, canonical
`x-featherlane-*` headers, and temporary `x-governance-*` compatibility headers.
Copy baggage values into span attributes if the SDK does not do that itself:

```text
featherlane.eval_run.id
featherlane.invocation.id
featherlane.scenario.id
```

After the full workflow/task has ended, close the boundary and wait for the one
immutable result:

```bash
cargo run -q -p gov-eval -- complete \
  --run-id "$FEATHERLANE_EVAL_RUN_ID" --terminal-state completed
cargo run -q -p gov-eval -- wait \
  --run-id "$FEATHERLANE_EVAL_RUN_ID" --format junit --fail-on-inconclusive
```

A successful target HTTP response is not automatically a completed workflow.
The target contract must say it is terminal or the caller must invoke complete.

## Generic OTLP export

Keep the OpenTelemetry/OpenInference instrumentation already used by Google ADK,
Mastra, OpenAI Agents SDK, LangGraph, or a custom framework. Add a small resource
or span processor that attaches the correlation attributes above, then point the
normal OTLP/HTTP exporter at Featherlane:

```text
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://localhost:4318/v1/traces
OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=http/protobuf
OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer flt_...
```

The endpoint accepts OTLP protobuf and OTLP JSON, optional gzip, and responds in
the request format. The ingest key is target-scoped; tenant and target identity
come from the key, never from customer-controlled span attributes. Exact span
retries are idempotent. Conflicting retries receive an OTLP partial-success
rejection.

For passive workflows without a Featherlane-created ID, persist a target
`telemetry_boundary` capability with its boundary kind, ordered external-ID
attributes, approved default policy pack, and finite timeouts. For example:

```json
{
  "boundary_kind": "workflow_execution",
  "external_id_attributes": ["workflow.run.id"],
  "terminal_attribute": "workflow.completed",
  "default_policy_pack_id": "<approved-policy-pack-uuid>",
  "settle_seconds": 10,
  "idle_timeout_seconds": 300,
  "max_duration_seconds": 3600,
  "conversation_id_is_task_boundary": false
}
```

The first span with that configured external ID atomically finds or creates the
run. `gen_ai.conversation.id` is considered only when it appears in the target's
attribute list and `conversation_id_is_task_boundary` is true. A long-lived chat
session must not be treated as an evaluation run by default.

## Human approval and terminal events

Framework spans are useful evidence but cannot reliably reveal every domain
event. Emit a structured span attribute when the customer system records an
approval decision:

```text
featherlane.event.type=human_approval_decision
decision=approved
```

Likewise, a terminal event may use:

```text
featherlane.event.type=final_output
featherlane.run.terminal=true
featherlane.run.terminal_state=completed
```

Featherlane does not ask a model judge to invent an approval or guess where an
opaque workflow ended. A target-specific deterministic mapping is acceptable;
fabricated semantic evidence is not. If required approval evidence is absent,
the policy's missing-evidence behavior yields `not_observable`, `fail`, or
`error`—never a vacuous pass.

## Unassigned telemetry

Valid, redacted spans without a verified run or configured external boundary are
stored as unassigned diagnostics. They are not evaluated. Inspect the target's
correlation configuration, then resend with a canonical run ID or create the
matching external-boundary run. Late spans are retained as diagnostics but never
mutate an already finalized bundle or verdict.

The sample requests in `fixtures/otlp/correlated-run/` put approval in trace A
and execution/terminal events in linked trace B. Send `02-execution.json` first,
then `01-approval.json`, and retry either file to exercise out-of-order and
idempotent ingestion.

## Synchronous inline target adapters

For CI jobs that can finish in one bounded HTTP exchange, register a staging,
preview, or sandbox target through the console or `POST /v1/targets`. The
adapter is protocol-based rather than SDK-specific:

- `http_text` accepts a `user_text` scenario event and sends a session ID plus
  message.
- `webhook` accepts `webhook` or `system` events and preserves the configured
  JSON payload.

Every request propagates `traceparent`, W3C `baggage`, and the canonical
evaluation, invocation, and scenario headers. The target returns a bounded
`schema_version: "1.0"` envelope with normalized observations, side effects,
and an explicit terminal flag. Missing terminal evidence becomes
`INCONCLUSIVE`; it never becomes a vacuous pass.

Run a committed scenario from CI with:

```bash
FEATHERLANE_API_URL=http://127.0.0.1:8080 cargo run -p gov-eval -- run \
  --target-id TARGET_UUID \
  --policy-pack-id POLICY_PACK_ID \
  --scenario fixtures/scenarios/refund-approval.json \
  --format junit --fail-on-inconclusive > governance-junit.xml
```

Use this synchronous path for fast resettable test targets. Use the correlated
OTLP run lifecycle above for asynchronous workflows, multiple traces, retries,
or passive production observation.
