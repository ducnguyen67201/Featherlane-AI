use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
ALTER TABLE eval_runs ALTER COLUMN verdict DROP NOT NULL;
ALTER TABLE eval_runs ALTER COLUMN summary DROP NOT NULL;
ALTER TABLE eval_runs
    ADD COLUMN policy_pack_id uuid REFERENCES policy_packs(id),
    ADD COLUMN policy_pack_version integer,
    ADD COLUMN policy_content_sha256 text,
    ADD COLUMN target_version text,
    ADD COLUMN scenario_id uuid,
    ADD COLUMN rule_ids jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN boundary_kind text NOT NULL DEFAULT 'explicit_ci',
    ADD COLUMN external_run_id text,
    ADD COLUMN primary_invocation_id uuid,
    ADD COLUMN state text NOT NULL DEFAULT 'completed',
    ADD COLUMN completion_reason text,
    ADD COLUMN terminal_state text,
    ADD COLUMN settle_until timestamptz,
    ADD COLUMN hard_deadline_at timestamptz,
    ADD COLUMN last_seen_at timestamptz,
    ADD COLUMN finalized_at timestamptz,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN span_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN trace_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN event_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN trace_quality text,
    ADD COLUMN evidence_sha256 text;

UPDATE eval_runs
SET state = 'completed',
    updated_at = COALESCE(completed_at, created_at),
    finalized_at = completed_at,
    hard_deadline_at = COALESCE(completed_at, created_at),
    target_version = 'legacy',
    policy_pack_version = 0,
    policy_content_sha256 = 'legacy',
    primary_invocation_id = id,
    scenario_id = id;

ALTER TABLE eval_runs
    ALTER COLUMN target_version SET NOT NULL,
    ALTER COLUMN policy_content_sha256 SET NOT NULL,
    ALTER COLUMN policy_pack_version SET NOT NULL,
    ALTER COLUMN primary_invocation_id SET NOT NULL,
    ALTER COLUMN scenario_id SET NOT NULL,
    ALTER COLUMN hard_deadline_at SET NOT NULL;

CREATE INDEX idx_eval_runs_state_deadline
    ON eval_runs (state, hard_deadline_at);
CREATE UNIQUE INDEX uq_eval_runs_external_boundary
    ON eval_runs (organization_id, target_id, boundary_kind, external_run_id)
    WHERE external_run_id IS NOT NULL AND state NOT IN ('cancelled', 'failed');

ALTER TABLE normalized_events
    ADD COLUMN scenario_id uuid,
    ADD COLUMN ended_at timestamptz,
    ADD COLUMN linked_event_ids jsonb NOT NULL DEFAULT '[]'::jsonb;
UPDATE normalized_events SET scenario_id = eval_run_id WHERE scenario_id IS NULL;
ALTER TABLE normalized_events ALTER COLUMN scenario_id SET NOT NULL;
CREATE UNIQUE INDEX uq_normalized_event_run_sequence
    ON normalized_events (eval_run_id, sequence);
CREATE UNIQUE INDEX uq_rule_results_run_rule
    ON rule_results (eval_run_id, rule_id);

CREATE TABLE ingested_spans (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    eval_run_id uuid REFERENCES eval_runs(id) ON DELETE CASCADE,
    target_id text NOT NULL,
    external_run_id text,
    invocation_id uuid,
    scenario_id uuid,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    links jsonb NOT NULL DEFAULT '[]'::jsonb,
    resource jsonb NOT NULL DEFAULT '{}'::jsonb,
    scope_name text,
    scope_version text,
    name text NOT NULL,
    status text,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    attributes jsonb NOT NULL,
    sanitized_payload_sha256 text NOT NULL,
    correlation_status text NOT NULL,
    late_after_finalize boolean NOT NULL DEFAULT false,
    received_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX uq_ingested_spans_source
    ON ingested_spans (organization_id, target_id, trace_id, span_id);
CREATE INDEX idx_ingested_spans_run_time
    ON ingested_spans (eval_run_id, started_at, trace_id, span_id);
CREATE INDEX idx_ingested_spans_unassigned
    ON ingested_spans (organization_id, target_id, external_run_id, received_at)
    WHERE eval_run_id IS NULL;

CREATE TABLE evidence_bundles (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    eval_run_id uuid NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
    schema_version text NOT NULL,
    evidence_sha256 text NOT NULL,
    payload jsonb NOT NULL,
    finalized_at timestamptz NOT NULL,
    UNIQUE (eval_run_id),
    UNIQUE (organization_id, evidence_sha256)
);

CREATE TABLE telemetry_ingest_keys (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    target_id text NOT NULL,
    token_prefix text NOT NULL,
    token_sha256 text NOT NULL,
    status text NOT NULL,
    expires_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE (token_sha256)
);
CREATE INDEX idx_telemetry_ingest_keys_prefix
    ON telemetry_ingest_keys (token_prefix) WHERE status = 'active';

ALTER TABLE jobs
    ADD COLUMN dedupe_key text,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();
CREATE UNIQUE INDEX uq_jobs_live_dedupe
    ON jobs (organization_id, kind, dedupe_key)
    WHERE dedupe_key IS NOT NULL AND status IN ('pending', 'running');
CREATE INDEX idx_jobs_lease_recovery
    ON jobs (status, lease_expires_at) WHERE status = 'running';
",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
DROP INDEX IF EXISTS idx_jobs_lease_recovery;
DROP INDEX IF EXISTS uq_jobs_live_dedupe;
ALTER TABLE jobs DROP COLUMN IF EXISTS updated_at, DROP COLUMN IF EXISTS dedupe_key;
DROP TABLE IF EXISTS telemetry_ingest_keys;
DROP TABLE IF EXISTS evidence_bundles;
DROP TABLE IF EXISTS ingested_spans;
DROP INDEX IF EXISTS uq_rule_results_run_rule;
DROP INDEX IF EXISTS uq_normalized_event_run_sequence;
ALTER TABLE normalized_events
    DROP COLUMN IF EXISTS linked_event_ids,
    DROP COLUMN IF EXISTS ended_at,
    DROP COLUMN IF EXISTS scenario_id;
DROP INDEX IF EXISTS uq_eval_runs_external_boundary;
DROP INDEX IF EXISTS idx_eval_runs_state_deadline;
ALTER TABLE eval_runs
    DROP COLUMN IF EXISTS evidence_sha256,
    DROP COLUMN IF EXISTS trace_quality,
    DROP COLUMN IF EXISTS event_count,
    DROP COLUMN IF EXISTS trace_count,
    DROP COLUMN IF EXISTS span_count,
    DROP COLUMN IF EXISTS updated_at,
    DROP COLUMN IF EXISTS finalized_at,
    DROP COLUMN IF EXISTS last_seen_at,
    DROP COLUMN IF EXISTS hard_deadline_at,
    DROP COLUMN IF EXISTS settle_until,
    DROP COLUMN IF EXISTS terminal_state,
    DROP COLUMN IF EXISTS completion_reason,
    DROP COLUMN IF EXISTS state,
    DROP COLUMN IF EXISTS primary_invocation_id,
    DROP COLUMN IF EXISTS external_run_id,
    DROP COLUMN IF EXISTS boundary_kind,
    DROP COLUMN IF EXISTS rule_ids,
    DROP COLUMN IF EXISTS scenario_id,
    DROP COLUMN IF EXISTS target_version,
    DROP COLUMN IF EXISTS policy_content_sha256,
    DROP COLUMN IF EXISTS policy_pack_version,
    DROP COLUMN IF EXISTS policy_pack_id;
UPDATE eval_runs SET verdict = 'inconclusive' WHERE verdict IS NULL;
UPDATE eval_runs SET summary = '{}'::jsonb WHERE summary IS NULL;
ALTER TABLE eval_runs ALTER COLUMN verdict SET NOT NULL;
ALTER TABLE eval_runs ALTER COLUMN summary SET NOT NULL;
",
            )
            .await?;
        Ok(())
    }
}
