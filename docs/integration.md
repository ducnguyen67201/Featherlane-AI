# Target and trace integration

## Policy import lane

Send a complete JSON policy aggregate to `POST /v1/policy-packs`. The API
validates rule-to-obligation references and writes the pack, rules, sources,
obligations, and links in one PostgreSQL transaction. The pack remains a draft
until `POST /v1/policy-packs/{id}/approve` records the human publication review.
Only an approved, database-backed `policy_pack_id` can run.

## Active CI lane

Register a target once through the console or `POST /v1/targets`. The editable
request is demonstrated in `fixtures/targets/refund-agent.json`; Featherlane
adds the UUID, `schema_version: "1.0"`, `evidence_mode: "inline"`, and
`production_credentials_allowed: false`. Configuration is immutable by target
key/version. A failed readiness GET is stored as a degraded capability report,
which makes container-network mistakes visible without fabricating health.

The adapter is protocol-based, not SDK-specific. Put any OpenAI Agents SDK,
LangGraph, CrewAI, Vercel AI SDK, Temporal, n8n, or custom implementation behind
one of these wrappers:

- `http_text`: accepts only a `user_text` event and receives
  `{ "session_id": "...", "message": "..." }`.
- `webhook`: accepts `webhook` or `system` events and receives the configured
  JSON payload unchanged.

Before POSTing scenarios, the endpoint must answer a readiness GET with 2xx.
Every reset and invocation request carries:

- W3C `traceparent`;
- `x-governance-eval-run-id`;
- `x-governance-scenario-id`.

If `auth_secret_ref` is configured, it must be an uppercase environment-variable
name available to the Rust API. Its resolved value is sent as a Bearer token and
is never stored or returned. Redirects are disabled.

The target must synchronously return HTTP 2xx with a terminal inline envelope:

```json
{
  "schema_version": "1.0",
  "terminal": true,
  "terminal_state": "completed",
  "output": { "message": "Refund completed" },
  "events": [
    {
      "event_type": "final_output",
      "name": "refund completed",
      "actor": { "actor_type": "agent", "id": "refund-agent" },
      "input": null,
      "output": { "message": "completed" },
      "attributes": { "terminal_state": "completed" }
    }
  ],
  "side_effects": []
}
```

`synthetic_events` is accepted as a compatibility alias for `events`. Responses
are limited to 2 MiB and 1,000 observations. A scenario contains 1–50 events;
text is limited to 32 KiB and JSON payloads to 256 KiB. Unknown schema versions,
oversized content, malformed evidence, and non-2xx responses are operational
errors and do not create a completed run. Missing final evidence or
`terminal: false` becomes `INCONCLUSIVE`, never `PASS`.

Run `fixtures/scenarios/refund-approval.json` from CI:

```bash
FEATHERLANE_API_URL=http://127.0.0.1:8080 cargo run -p gov-eval -- run \
  --target-id TARGET_ID \
  --policy-pack-id POLICY_PACK_ID \
  --scenario fixtures/scenarios/refund-approval.json \
  --format junit --fail-on-inconclusive > governance-junit.xml
```

Exit codes are `0` for pass, `1` for fail (and strict inconclusive), and `2` for
invalid input or operational failure. JSON, JUnit, and HTML reports go to stdout;
diagnostics go to stderr.

The MVP never sends production credentials and supports only staging, preview,
or a resettable sandbox. The Rust API—not the browser or CI runner—must be able
to reach the saved endpoint. Active-CI webhooks must finish within the configured
1–120 second timeout and return terminal evidence inline. SDK-specific packages,
durable OTLP retrieval, and asynchronous workflow callbacks are intentionally
outside this release.

## Passive observability lane

Production or staging services may send trace envelopes to `POST /v1/traces` on
the telemetry gateway. The gateway allowlists governance/OpenInference
attributes, removes secret-like keys before persistence, normalizes spans into
the versioned event schema, and assigns a trace-quality result.

Passive observation can detect evidence about naturally occurring behavior, but
cannot prove scenarios that did not happen. Active CI testing and passive
monitoring share the event schema and evaluator; they are different evidence
sources.

## Minimal GitHub Actions job

```yaml
jobs:
  governance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p gov-eval --locked
      - name: Evaluate connected target
        env:
          FEATHERLANE_API_URL: ${{ secrets.FEATHERLANE_API_URL }}
        run: target/debug/gov-eval run --target-id "${{ secrets.FEATHERLANE_TARGET_ID }}" --policy-pack-id "${{ secrets.FEATHERLANE_POLICY_PACK_ID }}" --scenario fixtures/scenarios/refund-approval.json --format junit --fail-on-inconclusive > governance-junit.xml
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: governance-junit
          path: governance-junit.xml
```
