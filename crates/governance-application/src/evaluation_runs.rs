#![allow(clippy::missing_errors_doc)]

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use governance_domain::{
    CompletionReason, EvalRunId, EvaluationRun, EvaluationRunState, EvaluationSummary,
    EvidenceBundle, InvocationId, OrganizationId, PolicyPackId, RunBoundaryKind, ScenarioId,
};
use governance_evaluator::evaluate_pack;
use governance_targets::TelemetryBoundaryConfig;
use governance_telemetry::{
    CorrelationCandidate, FinalizationMetadata, ObservedSpan, RedactionPolicy, TelemetryLimits,
    extract_span_correlation, finalize_observed_spans,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{ApplicationError, EvaluationRepository, PolicyPackRepository};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateEvaluationRunRequest {
    pub organization_id: OrganizationId,
    pub target_id: String,
    pub target_version: String,
    pub policy_pack_id: PolicyPackId,
    pub scenario_id: ScenarioId,
    #[serde(default)]
    pub rule_ids: Vec<String>,
    pub boundary_kind: RunBoundaryKind,
    pub external_run_id: Option<String>,
    pub invocation_id: Option<InvocationId>,
    #[serde(default = "default_max_duration_seconds")]
    pub max_duration_seconds: u64,
}

const fn default_max_duration_seconds() -> u64 {
    3_600
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub reason: CompletionReason,
    pub terminal_state: Option<String>,
    #[serde(default = "default_settle_seconds")]
    pub settle_seconds: u64,
}

const fn default_settle_seconds() -> u64 {
    10
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DurableJob {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub kind: String,
    pub dedupe_key: String,
    #[serde(with = "time::serde::rfc3339")]
    pub available_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub kind: String,
    pub attempts: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanInsertOutcome {
    Inserted,
    Duplicate,
    Conflict,
    Unassigned,
    LateAfterFinalize,
}

#[derive(Clone, Debug)]
pub struct SpanInsert {
    pub organization_id: OrganizationId,
    pub target_id: String,
    pub correlation: CorrelationCandidate,
    pub span: ObservedSpan,
    pub sanitized_payload_sha256: String,
    pub received_at: OffsetDateTime,
    pub max_spans_per_run: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IngestBatchResult {
    pub accepted: usize,
    pub rejected: usize,
    pub duplicates: usize,
    pub conflicts: usize,
    pub unassigned: usize,
    pub late: usize,
}

#[derive(Clone, Debug)]
pub struct TelemetryIngestIdentity {
    pub organization_id: OrganizationId,
    pub target_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryTargetBoundary {
    pub target_version: String,
    pub config: TelemetryBoundaryConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryIngestKey {
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub target_id: String,
    pub token_prefix: String,
    pub token_sha256: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotatedTelemetryIngestKey {
    pub key: TelemetryIngestKey,
    pub plaintext: String,
}

#[async_trait]
pub trait EvaluationRunRepository: Send + Sync {
    async fn create_run(&self, run: &EvaluationRun) -> Result<(), ApplicationError>;
    async fn create_run_with_job(
        &self,
        run: &EvaluationRun,
        job: &DurableJob,
    ) -> Result<(), ApplicationError>;
    async fn get_run(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvaluationRun>, ApplicationError>;
    async fn list_runs(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<EvaluationRun>, ApplicationError>;
    async fn get_run_by_external_id(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
        boundary_kind: RunBoundaryKind,
        external_run_id: &str,
    ) -> Result<Option<EvaluationRun>, ApplicationError>;
    async fn update_run(
        &self,
        run: &EvaluationRun,
        expected_state: EvaluationRunState,
        expected_updated_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError>;
}

async fn persist_run_update<R: EvaluationRunRepository>(
    repository: &R,
    run: &EvaluationRun,
    expected_state: EvaluationRunState,
    expected_updated_at: OffsetDateTime,
) -> Result<(), ApplicationError> {
    if repository
        .update_run(run, expected_state, expected_updated_at)
        .await?
    {
        Ok(())
    } else {
        Err(ApplicationError::Conflict(format!(
            "evaluation run {} changed concurrently",
            run.id
        )))
    }
}

#[async_trait]
pub trait TelemetryBoundaryRepository: Send + Sync {
    async fn get_telemetry_boundary(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
    ) -> Result<Option<TelemetryTargetBoundary>, ApplicationError>;
}

#[async_trait]
pub trait TelemetrySpanRepository: Send + Sync {
    async fn insert_span(&self, insert: &SpanInsert)
    -> Result<SpanInsertOutcome, ApplicationError>;
    async fn list_spans(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Vec<ObservedSpan>, ApplicationError>;
}

#[async_trait]
pub trait EvidenceBundleRepository: Send + Sync {
    async fn insert_bundle(&self, bundle: &EvidenceBundle) -> Result<(), ApplicationError>;
    async fn get_bundle(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvidenceBundle>, ApplicationError>;
}

#[async_trait]
pub trait DurableJobRepository: Send + Sync {
    async fn enqueue(&self, job: &DurableJob) -> Result<(), ApplicationError>;
    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease_seconds: u64,
    ) -> Result<Option<ClaimedJob>, ApplicationError>;
    async fn complete_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        completed_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError>;
    async fn fail_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        error: &str,
        retry_at: OffsetDateTime,
        terminal: bool,
    ) -> Result<bool, ApplicationError>;
    async fn reschedule_job(
        &self,
        job_id: Uuid,
        attempt: u32,
        available_at: OffsetDateTime,
    ) -> Result<bool, ApplicationError>;
}

#[async_trait]
pub trait TelemetryIngestKeyRepository: Send + Sync {
    async fn rotate_key(
        &self,
        key: &TelemetryIngestKey,
        revoked_at: OffsetDateTime,
    ) -> Result<(), ApplicationError>;
    async fn resolve_key(
        &self,
        token_prefix: &str,
        token_sha256: &str,
        now: OffsetDateTime,
    ) -> Result<Option<TelemetryIngestIdentity>, ApplicationError>;
    async fn revoke_target_keys(
        &self,
        organization_id: OrganizationId,
        target_id: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<(), ApplicationError>;
}

#[derive(Debug)]
pub struct CreateEvaluationRun<P, R, J> {
    policy_packs: P,
    runs: R,
    jobs: J,
}

impl<P, R, J> CreateEvaluationRun<P, R, J>
where
    P: PolicyPackRepository,
    R: EvaluationRunRepository,
    J: DurableJobRepository,
{
    pub fn new(policy_packs: P, runs: R, jobs: J) -> Self {
        Self {
            policy_packs,
            runs,
            jobs,
        }
    }

    pub async fn execute(
        &self,
        request: CreateEvaluationRunRequest,
    ) -> Result<EvaluationRun, ApplicationError> {
        let pack = self
            .policy_packs
            .get(request.organization_id, request.policy_pack_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(request.policy_pack_id.to_string()))?;
        pack.ensure_publishable()
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        validate_rule_scope(&request.rule_ids, &pack.rules)?;
        let now = OffsetDateTime::now_utc();
        let max_duration = i64::try_from(request.max_duration_seconds)
            .unwrap_or(i64::MAX)
            .clamp(1, 86_400);
        let run = EvaluationRun {
            id: EvalRunId::new(),
            organization_id: request.organization_id,
            target_id: request.target_id,
            target_version: request.target_version,
            policy_pack_id: pack.id,
            policy_pack_key: pack.key,
            policy_pack_version: pack.version,
            policy_content_sha256: pack.content_sha256,
            scenario_id: request.scenario_id,
            rule_ids: request.rule_ids,
            boundary_kind: request.boundary_kind,
            external_run_id: request.external_run_id,
            primary_invocation_id: request.invocation_id.unwrap_or_default(),
            state: EvaluationRunState::Created,
            completion_reason: None,
            terminal_state: None,
            verdict: None,
            trace_quality: None,
            evidence_sha256: None,
            span_count: 0,
            trace_count: 0,
            event_count: 0,
            created_at: now,
            updated_at: now,
            last_seen_at: None,
            settle_until: None,
            hard_deadline_at: now + Duration::seconds(max_duration),
            finalized_at: None,
            completed_at: None,
        };
        let timeout_job = DurableJob {
            organization_id: run.organization_id,
            eval_run_id: run.id,
            kind: "evaluation_run_timeout".to_owned(),
            dedupe_key: format!("timeout:{}", run.id),
            available_at: run.hard_deadline_at,
        };
        if let Err(error) = self.runs.create_run_with_job(&run, &timeout_job).await {
            if let Some(external_run_id) = run.external_run_id.as_deref()
                && let Some(winner) = self
                    .runs
                    .get_run_by_external_id(
                        run.organization_id,
                        &run.target_id,
                        run.boundary_kind,
                        external_run_id,
                    )
                    .await?
            {
                self.jobs
                    .enqueue(&DurableJob {
                        organization_id: winner.organization_id,
                        eval_run_id: winner.id,
                        kind: "evaluation_run_timeout".to_owned(),
                        dedupe_key: format!("timeout:{}", winner.id),
                        available_at: winner.hard_deadline_at,
                    })
                    .await?;
                return Ok(winner);
            }
            return Err(error);
        }
        Ok(run)
    }
}

#[derive(Debug)]
pub struct IngestTelemetryBatch<P, R, S, J, T> {
    policy_packs: P,
    runs: R,
    spans: S,
    jobs: J,
    boundaries: T,
    redaction: RedactionPolicy,
    limits: TelemetryLimits,
    settle_seconds: u64,
    idle_timeout_seconds: u64,
    max_run_duration_seconds: u64,
}

impl<P, R, S, J, T> IngestTelemetryBatch<P, R, S, J, T>
where
    P: PolicyPackRepository + Clone,
    R: EvaluationRunRepository + Clone,
    S: TelemetrySpanRepository,
    J: DurableJobRepository + Clone,
    T: TelemetryBoundaryRepository,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        policy_packs: P,
        runs: R,
        spans: S,
        jobs: J,
        boundaries: T,
        redaction: RedactionPolicy,
        limits: TelemetryLimits,
        settle_seconds: u64,
        idle_timeout_seconds: u64,
        max_run_duration_seconds: u64,
    ) -> Self {
        Self {
            policy_packs,
            runs,
            spans,
            jobs,
            boundaries,
            redaction,
            limits,
            settle_seconds,
            idle_timeout_seconds,
            max_run_duration_seconds,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        identity: &TelemetryIngestIdentity,
        spans: Vec<ObservedSpan>,
    ) -> Result<IngestBatchResult, ApplicationError> {
        if spans.len() > self.limits.max_spans_per_request {
            return Err(ApplicationError::InvalidRequest(
                "telemetry span count exceeded".to_owned(),
            ));
        }
        let boundary = self
            .boundaries
            .get_telemetry_boundary(identity.organization_id, &identity.target_id)
            .await?;
        let redaction = boundary.as_ref().map_or_else(
            || self.redaction.clone(),
            |target| {
                let mut allowed = target.config.external_id_attributes.clone();
                allowed.extend(target.config.terminal_attribute.clone());
                self.redaction.clone().with_allowed_attributes(allowed)
            },
        );
        let settle_duration = Duration::seconds(configured_settle_seconds(
            boundary.as_ref(),
            self.settle_seconds,
        ));
        let mut result = IngestBatchResult::default();
        for mut span in spans {
            if self.limits.validate_span(&span).is_err() {
                result.rejected += 1;
                continue;
            }
            redaction.redact_span(&mut span);
            let Ok(mut correlation) = extract_span_correlation(&span) else {
                result.rejected += 1;
                continue;
            };
            if !correlation.terminal
                && let Some(target) = boundary.as_ref()
                && let Some(attribute) = target.config.terminal_attribute.as_deref()
            {
                correlation.terminal = merged_span_attributes(&span)
                    .get(attribute)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
            }
            if correlation.eval_run_id.is_none() {
                correlation.external_run_id = correlation.external_run_id.or_else(|| {
                    boundary
                        .as_ref()
                        .and_then(|target| configured_external_run_id(&span, &target.config))
                });
                if let (Some(target), Some(external_run_id)) =
                    (boundary.as_ref(), correlation.external_run_id.as_deref())
                {
                    let run = if let Some(run) = self
                        .runs
                        .get_run_by_external_id(
                            identity.organization_id,
                            &identity.target_id,
                            target.config.boundary_kind,
                            external_run_id,
                        )
                        .await?
                    {
                        Some(run)
                    } else if let Some(policy_pack_id) = target.config.default_policy_pack_id {
                        Some(
                            CreateEvaluationRun::new(
                                self.policy_packs.clone(),
                                self.runs.clone(),
                                self.jobs.clone(),
                            )
                            .execute(CreateEvaluationRunRequest {
                                organization_id: identity.organization_id,
                                target_id: identity.target_id.clone(),
                                target_version: target.target_version.clone(),
                                policy_pack_id,
                                scenario_id: correlation.scenario_id.unwrap_or_default(),
                                rule_ids: Vec::new(),
                                boundary_kind: target.config.boundary_kind,
                                external_run_id: Some(external_run_id.to_owned()),
                                invocation_id: correlation.invocation_id,
                                max_duration_seconds: target
                                    .config
                                    .max_duration_seconds
                                    .unwrap_or(self.max_run_duration_seconds)
                                    .min(self.max_run_duration_seconds),
                            })
                            .await?,
                        )
                    } else {
                        None
                    };
                    if let Some(run) = run {
                        correlation.eval_run_id = Some(run.id);
                        correlation.invocation_id = correlation
                            .invocation_id
                            .or(Some(run.primary_invocation_id));
                        correlation.scenario_id = correlation.scenario_id.or(Some(run.scenario_id));
                    }
                }
            }
            if let Some(run_id) = correlation.eval_run_id {
                let run = self.runs.get_run(identity.organization_id, run_id).await?;
                let authorized = run.as_ref().is_some_and(|run| {
                    run.target_id == identity.target_id
                        && run.span_count
                            < u64::try_from(self.limits.max_spans_per_run).unwrap_or(u64::MAX)
                });
                if !authorized {
                    result.rejected += 1;
                    continue;
                }
            }
            let sanitized = serde_json::to_vec(&span)
                .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
            let correlated_run_id = correlation.eval_run_id;
            let terminal = correlation.terminal;
            let terminal_state = correlation.terminal_state.clone();
            let outcome = self
                .spans
                .insert_span(&SpanInsert {
                    organization_id: identity.organization_id,
                    target_id: identity.target_id.clone(),
                    correlation,
                    span,
                    sanitized_payload_sha256: format!("{:x}", Sha256::digest(sanitized)),
                    received_at: OffsetDateTime::now_utc(),
                    max_spans_per_run: self.limits.max_spans_per_run,
                })
                .await?;
            if matches!(
                outcome,
                SpanInsertOutcome::Inserted | SpanInsertOutcome::Duplicate
            ) && let Some(eval_run_id) = correlated_run_id
            {
                let is_new_span = outcome == SpanInsertOutcome::Inserted;
                let mut run = None;
                for attempt in 0..8 {
                    let mut candidate = self
                        .runs
                        .get_run(identity.organization_id, eval_run_id)
                        .await?
                        .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
                    let expected_state = candidate.state;
                    let expected_updated_at = candidate.updated_at;
                    let now = OffsetDateTime::now_utc();
                    let changed = if terminal
                        && matches!(
                            candidate.state,
                            EvaluationRunState::Created | EvaluationRunState::Collecting
                        ) {
                        candidate
                            .begin_settling(
                                CompletionReason::TerminalEvent,
                                terminal_state.clone(),
                                now + settle_duration,
                                now,
                            )
                            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
                        true
                    } else if candidate.state == EvaluationRunState::Created {
                        candidate
                            .transition_to(EvaluationRunState::Collecting, now)
                            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
                        true
                    } else if candidate.state == EvaluationRunState::Settling && is_new_span {
                        let proposed = (now + settle_duration).min(candidate.hard_deadline_at);
                        let extended = candidate
                            .settle_until
                            .map_or(proposed, |current| current.max(proposed));
                        if candidate.settle_until == Some(extended) {
                            false
                        } else {
                            candidate.settle_until = Some(extended);
                            candidate.updated_at = now;
                            true
                        }
                    } else {
                        false
                    };
                    if !changed
                        || self
                            .runs
                            .update_run(&candidate, expected_state, expected_updated_at)
                            .await?
                    {
                        run = Some(candidate);
                        break;
                    }
                    if attempt == 7 {
                        return Err(ApplicationError::Conflict(format!(
                            "evaluation run {eval_run_id} changed during telemetry ingestion"
                        )));
                    }
                }
                let run = run.ok_or_else(|| {
                    ApplicationError::Conflict(format!(
                        "evaluation run {eval_run_id} could not be reconciled"
                    ))
                })?;
                let now = OffsetDateTime::now_utc();
                if run.state == EvaluationRunState::Settling {
                    self.jobs
                        .enqueue(&DurableJob {
                            organization_id: run.organization_id,
                            eval_run_id: run.id,
                            kind: "finalize_evaluation_run".to_owned(),
                            dedupe_key: format!("finalize:{}", run.id),
                            available_at: run.settle_until.unwrap_or(now),
                        })
                        .await?;
                } else if run.state == EvaluationRunState::Collecting {
                    let idle_timeout = boundary
                        .as_ref()
                        .and_then(|target| target.config.idle_timeout_seconds)
                        .unwrap_or(self.idle_timeout_seconds)
                        .clamp(1, self.max_run_duration_seconds);
                    self.jobs
                        .enqueue(&DurableJob {
                            organization_id: run.organization_id,
                            eval_run_id: run.id,
                            kind: "evaluation_run_idle_timeout".to_owned(),
                            dedupe_key: format!("idle:{}", run.id),
                            available_at: now
                                + Duration::seconds(
                                    i64::try_from(idle_timeout).unwrap_or(i64::MAX),
                                ),
                        })
                        .await?;
                }
            }
            match outcome {
                SpanInsertOutcome::Inserted => result.accepted += 1,
                SpanInsertOutcome::Duplicate => {
                    result.accepted += 1;
                    result.duplicates += 1;
                }
                SpanInsertOutcome::Conflict => {
                    result.rejected += 1;
                    result.conflicts += 1;
                }
                SpanInsertOutcome::Unassigned => {
                    result.accepted += 1;
                    result.unassigned += 1;
                }
                SpanInsertOutcome::LateAfterFinalize => {
                    result.rejected += 1;
                    result.late += 1;
                }
            }
        }
        Ok(result)
    }
}

#[derive(Debug)]
pub struct CompleteEvaluationRun<R, J> {
    runs: R,
    jobs: J,
}

#[derive(Debug)]
pub struct CancelEvaluationRun<R> {
    runs: R,
}

impl<R: EvaluationRunRepository> CancelEvaluationRun<R> {
    pub fn new(runs: R) -> Self {
        Self { runs }
    }

    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<EvaluationRun, ApplicationError> {
        let mut run = self
            .runs
            .get_run(organization_id, eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
        let expected_state = run.state;
        let expected_updated_at = run.updated_at;
        run.transition_to(EvaluationRunState::Cancelled, OffsetDateTime::now_utc())
            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
        Ok(run)
    }
}

impl<R, J> CompleteEvaluationRun<R, J>
where
    R: EvaluationRunRepository,
    J: DurableJobRepository,
{
    pub fn new(runs: R, jobs: J) -> Self {
        Self { runs, jobs }
    }

    pub async fn execute(
        &self,
        request: CompletionRequest,
    ) -> Result<EvaluationRun, ApplicationError> {
        let mut run = self
            .runs
            .get_run(request.organization_id, request.eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(request.eval_run_id.to_string()))?;
        let expected_state = run.state;
        let expected_updated_at = run.updated_at;
        let now = OffsetDateTime::now_utc();
        let settle_seconds = i64::try_from(request.settle_seconds)
            .unwrap_or(i64::MAX)
            .clamp(0, 300);
        run.begin_settling(
            request.reason,
            request.terminal_state,
            now + Duration::seconds(settle_seconds),
            now,
        )
        .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
        self.jobs
            .enqueue(&DurableJob {
                organization_id: run.organization_id,
                eval_run_id: run.id,
                kind: "finalize_evaluation_run".to_owned(),
                dedupe_key: format!("finalize:{}", run.id),
                available_at: run.settle_until.unwrap_or(now),
            })
            .await?;
        Ok(run)
    }
}

#[derive(Debug)]
pub struct FinalizeEvaluationRun<R, S, E, J> {
    runs: R,
    spans: S,
    evidence: E,
    jobs: J,
}

impl<R, S, E, J> FinalizeEvaluationRun<R, S, E, J>
where
    R: EvaluationRunRepository,
    S: TelemetrySpanRepository,
    E: EvidenceBundleRepository,
    J: DurableJobRepository,
{
    pub fn new(runs: R, spans: S, evidence: E, jobs: J) -> Self {
        Self {
            runs,
            spans,
            evidence,
            jobs,
        }
    }

    #[allow(clippy::too_many_lines)] // Recovery and first-finalization paths stay adjacent.
    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<EvidenceBundle, ApplicationError> {
        if let Some(bundle) = self
            .evidence
            .get_bundle(organization_id, eval_run_id)
            .await?
        {
            let mut run = self
                .runs
                .get_run(organization_id, eval_run_id)
                .await?
                .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
            if run.state == EvaluationRunState::Finalizing {
                let now = OffsetDateTime::now_utc();
                let expected_state = run.state;
                let expected_updated_at = run.updated_at;
                run.trace_count = u64::try_from(bundle.trace_ids.len()).unwrap_or(u64::MAX);
                run.event_count = u64::try_from(bundle.events.len()).unwrap_or(u64::MAX);
                run.trace_quality = Some(bundle.trace_quality);
                run.evidence_sha256 = Some(bundle.evidence_sha256.clone());
                run.finalized_at = bundle.finalized_at;
                run.transition_to(EvaluationRunState::Evaluating, now)
                    .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
                persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
                self.jobs
                    .enqueue(&DurableJob {
                        organization_id,
                        eval_run_id,
                        kind: "evaluate_evidence".to_owned(),
                        dedupe_key: format!("evaluate:{eval_run_id}"),
                        available_at: now,
                    })
                    .await?;
            }
            return Ok(bundle);
        }
        let mut run = self
            .runs
            .get_run(organization_id, eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
        let now = OffsetDateTime::now_utc();
        if run.state == EvaluationRunState::Settling {
            if run.settle_until.is_some_and(|deadline| deadline > now) {
                return Err(ApplicationError::Conflict(
                    "run is still settling".to_owned(),
                ));
            }
            let expected_state = run.state;
            let expected_updated_at = run.updated_at;
            run.transition_to(EvaluationRunState::Finalizing, now)
                .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
            persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
        } else if run.state != EvaluationRunState::Finalizing {
            return Err(ApplicationError::Conflict(format!(
                "run cannot finalize from {:?}",
                run.state
            )));
        }
        let spans = self.spans.list_spans(organization_id, eval_run_id).await?;
        let span_count = spans.len();
        let finalization_redaction = finalization_redaction(&spans);
        let bundle = finalize_observed_spans(
            governance_telemetry::NormalizationContext {
                organization_id,
                eval_run_id,
                invocation_id: run.primary_invocation_id,
                scenario_id: run.scenario_id,
            },
            FinalizationMetadata {
                target_version: run.target_version.clone(),
                policy_content_sha256: run.policy_content_sha256.clone(),
                completion_reason: run.completion_reason,
                terminal_state: run.terminal_state.clone(),
                finalized_at: now,
            },
            spans,
            vec![],
            &finalization_redaction,
        );
        self.evidence.insert_bundle(&bundle).await?;
        run.span_count = u64::try_from(span_count).unwrap_or(u64::MAX);
        run.trace_count = u64::try_from(bundle.trace_ids.len()).unwrap_or(u64::MAX);
        run.event_count = u64::try_from(bundle.events.len()).unwrap_or(u64::MAX);
        run.trace_quality = Some(bundle.trace_quality);
        run.evidence_sha256 = Some(bundle.evidence_sha256.clone());
        run.finalized_at = Some(now);
        let expected_state = run.state;
        let expected_updated_at = run.updated_at;
        run.transition_to(EvaluationRunState::Evaluating, now)
            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
        self.jobs
            .enqueue(&DurableJob {
                organization_id,
                eval_run_id,
                kind: "evaluate_evidence".to_owned(),
                dedupe_key: format!("evaluate:{eval_run_id}"),
                available_at: now,
            })
            .await?;
        Ok(bundle)
    }
}

#[derive(Debug)]
pub struct EvaluateFinalizedRun<P, R, E, V> {
    policy_packs: P,
    runs: R,
    evidence: E,
    evaluations: V,
}

impl<P, R, E, V> EvaluateFinalizedRun<P, R, E, V>
where
    P: PolicyPackRepository,
    R: EvaluationRunRepository,
    E: EvidenceBundleRepository,
    V: EvaluationRepository,
{
    pub fn new(policy_packs: P, runs: R, evidence: E, evaluations: V) -> Self {
        Self {
            policy_packs,
            runs,
            evidence,
            evaluations,
        }
    }

    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<EvaluationSummary, ApplicationError> {
        if let Some(summary) = self
            .evaluations
            .get_summary(organization_id, eval_run_id)
            .await?
        {
            let mut run = self
                .runs
                .get_run(organization_id, eval_run_id)
                .await?
                .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
            if run.state == EvaluationRunState::Evaluating {
                let expected_state = run.state;
                let expected_updated_at = run.updated_at;
                run.verdict = Some(summary.verdict);
                run.transition_to(EvaluationRunState::Completed, OffsetDateTime::now_utc())
                    .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
                persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
            }
            return Ok(summary);
        }
        let run = self
            .runs
            .get_run(organization_id, eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
        let bundle = self
            .evidence
            .get_bundle(organization_id, eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(format!("evidence for {eval_run_id}")))?;
        let pack = self
            .policy_packs
            .get(organization_id, run.policy_pack_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(run.policy_pack_id.to_string()))?;
        if pack.content_sha256 != run.policy_content_sha256 {
            return Err(ApplicationError::Conflict(
                "pinned policy content changed".to_owned(),
            ));
        }
        let selected = if run.rule_ids.is_empty() {
            pack.rules
        } else {
            pack.rules
                .into_iter()
                .filter(|rule| run.rule_ids.contains(&rule.id))
                .collect()
        };
        let summary = evaluate_pack(eval_run_id, &selected, &bundle);
        self.evaluations
            .save_summary(organization_id, &summary)
            .await?;
        let mut run = self
            .runs
            .get_run(organization_id, eval_run_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(eval_run_id.to_string()))?;
        let expected_state = run.state;
        let expected_updated_at = run.updated_at;
        run.verdict = Some(summary.verdict);
        run.transition_to(EvaluationRunState::Completed, OffsetDateTime::now_utc())
            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        persist_run_update(&self.runs, &run, expected_state, expected_updated_at).await?;
        Ok(summary)
    }
}

#[derive(Debug)]
pub struct RotateTelemetryIngestKey<K> {
    keys: K,
}

impl<K: TelemetryIngestKeyRepository> RotateTelemetryIngestKey<K> {
    pub fn new(keys: K) -> Self {
        Self { keys }
    }

    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        target_id: String,
        expires_at: Option<OffsetDateTime>,
    ) -> Result<RotatedTelemetryIngestKey, ApplicationError> {
        let now = OffsetDateTime::now_utc();
        let mut bytes = [0_u8; 32];
        rand::rng().fill(&mut bytes);
        let plaintext = format!("flt_{}", URL_SAFE_NO_PAD.encode(bytes));
        let digest = format!("{:x}", Sha256::digest(plaintext.as_bytes()));
        let key = TelemetryIngestKey {
            id: Uuid::now_v7(),
            organization_id,
            target_id,
            token_prefix: plaintext.chars().take(12).collect(),
            token_sha256: digest,
            created_at: now,
            expires_at,
        };
        self.keys.rotate_key(&key, now).await?;
        Ok(RotatedTelemetryIngestKey { key, plaintext })
    }
}

fn validate_rule_scope(
    requested: &[String],
    rules: &[governance_domain::CompiledRule],
) -> Result<(), ApplicationError> {
    let available: BTreeSet<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    if let Some(unknown) = requested.iter().find(|id| !available.contains(id.as_str())) {
        return Err(ApplicationError::InvalidRequest(format!(
            "rule {unknown} is not part of the pinned policy pack"
        )));
    }
    Ok(())
}

fn configured_external_run_id(
    span: &ObservedSpan,
    config: &TelemetryBoundaryConfig,
) -> Option<String> {
    let attributes = merged_span_attributes(span);
    config
        .external_id_attributes
        .iter()
        .filter(|attribute| {
            attribute.as_str() != "gen_ai.conversation.id"
                || config.conversation_id_is_task_boundary
        })
        .find_map(|attribute| {
            attributes
                .get(attribute)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn configured_settle_seconds(boundary: Option<&TelemetryTargetBoundary>, fallback: u64) -> i64 {
    i64::try_from(
        boundary
            .map_or(fallback, |target| target.config.settle_seconds)
            .min(300),
    )
    .unwrap_or(300)
}

fn finalization_redaction(spans: &[ObservedSpan]) -> RedactionPolicy {
    let allowed_attributes = spans.iter().flat_map(|span| {
        span.attributes
            .keys()
            .chain(span.resource_attributes.keys())
            .cloned()
    });
    RedactionPolicy::default().with_allowed_attributes(allowed_attributes)
}

fn merged_span_attributes(span: &ObservedSpan) -> BTreeMap<String, serde_json::Value> {
    let mut attributes = span.resource_attributes.clone();
    attributes.extend(span.attributes.clone());
    attributes
}

#[cfg(test)]
mod tests {
    use governance_domain::RunBoundaryKind;
    use serde_json::json;

    use super::*;

    fn span_with_conversation_id() -> ObservedSpan {
        ObservedSpan {
            trace_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            span_id: "aaaaaaaaaaaaaaaa".to_owned(),
            parent_span_id: None,
            links: Vec::new(),
            name: "invoke_agent".to_owned(),
            started_at: OffsetDateTime::UNIX_EPOCH,
            ended_at: None,
            attributes: BTreeMap::from([(
                "gen_ai.conversation.id".to_owned(),
                json!("conversation-42"),
            )]),
            resource_attributes: BTreeMap::new(),
            instrumentation_scope: None,
            status: None,
        }
    }

    #[test]
    fn target_must_opt_in_to_conversation_task_boundaries() {
        let span = span_with_conversation_id();
        let mut config = TelemetryBoundaryConfig {
            boundary_kind: RunBoundaryKind::AgentTask,
            external_id_attributes: vec!["gen_ai.conversation.id".to_owned()],
            ..TelemetryBoundaryConfig::default()
        };

        assert_eq!(configured_external_run_id(&span, &config), None);
        config.conversation_id_is_task_boundary = true;
        assert_eq!(
            configured_external_run_id(&span, &config).as_deref(),
            Some("conversation-42")
        );
    }

    #[test]
    fn target_settle_window_overrides_gateway_default() {
        let boundary = TelemetryTargetBoundary {
            target_version: "git:test".to_owned(),
            config: TelemetryBoundaryConfig {
                settle_seconds: 42,
                ..TelemetryBoundaryConfig::default()
            },
        };
        assert_eq!(configured_settle_seconds(Some(&boundary), 10), 42);
        assert_eq!(configured_settle_seconds(None, 10), 10);
    }
}
