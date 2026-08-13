# Target and trace integration

## Policy import lane

Send a complete JSON policy aggregate to `POST /v1/policy-packs`. The API
validates rule-to-obligation references and writes the pack, rules, sources,
obligations, and links in one PostgreSQL transaction. The pack remains a draft
until `POST /v1/policy-packs/{id}/approve` records the human publication review.
Evaluation requests may then name that persisted `policy_pack_id`; background
jobs carry only the ID and evidence, never a serialized policy definition.

## Active CI lane

Register a target manifest with an HTTP message endpoint, optional reset
endpoint, target version, timeout, and a secret reference. Featherlane starts a
session and sends typed events. Each request carries:

- W3C `traceparent`;
- `x-governance-eval-run-id`;
- `x-governance-scenario-id`.

Text agents receive `{ "session_id", "message" }`. Webhook workflows receive the
configured JSON event. Voice agents are integrated through their test/media
gateway: the adapter can send an audio reference or transcript event while the
same trace contract observes model, tool, handoff, and terminal events.

The MVP never sends production credentials and should target staging, preview,
or a resettable sandbox.

## Passive observability lane

Production or staging services may send trace envelopes to `POST /v1/traces` on
the telemetry gateway. The gateway allowlists governance/OpenInference
attributes, removes secret-like keys before persistence, normalizes spans into
the versioned event schema, and assigns a trace-quality result.

Passive observation can detect evidence about naturally occurring behavior, but
cannot prove scenarios that did not happen. Active CI testing and passive
monitoring share the event schema and evaluator; they are different evidence
sources.

## Terminal workflows

Long-running workflows should expose a status endpoint or emit a final workflow
span. A successful HTTP response is not automatically proof of completed side
effects. If the terminal state is missing, affected rules become inconclusive.
