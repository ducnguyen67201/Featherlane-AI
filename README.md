# Featherlane

Featherlane is an evidence-first governance evaluation platform for AI agents and
workflows. It drives a target with synthetic events, observes the resulting
trajectory, and evaluates that evidence against human-approved policy rules.

The MVP is deliberately narrow:

- active CI evaluation through HTTP or webhook adapters;
- passive trace ingestion through a Rust telemetry gateway;
- database-backed governance policy imports with human-reviewed obligations;
- deterministic checks for ordering, absence, count limits, and terminal state;
- `PASS`, `FAIL`, and `INCONCLUSIVE` as separate outcomes;
- JSON, JUnit, HTML, API, and dark Next.js console surfaces.

Featherlane does **not** approve, deny, pause, or otherwise control the customer's
business workflow. A human reviewer approves the translation of a source
obligation into an executable rule. At runtime, the engine observes approval
events produced by the customer's system.

## Stack

- Rust 2024 workspace for all evaluation and control-plane logic
- [Loco 1.x](https://loco.rs/) as the API/worker application shell
- Axum/Tokio for target, telemetry, and sandbox services
- SeaORM 2 and PostgreSQL for tenant-scoped persistence
- Next.js 16 and React 19 for the governance console

Core crates remain framework-neutral, so Loco accelerates delivery without
coupling the evaluator or policy compiler to a web framework.

## Policy storage boundary

PostgreSQL is the only runtime source of truth for policy packs. Importing JSON
through the API or console creates one transactional aggregate across
`policy_packs`, `policy_rules`, `sources`, `obligations`, and
`policy_pack_sources`. Human publication decisions are written to
`policy_reviews`. The evaluator and background worker resolve an approved pack
by database ID; they never load policy definitions from YAML, fixtures, or the
frontend.

Files under `fixtures/policies` are one-time example request bodies only. They
are not read by the service at startup or evaluation time.

## Start locally

Requirements: Rust 1.95, Node.js 22+, pnpm 10, and Docker.

```bash
cp .env.example .env
pnpm dlx auth@latest secret
pnpm install
docker compose up postgres -d
cd apps/governance-api && cargo run -- start
```

Copy the generated secret into `BETTER_AUTH_SECRET` in `.env`; never commit the
populated file. In Google Cloud, create an OAuth 2.0 client with application type
**Web application**, configure the consent screen and test users if Google
requires them, and register this exact authorized redirect URI:

```text
http://localhost:3000/api/auth/callback/google
```

Set `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` from that client. Keep
`BETTER_AUTH_URL=http://localhost:3000`. Featherlane requests only the basic
Google identity needed to sign in; there is no password or additional provider.

In another terminal:

```bash
pnpm --dir apps/web dev
```

Open `http://localhost:3000`. The console has a deterministic seed fallback, so
its complete visual flow is still available after sign-in while the Rust API is
offline.

To start the complete container topology:

```bash
docker compose up --build
```

For production, set `BETTER_AUTH_URL` to the public HTTPS console origin and
register the matching exact callback, for example
`https://console.example.com/api/auth/callback/google`. Load the Better Auth and
Google client secrets from the deployment secret manager, not image build args
or source control. Complete Google's production consent-screen, homepage, and
privacy-policy requirements before making the client public.

Google login protects the web console and its same-origin mutation endpoints.
The Rust/Loco `/v1/*` API intentionally remains directly callable for the CLI
and service integrations in this MVP; add API authentication, service identity,
organization authorization, and RBAC before exposing that API publicly.

## Import and approve a policy

Importing always creates a draft in PostgreSQL:

```bash
curl --fail-with-body \
  -H 'content-type: application/json' \
  --data @fixtures/policies/refund-governance.import.json \
  http://127.0.0.1:8080/v1/policy-packs
```

After reviewing the stored obligations, approve the returned policy pack ID:

```bash
curl --fail-with-body \
  -H 'content-type: application/json' \
  --data '{"reviewer_id":"policy-owner@example.com","notes":"Reviewed against source"}' \
  http://127.0.0.1:8080/v1/policy-packs/POLICY_PACK_ID/approve
```

## CI evaluation

List the persisted packs and select an approved database ID:

```bash
cargo run -p gov-eval -- list-policies --api-url http://127.0.0.1:8080
```

Evaluate an evidence bundle and return a CI-friendly exit code:

```bash
cargo run -p gov-eval -- evaluate \
  --api-url http://127.0.0.1:8080 \
  --policy-pack-id POLICY_PACK_ID \
  --evidence fixtures/traces/refund-pass.json \
  --format junit \
  --fail-on-inconclusive
```

Exit code `0` means pass, `1` means fail (or inconclusive when the strict flag is
set), and `2` means invalid input or execution error.

## Repository map

```text
apps/governance-api                 Loco API and PostgreSQL worker host
apps/governance-worker              Loco evaluation job contract
apps/governance-telemetry-gateway   trace redaction and normalization ingress
apps/governance-sandbox             resettable synthetic approval/refund tools
apps/gov-eval                       CI CLI
apps/web                            Next.js governance console
crates/governance-*                 portable domain and infrastructure modules
migration                           SeaORM migrations
fixtures/policies                  example API import bodies; never runtime policy storage
examples/refund-agent               reference HTTP agent integration
```

See [architecture](docs/architecture.md), [integration](docs/integration.md),
[security](docs/security.md), and [Open US Law ingestion](docs/open-us-law.md).

## Result boundary

Reports say that a target was “tested against” a policy pack and expose the
underlying evidence. They are not a legal opinion, regulatory determination, or
compliance certification.
