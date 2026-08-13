# Featherlane AI

> Open-source evals for AI agents that act.

![Featherlane evaluation runs and agent trajectory inspector](docs/assets/featherlane-evals.png)

Featherlane runs complete agent workflows against human-approved policies. Turn
rules and real failures into repeatable evals, inspect model and tool traces,
and get `PASS`, `FAIL`, or `INCONCLUSIVE` with the evidence behind each result.

## What you get

- end-to-end HTTP and webhook agent evaluations;
- passive, multi-trace OTLP ingestion grouped into one business-level run;
- PDF, DOCX, TXT, and pasted-policy ingestion with grounded human review;
- deterministic ordering, absence, count, approval, and terminal-state checks;
- JSON, JUnit, HTML, API, and web-console results.

Featherlane does **not** approve, deny, pause, or otherwise control a customer's
workflow. It provides scope-limited evaluation evidence—not legal compliance
certification. A human reviewer approves the translation of source obligations
into executable rules; runtime approval evidence comes from the customer's own
structured events.

## Stack

- Rust 2024 workspace for evaluation and control-plane logic
- [Loco 1.x](https://loco.rs/) with Axum and Tokio
- SeaORM 2 and PostgreSQL for tenant-scoped persistence and durable jobs
- OpenTelemetry Protocol for framework-neutral trace ingestion
- Next.js 16 and React 19 for the governance console

Core crates remain framework-neutral, so the evaluator, policy compiler, target
contracts, and normalizer can be embedded independently of the web shell.

## Quick start

Requirements: Rust 1.95, Node.js 22+, pnpm 10, and Docker.

```bash
git clone https://github.com/ducnguyen67201/Featherlane-AI.git
cd Featherlane-AI
cp .env.example .env
pnpm install
pnpm dlx auth@latest secret
# Add the generated secret and your Google OAuth credentials to .env.
docker compose up --build
```

Register `http://localhost:3000/api/auth/callback/google` as the Google OAuth
callback, then open [http://localhost:3000](http://localhost:3000).

For a host-run development API instead of the complete container topology:

```bash
set -a; source .env; set +a
docker compose up postgres minio minio-init -d
export POLICY_LLM_ENABLED=true POLICY_LLM_PROVIDER=heuristic
cd apps/governance-api && cargo run -- start --server-and-worker
```

In another terminal, run `pnpm --dir apps/web dev`.

## Policy storage boundary

PostgreSQL is the runtime source of truth for import state, extracted candidates,
review decisions, and compiled policy packs. Source files are immutable,
content-addressed objects in S3-compatible storage. Completing review creates a
transactional aggregate across `policy_packs`, `policy_rules`, `sources`,
`obligations`, and `policy_pack_sources`; publication decisions are recorded in
`policy_reviews`.

The evaluator and worker resolve approved packs by database ID. Files under
`fixtures/policies` and `fixtures/policy-sources` are examples only and are never
loaded as runtime policy definitions.

## Import, review, and compile a policy source

Use **Policies → Import policy source** in the console. Choose a PDF, DOCX,
UTF-8 TXT file, or pasted text and submit its provenance metadata. The API stores
the bounded artifact, queues parsing and extraction, and opens a review workspace
where a human must:

1. verify source provenance;
2. compare each candidate with its exact source excerpt;
3. approve, reject, or edit every candidate and deterministic rule; and
4. compile the approved set into a database-backed draft policy pack.

For a direct multipart example:

```bash
curl --fail-with-body -D - \
  -H "Idempotency-Key: demo-refund-policy-1" \
  -F "file=@fixtures/policy-sources/refund-approval-policy.txt;type=text/plain" \
  -F "title=Customer refund approval policy" \
  -F "source_type=company_policy" \
  -F "jurisdiction=internal" \
  http://127.0.0.1:8080/v1/policy-imports
```

Poll the returned `Location`, then review the candidates in the console. Local
Compose uses the deterministic development extractor. For OpenRouter, configure
an approved pinned model and review the zero-retention/provider controls first.

### Advanced canonical JSON import

The canonical endpoint creates a draft in PostgreSQL:

```bash
curl --fail-with-body \
  -H 'content-type: application/json' \
  --data @fixtures/policies/refund-governance.import.json \
  http://127.0.0.1:8080/v1/policy-packs
```

After reviewing its obligations, approve the returned pack ID:

```bash
curl --fail-with-body \
  -H 'content-type: application/json' \
  --data '{"reviewer_id":"policy-owner@example.com","notes":"Reviewed against source"}' \
  http://127.0.0.1:8080/v1/policy-packs/POLICY_PACK_ID/approve
```

## CI evaluation

List persisted packs and select an approved database ID:

```bash
cargo run -p gov-eval -- list-policies --api-url http://127.0.0.1:8080
```

Create one explicit run, propagate the emitted resource attributes through the
workflow, then complete and wait for its one immutable result:

```bash
eval "$(cargo run -q -p gov-eval -- start \
  --target refund-agent-staging \
  --target-version "$GIT_SHA" \
  --policy-pack-id "$POLICY_PACK_ID" \
  --boundary explicit-ci --format shell)"
# Run the workflow with the emitted FEATHERLANE_* and OTEL_* values.
cargo run -q -p gov-eval -- complete \
  --run-id "$FEATHERLANE_EVAL_RUN_ID" --terminal-state completed
cargo run -q -p gov-eval -- wait \
  --run-id "$FEATHERLANE_EVAL_RUN_ID" --format junit --fail-on-inconclusive
```

Offline replay of immutable evidence remains available:

```bash
cargo run -p gov-eval -- evaluate \
  --api-url http://127.0.0.1:8080 \
  --policy-pack-id POLICY_PACK_ID \
  --evidence fixtures/traces/refund-pass.json \
  --format junit \
  --fail-on-inconclusive
```

Exit code `0` means pass, `1` means fail (or strict inconclusive), and `2` means
invalid input, timeout, or an operational error.

For a synchronous staging or sandbox target, register the generic `http_text`
or `webhook` manifest once, then drive a committed scenario directly from CI:

```bash
cargo run -p gov-eval -- run \
  --target-id TARGET_UUID \
  --policy-pack-id POLICY_PACK_ID \
  --scenario fixtures/scenarios/refund-approval.json \
  --format junit --fail-on-inconclusive > governance-junit.xml
```

This inline path works with any SDK or workflow behind the small HTTP contract;
the longer-lived `start`/`complete`/`wait` path above collects standard OTLP
across multiple traces.

## Repository map

```text
apps/governance-api                 Loco API and PostgreSQL worker host
apps/governance-worker              durable import/finalization/evaluation jobs
apps/governance-telemetry-gateway   authenticated OTLP/HTTP trace ingress
apps/governance-sandbox             resettable synthetic approval/refund tools
apps/gov-eval                       CI and offline evaluation CLI
apps/web                            authenticated Next.js governance console
crates/governance-*                 portable domain and infrastructure modules
migration                           additive SeaORM migrations
fixtures                            policy, source, trace, and OTLP examples
examples/refund-agent               reference HTTP agent integration
```

Reports say a target was “tested against” a policy pack and expose the underlying
evidence. They are not a legal opinion, regulatory determination, or compliance
certification.

[Architecture](docs/architecture.md) · [Integration](docs/integration.md) ·
[Security](docs/security.md) · [Open US Law ingestion](docs/open-us-law.md)

Built with Rust, PostgreSQL, Next.js, and OpenTelemetry. Licensed under
[Apache 2.0](LICENSE).
