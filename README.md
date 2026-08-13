# Featherlane AI

> Open-source evals for AI agents that act.

![Featherlane evaluation runs and agent trajectory inspector](docs/assets/featherlane-evals.png)

Featherlane runs complete agent workflows against human-approved policies. Turn
rules and real failures into repeatable evals, inspect model and tool traces,
and get `PASS`, `FAIL`, or `INCONCLUSIVE` with the evidence behind each result.

## What you get

- End-to-end HTTP and webhook agent evals
- Trace, tool-action, and side-effect evaluation
- Deterministic policy checks that run in CI
- JSON, JUnit, HTML, API, and web-console results

## Quick start

```bash
git clone https://github.com/ducnguyen67201/Featherlane-AI.git
cd Featherlane-AI
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

Open `http://localhost:3000`. Targets and evaluation history come from the Rust
API and PostgreSQL; an API outage is shown as an error and is never replaced by
demo records.

To start the complete container topology:

```bash
docker compose up --build
```

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

## Connect a target

Start the reference wrapper, then register it using the copy-ready fixture:

```bash
cargo run -p refund-agent

curl --fail-with-body \
  -H 'content-type: application/json' \
  --data @fixtures/targets/refund-agent.json \
  http://127.0.0.1:8080/v1/targets
```

When the API runs inside Compose, the fixture endpoint
`http://refund-agent:8091/v1/messages` is correct. For an API running directly
on the host, change it to `http://127.0.0.1:8091/v1/messages`. Save the returned
target UUID alongside the approved policy-pack UUID.

## CI evaluation

List the persisted packs and select an approved database ID:

```bash
cargo run -p gov-eval -- list-policies --api-url http://127.0.0.1:8080
```

Drive the saved target with a committed scenario and return a CI-friendly exit
code:

```bash
cargo run -p gov-eval -- run \
  --target-id TARGET_ID \
  --policy-pack-id POLICY_PACK_ID \
  --scenario fixtures/scenarios/refund-approval.json \
  --format junit \
  --fail-on-inconclusive > governance-junit.xml
```

`FEATHERLANE_API_URL` sets the service URL and defaults to
`http://127.0.0.1:8080`. Exit code `0` means pass, `1` means fail (or
inconclusive when the strict flag is set), and `2` means invalid input or an
execution/transport error.

The original offline evidence evaluator remains available:

```bash
cargo run -p gov-eval -- evaluate \
  --api-url http://127.0.0.1:8080 \
  --policy-pack-id POLICY_PACK_ID \
  --evidence fixtures/traces/refund-pass.json \
  --format junit \
  --fail-on-inconclusive
```

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
fixtures/targets                   example target registration bodies
fixtures/scenarios                 commit-ready active-CI scenarios
examples/refund-agent               reference HTTP agent integration
```

See [architecture](docs/architecture.md), [integration](docs/integration.md),
[security](docs/security.md), and [Open US Law ingestion](docs/open-us-law.md).

## Result boundary

Reports say that a target was “tested against” a policy pack and expose the
underlying evidence. They are not a legal opinion, regulatory determination, or
compliance certification.

Featherlane is a working, pre-pilot MVP built with Rust, PostgreSQL, Next.js,
and OpenTelemetry. Licensed under [Apache 2.0](LICENSE).
