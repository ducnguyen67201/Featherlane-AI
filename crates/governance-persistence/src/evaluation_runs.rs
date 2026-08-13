use async_trait::async_trait;
use governance_application::{
    ApplicationError, ClaimedJob, DurableJob, DurableJobRepository, EvaluationRunRepository,
    EvidenceBundleRepository, SpanInsert, SpanInsertOutcome, TelemetryBoundaryRepository,
    TelemetryIngestIdentity, TelemetryIngestKey, TelemetryIngestKeyRepository,
    TelemetrySpanRepository, TelemetryTargetBoundary,
};
use governance_domain::{
    EvalRunId, EvaluationRun, EvaluationRunState, EvidenceBundle, InvocationId, OrganizationId,
    PolicyPackId, ScenarioId,
};
use governance_targets::TelemetryBoundaryConfig;
use governance_telemetry::{ObservedSpan, SpanLink};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbBackend,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, Statement, TransactionTrait,
    sea_query::{Expr, LockBehavior, LockType},
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    entities::{
        eval_runs, evidence_bundles, ingested_spans, jobs, normalized_events, targets,
        telemetry_ingest_keys,
    },
    enum_from_string, enum_string, repository_error, serialization_error,
};

#[derive(Clone, Debug)]
pub struct SeaOrmEvaluationRunRepository {
    database: DatabaseConnection,
}

impl SeaOrmEvaluationRunRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl EvaluationRunRepository for SeaOrmEvaluationRunRepository {
    async fn create_run(&self, run: &EvaluationRun) -> Result<(), ApplicationError> {
        run_active_model(run)?
            .insert(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(())
    }

    async fn create_run_with_job(
        &self,
        run: &EvaluationRun,
        job: &DurableJob,
    ) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        run_active_model(run)?
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        jobs::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(job.organization_id.0),
            kind: Set(job.kind.clone()),
            dedupe_key: Set(Some(job.dedupe_key.clone())),
            status: Set("pending".to_owned()),
            payload: Set(serde_json::json!({
                "organization_id": job.organization_id,
                "eval_run_id": job.eval_run_id,
            })),
            attempts: Set(0),
            available_at: Set(job.available_at),
            lease_expires_at: Set(None),
            last_error: Set(None),
            created_at: Set(run.created_at),
            updated_at: Set(run.created_at),
        }
        .insert(&transaction)
        .await
        .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)
    }

    async fn get_run(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvaluationRun>, ApplicationError> {
        eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::Id.eq(eval_run_id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(run_from_model)
            .transpose()
    }

    async fn list_runs(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<EvaluationRun>, ApplicationError> {
        eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .order_by_desc(eval_runs::Column::CreatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(run_from_model)
            .collect()
    }

    async fn get_run_by_external_id(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
        boundary_kind: governance_domain::RunBoundaryKind,
        external_run_id: &str,
    ) -> Result<Option<EvaluationRun>, ApplicationError> {
        eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::TargetId.eq(target_id))
            .filter(eval_runs::Column::BoundaryKind.eq(enum_string(boundary_kind)?))
            .filter(eval_runs::Column::ExternalRunId.eq(external_run_id))
            .filter(eval_runs::Column::State.is_not_in(["cancelled", "failed"]))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(run_from_model)
            .transpose()
    }

    async fn update_run(
        &self,
        run: &EvaluationRun,
        expected_state: EvaluationRunState,
        expected_updated_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError> {
        let result = eval_runs::Entity::update_many()
            .col_expr(
                eval_runs::Column::State,
                Expr::value(enum_string(run.state)?),
            )
            .col_expr(
                eval_runs::Column::CompletionReason,
                Expr::value(run.completion_reason.map(enum_string).transpose()?),
            )
            .col_expr(
                eval_runs::Column::TerminalState,
                Expr::value(run.terminal_state.clone()),
            )
            .col_expr(
                eval_runs::Column::Verdict,
                Expr::value(run.verdict.map(enum_string).transpose()?),
            )
            .col_expr(
                eval_runs::Column::SettleUntil,
                Expr::value(run.settle_until),
            )
            .col_expr(
                eval_runs::Column::HardDeadlineAt,
                Expr::value(Some(run.hard_deadline_at)),
            )
            .col_expr(eval_runs::Column::LastSeenAt, Expr::value(run.last_seen_at))
            .col_expr(
                eval_runs::Column::FinalizedAt,
                Expr::value(run.finalized_at),
            )
            .col_expr(eval_runs::Column::UpdatedAt, Expr::value(run.updated_at))
            .col_expr(
                eval_runs::Column::SpanCount,
                Expr::value(i64::try_from(run.span_count).unwrap_or(i64::MAX)),
            )
            .col_expr(
                eval_runs::Column::TraceCount,
                Expr::value(i64::try_from(run.trace_count).unwrap_or(i64::MAX)),
            )
            .col_expr(
                eval_runs::Column::EventCount,
                Expr::value(i64::try_from(run.event_count).unwrap_or(i64::MAX)),
            )
            .col_expr(
                eval_runs::Column::TraceQuality,
                Expr::value(run.trace_quality.map(enum_string).transpose()?),
            )
            .col_expr(
                eval_runs::Column::EvidenceSha256,
                Expr::value(run.evidence_sha256.clone()),
            )
            .col_expr(
                eval_runs::Column::CompletedAt,
                Expr::value(run.completed_at),
            )
            .filter(eval_runs::Column::OrganizationId.eq(run.organization_id.0))
            .filter(eval_runs::Column::Id.eq(run.id.0))
            .filter(eval_runs::Column::State.eq(enum_string(expected_state)?))
            .filter(eval_runs::Column::UpdatedAt.eq(expected_updated_at))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(result.rows_affected == 1)
    }
}

#[async_trait]
impl TelemetryBoundaryRepository for SeaOrmEvaluationRunRepository {
    async fn get_telemetry_boundary(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
    ) -> Result<Option<TelemetryTargetBoundary>, ApplicationError> {
        let Some(target) = targets::Entity::find()
            .filter(targets::Column::OrganizationId.eq(organization_id.0))
            .filter(targets::Column::Key.eq(target_id))
            .order_by_desc(targets::Column::CreatedAt)
            .one(&self.database)
            .await
            .map_err(repository_error)?
        else {
            return Ok(None);
        };
        let config = target
            .capabilities
            .get("telemetry_boundary")
            .cloned()
            .or_else(|| {
                target
                    .capabilities
                    .get("boundary_kind")
                    .is_some()
                    .then(|| target.capabilities.clone())
            })
            .map(serde_json::from_value::<TelemetryBoundaryConfig>)
            .transpose()
            .map_err(serialization_error)?;
        Ok(config.map(|config| TelemetryTargetBoundary {
            target_version: target.version,
            config,
        }))
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait]
impl TelemetrySpanRepository for SeaOrmEvaluationRunRepository {
    async fn insert_span(
        &self,
        insert: &SpanInsert,
    ) -> Result<SpanInsertOutcome, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let trace_lock = format!(
            "{}:{}:{}",
            insert.organization_id, insert.target_id, insert.span.trace_id
        );
        transaction
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                [trace_lock.into()],
            ))
            .await
            .map_err(repository_error)?;
        let duplicate = ingested_spans::Entity::find()
            .filter(ingested_spans::Column::OrganizationId.eq(insert.organization_id.0))
            .filter(ingested_spans::Column::TargetId.eq(&insert.target_id))
            .filter(ingested_spans::Column::TraceId.eq(&insert.span.trace_id))
            .filter(ingested_spans::Column::SpanId.eq(&insert.span.span_id))
            .one(&transaction)
            .await
            .map_err(repository_error)?;
        if let Some(existing) = duplicate {
            let outcome = if existing.sanitized_payload_sha256 == insert.sanitized_payload_sha256 {
                SpanInsertOutcome::Duplicate
            } else {
                SpanInsertOutcome::Conflict
            };
            transaction.commit().await.map_err(repository_error)?;
            return Ok(outcome);
        }

        let mut late = false;
        let mut locked_run = None;
        if let Some(eval_run_id) = insert.correlation.eval_run_id {
            let trace_owner = ingested_spans::Entity::find()
                .filter(ingested_spans::Column::OrganizationId.eq(insert.organization_id.0))
                .filter(ingested_spans::Column::TargetId.eq(&insert.target_id))
                .filter(ingested_spans::Column::TraceId.eq(&insert.span.trace_id))
                .filter(ingested_spans::Column::EvalRunId.is_not_null())
                .one(&transaction)
                .await
                .map_err(repository_error)?;
            if trace_owner.is_some_and(|owner| owner.eval_run_id != Some(eval_run_id.0)) {
                transaction.commit().await.map_err(repository_error)?;
                return Ok(SpanInsertOutcome::Conflict);
            }
            let run = eval_runs::Entity::find()
                .filter(eval_runs::Column::OrganizationId.eq(insert.organization_id.0))
                .filter(eval_runs::Column::Id.eq(eval_run_id.0))
                .filter(eval_runs::Column::TargetId.eq(&insert.target_id))
                .lock_exclusive()
                .one(&transaction)
                .await
                .map_err(repository_error)?
                .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
            let state: EvaluationRunState = enum_from_string(&run.state)?;
            late = !state.accepts_spans();
            if !late
                && run.span_count >= i64::try_from(insert.max_spans_per_run).unwrap_or(i64::MAX)
            {
                transaction.commit().await.map_err(repository_error)?;
                return Ok(SpanInsertOutcome::Conflict);
            }
            locked_run = Some(run);
        }
        let unassigned = insert.correlation.eval_run_id.is_none();
        let attributes =
            serde_json::to_value(&insert.span.attributes).map_err(serialization_error)?;
        let resource =
            serde_json::to_value(&insert.span.resource_attributes).map_err(serialization_error)?;
        let links = serde_json::to_value(&insert.span.links).map_err(serialization_error)?;
        let active = ingested_spans::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(insert.organization_id.0),
            eval_run_id: Set(insert.correlation.eval_run_id.map(|value| value.0)),
            target_id: Set(insert.target_id.clone()),
            external_run_id: Set(insert.correlation.external_run_id.clone()),
            invocation_id: Set(insert.correlation.invocation_id.map(|value| value.0)),
            scenario_id: Set(insert.correlation.scenario_id.map(|value| value.0)),
            trace_id: Set(insert.span.trace_id.clone()),
            span_id: Set(insert.span.span_id.clone()),
            parent_span_id: Set(insert.span.parent_span_id.clone()),
            links: Set(links),
            resource: Set(resource),
            scope_name: Set(insert.span.instrumentation_scope.clone()),
            scope_version: Set(None),
            name: Set(insert.span.name.clone()),
            status: Set(insert.span.status.clone()),
            started_at: Set(insert.span.started_at),
            ended_at: Set(insert.span.ended_at),
            attributes: Set(attributes),
            sanitized_payload_sha256: Set(insert.sanitized_payload_sha256.clone()),
            correlation_status: Set(if unassigned {
                "unassigned"
            } else if late {
                "late"
            } else {
                "assigned"
            }
            .to_owned()),
            late_after_finalize: Set(late),
            received_at: Set(insert.received_at),
        };
        active
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        if !late && let Some(model) = locked_run {
            let mut active: eval_runs::ActiveModel = model.into();
            active.last_seen_at = Set(Some(insert.received_at));
            active.updated_at = Set(insert.received_at);
            active.span_count = Set(active.span_count.take().unwrap_or_default() + 1);
            active
                .update(&transaction)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)?;
        Ok(if late {
            SpanInsertOutcome::LateAfterFinalize
        } else if unassigned {
            SpanInsertOutcome::Unassigned
        } else {
            SpanInsertOutcome::Inserted
        })
    }

    async fn list_spans(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Vec<ObservedSpan>, ApplicationError> {
        ingested_spans::Entity::find()
            .filter(ingested_spans::Column::OrganizationId.eq(organization_id.0))
            .filter(ingested_spans::Column::EvalRunId.eq(eval_run_id.0))
            .filter(ingested_spans::Column::LateAfterFinalize.eq(false))
            .order_by_asc(ingested_spans::Column::StartedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(span_from_model)
            .collect()
    }
}

#[async_trait]
impl EvidenceBundleRepository for SeaOrmEvaluationRunRepository {
    async fn insert_bundle(&self, bundle: &EvidenceBundle) -> Result<(), ApplicationError> {
        if let Some(existing) = self
            .get_bundle(bundle.organization_id, bundle.eval_run_id)
            .await?
        {
            return if existing.evidence_sha256 == bundle.evidence_sha256 {
                Ok(())
            } else {
                Err(ApplicationError::Conflict(
                    "run already has different immutable evidence".to_owned(),
                ))
            };
        }
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let evidence_insert = evidence_bundles::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(bundle.organization_id.0),
            eval_run_id: Set(bundle.eval_run_id.0),
            schema_version: Set(bundle.schema_version.clone()),
            evidence_sha256: Set(bundle.evidence_sha256.clone()),
            payload: Set(serde_json::to_value(bundle).map_err(serialization_error)?),
            finalized_at: Set(bundle.finalized_at.unwrap_or_else(OffsetDateTime::now_utc)),
        }
        .insert(&transaction)
        .await;
        if let Err(error) = evidence_insert {
            transaction.rollback().await.map_err(repository_error)?;
            if let Some(existing) = self
                .get_bundle(bundle.organization_id, bundle.eval_run_id)
                .await?
            {
                return if existing.evidence_sha256 == bundle.evidence_sha256 {
                    Ok(())
                } else {
                    Err(ApplicationError::Conflict(
                        "run already has different immutable evidence".to_owned(),
                    ))
                };
            }
            return Err(repository_error(error));
        }
        for event in &bundle.events {
            normalized_events::ActiveModel {
                id: Set(event.id.0),
                organization_id: Set(event.organization_id.0),
                eval_run_id: Set(event.eval_run_id.0),
                invocation_id: Set(event.invocation_id.0),
                scenario_id: Set(event.scenario_id.0),
                trace_id: Set(event.trace_id.clone()),
                span_id: Set(event.source_span_id.clone()),
                sequence: Set(i64::try_from(event.sequence).unwrap_or(i64::MAX)),
                event_type: Set(enum_string(event.event_type)?),
                name: Set(event.name.clone()),
                payload: Set(serde_json::to_value(event).map_err(serialization_error)?),
                started_at: Set(event.started_at),
                ended_at: Set(event.ended_at),
                linked_event_ids: Set(
                    serde_json::to_value(&event.linked_event_ids).map_err(serialization_error)?
                ),
            }
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)
    }

    async fn get_bundle(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvidenceBundle>, ApplicationError> {
        evidence_bundles::Entity::find()
            .filter(evidence_bundles::Column::OrganizationId.eq(organization_id.0))
            .filter(evidence_bundles::Column::EvalRunId.eq(eval_run_id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(|model| serde_json::from_value(model.payload).map_err(serialization_error))
            .transpose()
    }
}

#[async_trait]
impl DurableJobRepository for SeaOrmEvaluationRunRepository {
    async fn enqueue(&self, job: &DurableJob) -> Result<(), ApplicationError> {
        let payload = serde_json::json!({
            "organization_id": job.organization_id,
            "eval_run_id": job.eval_run_id,
        });
        let now = OffsetDateTime::now_utc();
        let existing = jobs::Entity::find()
            .filter(jobs::Column::OrganizationId.eq(job.organization_id.0))
            .filter(jobs::Column::Kind.eq(&job.kind))
            .filter(jobs::Column::DedupeKey.eq(&job.dedupe_key))
            .filter(jobs::Column::Status.is_in(["pending", "running"]))
            .one(&self.database)
            .await
            .map_err(repository_error)?;
        if let Some(existing) = existing {
            let available_at = existing.available_at.max(job.available_at);
            let mut active: jobs::ActiveModel = existing.into();
            active.available_at = Set(available_at);
            active.updated_at = Set(now);
            active
                .update(&self.database)
                .await
                .map_err(repository_error)?;
            return Ok(());
        }
        let active = jobs::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(job.organization_id.0),
            kind: Set(job.kind.clone()),
            dedupe_key: Set(Some(job.dedupe_key.clone())),
            status: Set("pending".to_owned()),
            payload: Set(payload),
            attempts: Set(0),
            available_at: Set(job.available_at),
            lease_expires_at: Set(None),
            last_error: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        if let Err(error) = active.insert(&self.database).await {
            let winner = jobs::Entity::find()
                .filter(jobs::Column::OrganizationId.eq(job.organization_id.0))
                .filter(jobs::Column::Kind.eq(&job.kind))
                .filter(jobs::Column::DedupeKey.eq(&job.dedupe_key))
                .filter(jobs::Column::Status.is_in(["pending", "running"]))
                .one(&self.database)
                .await
                .map_err(repository_error)?;
            let Some(winner) = winner else {
                return Err(repository_error(error));
            };
            let available_at = winner.available_at.max(job.available_at);
            let mut active: jobs::ActiveModel = winner.into();
            active.available_at = Set(available_at);
            active.updated_at = Set(now);
            active
                .update(&self.database)
                .await
                .map_err(repository_error)?;
        }
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease_seconds: u64,
    ) -> Result<Option<ClaimedJob>, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let candidate = jobs::Entity::find()
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(jobs::Column::Status.eq("pending"))
                            .add(jobs::Column::AvailableAt.lte(now)),
                    )
                    .add(
                        Condition::all()
                            .add(jobs::Column::Status.eq("running"))
                            .add(jobs::Column::LeaseExpiresAt.lte(now)),
                    ),
            )
            .order_by_asc(jobs::Column::AvailableAt)
            .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
            .one(&transaction)
            .await
            .map_err(repository_error)?;
        let Some(candidate) = candidate else {
            transaction.commit().await.map_err(repository_error)?;
            return Ok(None);
        };
        let eval_run_id = candidate
            .payload
            .get("eval_run_id")
            .cloned()
            .ok_or_else(|| ApplicationError::Repository("job is missing eval_run_id".to_owned()))
            .and_then(|value| serde_json::from_value(value).map_err(serialization_error))?;
        let attempts = candidate.attempts.saturating_add(1);
        let mut active: jobs::ActiveModel = candidate.clone().into();
        active.status = Set("running".to_owned());
        active.attempts = Set(attempts);
        active.lease_expires_at = Set(Some(
            now + time::Duration::seconds(
                i64::try_from(lease_seconds)
                    .unwrap_or(i64::MAX)
                    .clamp(1, 3_600),
            ),
        ));
        active.updated_at = Set(now);
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(Some(ClaimedJob {
            id: candidate.id,
            organization_id: OrganizationId(candidate.organization_id),
            eval_run_id,
            kind: candidate.kind,
            attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        }))
    }

    async fn complete_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        completed_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError> {
        let result = jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("completed"))
            .col_expr(
                jobs::Column::LeaseExpiresAt,
                Expr::value(None::<OffsetDateTime>),
            )
            .col_expr(jobs::Column::UpdatedAt, Expr::value(completed_at))
            .filter(jobs::Column::Id.eq(job_id))
            .filter(jobs::Column::Status.eq("running"))
            .filter(jobs::Column::Attempts.eq(i32::try_from(attempt).unwrap_or(i32::MAX)))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(result.rows_affected == 1)
    }

    async fn fail_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        error: &str,
        retry_at: OffsetDateTime,
        terminal: bool,
    ) -> Result<bool, ApplicationError> {
        let result = jobs::Entity::update_many()
            .col_expr(
                jobs::Column::Status,
                Expr::value(if terminal { "failed" } else { "pending" }),
            )
            .col_expr(jobs::Column::AvailableAt, Expr::value(retry_at))
            .col_expr(
                jobs::Column::LeaseExpiresAt,
                Expr::value(None::<OffsetDateTime>),
            )
            .col_expr(
                jobs::Column::LastError,
                Expr::value(Some(error.chars().take(1_024).collect::<String>())),
            )
            .col_expr(
                jobs::Column::UpdatedAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .filter(jobs::Column::Id.eq(job_id))
            .filter(jobs::Column::Status.eq("running"))
            .filter(jobs::Column::Attempts.eq(i32::try_from(attempt).unwrap_or(i32::MAX)))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(result.rows_affected == 1)
    }

    async fn reschedule_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        available_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError> {
        let result = jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, Expr::value("pending"))
            .col_expr(jobs::Column::AvailableAt, Expr::value(available_at))
            .col_expr(
                jobs::Column::LeaseExpiresAt,
                Expr::value(None::<OffsetDateTime>),
            )
            .col_expr(
                jobs::Column::Attempts,
                Expr::value(i32::try_from(attempt.saturating_sub(1)).unwrap_or(i32::MAX)),
            )
            .col_expr(
                jobs::Column::UpdatedAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .filter(jobs::Column::Id.eq(job_id))
            .filter(jobs::Column::Status.eq("running"))
            .filter(jobs::Column::Attempts.eq(i32::try_from(attempt).unwrap_or(i32::MAX)))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(result.rows_affected == 1)
    }
}

#[async_trait]
impl TelemetryIngestKeyRepository for SeaOrmEvaluationRunRepository {
    async fn rotate_key(
        &self,
        key: &TelemetryIngestKey,
        revoked_at: OffsetDateTime,
    ) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let rotation_lock = format!("{}:{}", key.organization_id, key.target_id);
        transaction
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                [rotation_lock.into()],
            ))
            .await
            .map_err(repository_error)?;
        telemetry_ingest_keys::Entity::update_many()
            .col_expr(
                telemetry_ingest_keys::Column::Status,
                Expr::value("revoked"),
            )
            .col_expr(
                telemetry_ingest_keys::Column::RevokedAt,
                Expr::value(Some(revoked_at)),
            )
            .col_expr(
                telemetry_ingest_keys::Column::UpdatedAt,
                Expr::value(revoked_at),
            )
            .filter(telemetry_ingest_keys::Column::OrganizationId.eq(key.organization_id.0))
            .filter(telemetry_ingest_keys::Column::TargetId.eq(&key.target_id))
            .filter(telemetry_ingest_keys::Column::Status.eq("active"))
            .exec(&transaction)
            .await
            .map_err(repository_error)?;
        telemetry_ingest_keys::ActiveModel {
            id: Set(key.id),
            organization_id: Set(key.organization_id.0),
            target_id: Set(key.target_id.clone()),
            token_prefix: Set(key.token_prefix.clone()),
            token_sha256: Set(key.token_sha256.clone()),
            status: Set("active".to_owned()),
            expires_at: Set(key.expires_at),
            revoked_at: Set(None),
            created_at: Set(key.created_at),
            updated_at: Set(key.created_at),
        }
        .insert(&transaction)
        .await
        .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)
    }

    async fn resolve_key(
        &self,
        token_prefix: &str,
        token_sha256: &str,
        now: OffsetDateTime,
    ) -> Result<Option<TelemetryIngestIdentity>, ApplicationError> {
        let model = telemetry_ingest_keys::Entity::find()
            .filter(telemetry_ingest_keys::Column::TokenPrefix.eq(token_prefix))
            .filter(telemetry_ingest_keys::Column::TokenSha256.eq(token_sha256))
            .filter(telemetry_ingest_keys::Column::Status.eq("active"))
            .one(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(model
            .filter(|model| model.expires_at.is_none_or(|expires_at| expires_at > now))
            .map(|model| TelemetryIngestIdentity {
                organization_id: OrganizationId(model.organization_id),
                target_id: model.target_id,
            }))
    }

    async fn revoke_target_keys(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<(), ApplicationError> {
        telemetry_ingest_keys::Entity::update_many()
            .col_expr(
                telemetry_ingest_keys::Column::Status,
                Expr::value("revoked"),
            )
            .col_expr(
                telemetry_ingest_keys::Column::RevokedAt,
                Expr::value(Some(revoked_at)),
            )
            .col_expr(
                telemetry_ingest_keys::Column::UpdatedAt,
                Expr::value(revoked_at),
            )
            .filter(telemetry_ingest_keys::Column::OrganizationId.eq(organization_id.0))
            .filter(telemetry_ingest_keys::Column::TargetId.eq(target_id))
            .filter(telemetry_ingest_keys::Column::Status.eq("active"))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(())
    }
}

fn run_active_model(run: &EvaluationRun) -> Result<eval_runs::ActiveModel, ApplicationError> {
    Ok(eval_runs::ActiveModel {
        id: Set(run.id.0),
        organization_id: Set(run.organization_id.0),
        target_id: Set(run.target_id.clone()),
        target_version: Set(Some(run.target_version.clone())),
        policy_pack_key: Set(run.policy_pack_key.clone()),
        policy_pack_id: Set(Some(run.policy_pack_id.0)),
        policy_pack_version: Set(Some(
            i32::try_from(run.policy_pack_version).unwrap_or(i32::MAX),
        )),
        policy_content_sha256: Set(Some(run.policy_content_sha256.clone())),
        scenario_id: Set(Some(run.scenario_id.0)),
        rule_ids: Set(serde_json::to_value(&run.rule_ids).map_err(serialization_error)?),
        boundary_kind: Set(enum_string(run.boundary_kind)?),
        external_run_id: Set(run.external_run_id.clone()),
        primary_invocation_id: Set(Some(run.primary_invocation_id.0)),
        state: Set(enum_string(run.state)?),
        completion_reason: Set(run.completion_reason.map(enum_string).transpose()?),
        terminal_state: Set(run.terminal_state.clone()),
        verdict: Set(run.verdict.map(enum_string).transpose()?),
        summary: Set(None),
        settle_until: Set(run.settle_until),
        hard_deadline_at: Set(Some(run.hard_deadline_at)),
        last_seen_at: Set(run.last_seen_at),
        finalized_at: Set(run.finalized_at),
        updated_at: Set(run.updated_at),
        span_count: Set(i64::try_from(run.span_count).unwrap_or(i64::MAX)),
        trace_count: Set(i64::try_from(run.trace_count).unwrap_or(i64::MAX)),
        event_count: Set(i64::try_from(run.event_count).unwrap_or(i64::MAX)),
        trace_quality: Set(run.trace_quality.map(enum_string).transpose()?),
        evidence_sha256: Set(run.evidence_sha256.clone()),
        created_at: Set(run.created_at),
        completed_at: Set(run.completed_at),
    })
}

fn run_from_model(model: eval_runs::Model) -> Result<EvaluationRun, ApplicationError> {
    Ok(EvaluationRun {
        id: EvalRunId(model.id),
        organization_id: OrganizationId(model.organization_id),
        target_id: model.target_id,
        target_version: model.target_version.unwrap_or_else(|| "legacy".to_owned()),
        policy_pack_id: PolicyPackId(model.policy_pack_id.unwrap_or(model.id)),
        policy_pack_key: model.policy_pack_key,
        policy_pack_version: u32::try_from(model.policy_pack_version.unwrap_or_default())
            .unwrap_or_default(),
        policy_content_sha256: model
            .policy_content_sha256
            .unwrap_or_else(|| "legacy".to_owned()),
        scenario_id: ScenarioId(model.scenario_id.unwrap_or(model.id)),
        rule_ids: serde_json::from_value(model.rule_ids).map_err(serialization_error)?,
        boundary_kind: enum_from_string(&model.boundary_kind)?,
        external_run_id: model.external_run_id,
        primary_invocation_id: InvocationId(model.primary_invocation_id.unwrap_or(model.id)),
        state: enum_from_string(&model.state)?,
        completion_reason: model
            .completion_reason
            .map(|value| enum_from_string(&value))
            .transpose()?,
        terminal_state: model.terminal_state,
        verdict: model
            .verdict
            .map(|value| enum_from_string(&value))
            .transpose()?,
        trace_quality: model
            .trace_quality
            .map(|value| enum_from_string(&value))
            .transpose()?,
        evidence_sha256: model.evidence_sha256,
        span_count: u64::try_from(model.span_count).unwrap_or_default(),
        trace_count: u64::try_from(model.trace_count).unwrap_or_default(),
        event_count: u64::try_from(model.event_count).unwrap_or_default(),
        created_at: model.created_at,
        updated_at: model.updated_at,
        last_seen_at: model.last_seen_at,
        settle_until: model.settle_until,
        hard_deadline_at: model.hard_deadline_at.unwrap_or(model.created_at),
        finalized_at: model.finalized_at,
        completed_at: model.completed_at,
    })
}

fn span_from_model(model: ingested_spans::Model) -> Result<ObservedSpan, ApplicationError> {
    Ok(ObservedSpan {
        trace_id: model.trace_id,
        span_id: model.span_id,
        parent_span_id: model.parent_span_id,
        links: serde_json::from_value::<Vec<SpanLink>>(model.links).map_err(serialization_error)?,
        name: model.name,
        started_at: model.started_at,
        ended_at: model.ended_at,
        attributes: serde_json::from_value(model.attributes).map_err(serialization_error)?,
        resource_attributes: serde_json::from_value(model.resource).map_err(serialization_error)?,
        instrumentation_scope: model.scope_name,
        status: model.status,
    })
}
