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
  authenticated same-origin BFF routes for browser mutations.

## Before production

The console login authenticates a Google identity; it does not authorize a role
or organization. Any Google account can access the single MVP organization, and
stateless sessions cannot be individually revoked before their eight-hour
expiry. Direct Loco `/v1/*` endpoints also remain unauthenticated so the CLI and
service integrations keep their current contract.

Add authentication at the Loco edge, API keys or workload identity for service
callers, RBAC and organization membership, session revocation and auth audit
events, and PostgreSQL row-level security as a second tenant boundary. Also add
envelope encryption for retained evidence, object-store retention policies,
signed report exports, KMS-backed secret references, rate limits, request-body
limits, audit-log export, and a reviewed deletion workflow.

Do not capture raw prompts, model outputs, audio, or tool payloads by default.
Customers must explicitly allowlist content fields needed by an approved rule.
