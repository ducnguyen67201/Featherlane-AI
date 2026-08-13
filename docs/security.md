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

## Before production

Add authentication and RBAC at the Loco edge, PostgreSQL row-level security as a
second tenant boundary, envelope encryption for retained evidence, object-store
retention policies, signed report exports, KMS-backed secret references, rate
limits, request-body limits, audit-log export, and a reviewed deletion workflow.

Do not capture raw prompts, model outputs, audio, or tool payloads by default.
Customers must explicitly allowlist content fields needed by an approved rule.
