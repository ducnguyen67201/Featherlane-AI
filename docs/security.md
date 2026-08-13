# Security and evidence boundaries

## MVP controls

- organization-scoped repository queries and foreign keys;
- schema-validated JSON policy imports with deterministic rule types and no embedded scripts;
- transactional policy aggregates in PostgreSQL; no runtime filesystem policy loader;
- secret references instead of secret values in target manifests;
- synthetic targets and a resettable mock side-effect ledger;
- attribute allowlisting and nested secret-key redaction before trace storage;
- pinned corpus manifests, declared byte limits, SHA-256, and Parquet markers;
- quarantined source records cannot be treated as verified obligations;
- immutable policy/evidence content hashes;
- non-certification language in reviewer-facing reports.
- Google-only authentication for the Next.js governance console using an
  encrypted, stateless eight-hour session;
- full session validation at console routing and data-access boundaries, with
  authenticated same-origin BFF routes for browser mutations;
- target URLs restricted to absolute HTTP(S) URLs without userinfo, with
  redirects disabled and well-known metadata/link-local literal hosts blocked;
- authenticated reset URLs restricted to the target origin so a configured
  Bearer secret cannot be forwarded to another service;
- target responses capped at 2 MiB and 1,000 observations, with scenario text,
  payload, event-count, and timeout limits enforced before network execution;
- server-side Bearer resolution by uppercase `auth_secret_ref`; the referenced
  value is absent from manifests, capability reports, errors, and logs;
- inline target observations receive server-owned organization, run, scenario,
  invocation, event, trace, sequence, and timestamp context; observations and
  side-effect values are redacted before their integrity metadata is persisted.

## Before production

The console login authenticates a Google identity; it does not authorize a role
or organization. Any Google account can access the single MVP organization, and
stateless sessions cannot be individually revoked before their eight-hour
expiry. Direct Loco `/v1/*` endpoints also remain unauthenticated so the CLI and
service integrations keep their current contract.

Target endpoints are trusted self-hosted administrator configuration, not a
safe multi-tenant URL-fetch feature. Private/container hostnames are deliberately
allowed for local deployments, so operators must restrict API access and
network egress. Only staging, preview, and sandbox targets are accepted;
production credentials are refused. Never expose the direct API publicly in
this MVP.

Inline observations are assertions supplied by the target wrapper. Featherlane
redacts sensitive-key content, limits it, assigns trusted correlation IDs, and
hashes the resulting bundle, but this is test evidence rather than a signed
attestation. Raw prompt, output, and tool fields can remain sensitive even after
redaction; retain and authorize them accordingly. Signed provenance and durable
OTLP correlation are future work.

Add authentication at the Loco edge, API keys or workload identity for service
callers, RBAC and organization membership, session revocation and auth audit
events, and PostgreSQL row-level security as a second tenant boundary. Also add
envelope encryption for retained evidence, object-store retention policies,
signed report exports, KMS-backed secret references, rate limits, request-body
limits, audit-log export, and a reviewed deletion workflow.

Do not capture raw prompts, model outputs, audio, or tool payloads by default.
Customers must explicitly allowlist content fields needed by an approved rule.
