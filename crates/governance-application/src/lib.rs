//! Application use cases and infrastructure-independent repository contracts.

use async_trait::async_trait;
use governance_domain::{
    Actor, ActorType, EvalRunId, EvaluationSummary, EventType, EvidenceBundle, InvocationId,
    ObservedEvent, OrganizationId, PolicyBundle, PolicyPack, PolicyPackApproval, PolicyPackId,
    RuleResult, RunVerdict, ScenarioId, TargetId, TraceDefect, TraceQualityStatus,
};
use governance_evaluator::evaluate_pack;
use governance_targets::{
    DriverError, RegisteredTarget, RunContext, ScenarioDefinition, TargetDriverRegistry,
    validate_scenario,
};
use governance_telemetry::{
    NormalizationContext, RedactionPolicy, finalize_evidence, normalize_observations,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("repository operation failed: {0}")]
    Repository(String),
    #[error("requested resource was not found: {0}")]
    NotFound(String),
    #[error("operation is not permitted: {0}")]
    Forbidden(String),
    #[error("invalid application request: {0}")]
    InvalidRequest(String),
    #[error("application state conflict: {0}")]
    Conflict(String),
    #[error("target transport failed: {0}")]
    TargetTransport(String),
    #[error("target request timed out: {0}")]
    TargetTimeout(String),
    #[error("target integration contract failed: {0}")]
    TargetContract(String),
}

#[async_trait]
pub trait PolicyPackRepository: Send + Sync {
    async fn get(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
    ) -> Result<Option<PolicyPack>, ApplicationError>;
    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PolicyPack>, ApplicationError>;
    async fn save_bundle(&self, bundle: &PolicyBundle) -> Result<(), ApplicationError>;
    async fn approve(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        approval: &PolicyPackApproval,
    ) -> Result<PolicyPack, ApplicationError>;
}

#[async_trait]
pub trait EvaluationRepository: Send + Sync {
    async fn save_summary(
        &self,
        organization_id: OrganizationId,
        summary: &EvaluationSummary,
    ) -> Result<(), ApplicationError>;
    async fn get_summary(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvaluationSummary>, ApplicationError>;
}

#[async_trait]
pub trait TargetRepository: Send + Sync {
    async fn create(&self, target: &RegisteredTarget) -> Result<(), ApplicationError>;
    async fn get(
        &self,
        organization_id: OrganizationId,
        id: TargetId,
    ) -> Result<Option<RegisteredTarget>, ApplicationError>;
    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RegisteredTarget>, ApplicationError>;
    async fn save_capability_report(
        &self,
        organization_id: OrganizationId,
        id: TargetId,
        report: &governance_targets::CapabilityReport,
    ) -> Result<RegisteredTarget, ApplicationError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvaluationRun {
    pub organization_id: OrganizationId,
    pub target_id: TargetId,
    pub target_name: String,
    pub target_version: String,
    pub policy_pack_key: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,
    pub evidence: EvidenceBundle,
    pub summary: EvaluationSummary,
}

impl StoredEvaluationRun {
    pub fn duration_ms(&self) -> u64 {
        u64::try_from((self.completed_at - self.created_at).whole_milliseconds())
            .unwrap_or_default()
    }
}

#[async_trait]
pub trait LiveEvaluationRepository: Send + Sync {
    async fn save_run(&self, run: &StoredEvaluationRun) -> Result<(), ApplicationError>;
    async fn get_run(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<StoredEvaluationRun>, ApplicationError>;
    async fn list_runs(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<StoredEvaluationRun>, ApplicationError>;
    async fn latest_by_target(
        &self,
        organization_id: OrganizationId,
    ) -> Result<std::collections::BTreeMap<TargetId, StoredEvaluationRun>, ApplicationError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunTargetEvaluationRequest {
    pub target_id: TargetId,
    pub policy_pack_id: PolicyPackId,
    pub scenario: ScenarioDefinition,
}

#[derive(Debug)]
pub struct RunTargetEvaluation<T, P, E, D> {
    targets: T,
    policy_packs: P,
    evaluations: E,
    drivers: D,
}

impl<T, P, E, D> RunTargetEvaluation<T, P, E, D>
where
    T: TargetRepository,
    P: PolicyPackRepository,
    E: LiveEvaluationRepository,
    D: TargetDriverRegistry,
{
    pub fn new(targets: T, policy_packs: P, evaluations: E, drivers: D) -> Self {
        Self {
            targets,
            policy_packs,
            evaluations,
            drivers,
        }
    }

    /// Drives a registered target and persists the resulting evaluation evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is invalid, a resource cannot be loaded,
    /// target execution fails, or the completed run cannot be persisted.
    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        request: RunTargetEvaluationRequest,
    ) -> Result<StoredEvaluationRun, ApplicationError> {
        validate_scenario(&request.scenario)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let target = self
            .targets
            .get(organization_id, request.target_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(request.target_id.to_string()))?;
        let pack = self
            .policy_packs
            .get(organization_id, request.policy_pack_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(request.policy_pack_id.to_string()))?;
        pack.ensure_publishable()
            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;

        let created_at = OffsetDateTime::now_utc();
        let context = RunContext {
            eval_run_id: EvalRunId::new(),
            scenario_id: ScenarioId::new(),
        };
        let invocation_id = InvocationId::new();
        let driver = self.drivers.driver_for(target.manifest.driver_type);
        let session = driver
            .start_session(context.clone())
            .await
            .map_err(application_driver_error)?;
        driver
            .reset(&target.manifest, &session)
            .await
            .map_err(application_driver_error)?;

        let mut observations = Vec::new();
        let mut side_effects = Vec::new();
        let mut terminal = false;
        let mut terminal_state = None;
        for event in &request.scenario.events {
            observations.push(ObservedEvent {
                event_type: EventType::ScenarioInput,
                name: request.scenario.name.clone(),
                actor: Actor {
                    actor_type: ActorType::User,
                    id: "synthetic-user".to_owned(),
                },
                input: event.evidence_input(),
                output: serde_json::Value::Null,
                attributes: std::collections::BTreeMap::new(),
            });
            let mut output = driver
                .send(&target.manifest, &session, event)
                .await
                .map_err(application_driver_error)?;
            terminal = output.terminal;
            terminal_state.clone_from(&output.terminal_state);
            if let Some(state) = &output.terminal_state
                && let Some(final_event) = output
                    .events
                    .iter_mut()
                    .rev()
                    .find(|event| event.event_type == EventType::FinalOutput)
            {
                final_event
                    .attributes
                    .insert("terminal_state".to_owned(), serde_json::json!(state));
            }
            observations.extend(output.events);
            side_effects.extend(output.side_effects);
        }

        if !terminal {
            for observation in &mut observations {
                if observation.event_type == EventType::FinalOutput {
                    observation.event_type = EventType::AgentStart;
                    observation.attributes.remove("terminal_state");
                }
            }
        }

        let trace_id = session.traceparent.split('-').nth(1).unwrap_or("unknown");
        let normalization_context = NormalizationContext {
            organization_id,
            eval_run_id: context.eval_run_id,
            invocation_id,
            scenario_id: context.scenario_id,
        };
        let redaction_policy = RedactionPolicy::default();
        for side_effect in &mut side_effects {
            redaction_policy.redact_value(side_effect);
        }
        let events = normalize_observations(
            normalization_context,
            trace_id,
            &target.manifest.target_id,
            observations,
            &redaction_policy,
        )
        .map_err(|error| ApplicationError::TargetContract(error.to_string()))?;
        let mut evidence = finalize_evidence(
            normalization_context,
            target.manifest.target_version.clone(),
            terminal_state,
            events,
            side_effects,
        );
        if !terminal {
            evidence.trace_quality = TraceQualityStatus::Insufficient;
            if !evidence
                .trace_defects
                .iter()
                .any(|defect| defect.code == "non_terminal")
            {
                evidence.trace_defects.push(TraceDefect {
                    code: "non_terminal".to_owned(),
                    message: "Target response did not declare a terminal result".to_owned(),
                    blocking: true,
                });
            }
        }
        let summary = evaluate_pack(evidence.eval_run_id, &pack.rules, &evidence);
        let completed_at = OffsetDateTime::now_utc();
        let run = StoredEvaluationRun {
            organization_id,
            target_id: target.id,
            target_name: target.name,
            target_version: target.manifest.target_version,
            policy_pack_key: pack.key,
            created_at,
            completed_at,
            evidence,
            summary,
        };
        self.evaluations.save_run(&run).await?;
        tracing::info!(
            eval_run_id = %run.summary.eval_run_id,
            target_id = %run.target_id,
            verdict = ?run.summary.verdict,
            duration_ms = run.duration_ms(),
            "live target evaluation completed"
        );
        Ok(run)
    }
}

#[allow(clippy::needless_pass_by_value)] // Required as a direct `map_err` adapter.
fn application_driver_error(error: DriverError) -> ApplicationError {
    let message = error.to_string();
    match error {
        DriverError::Timeout => ApplicationError::TargetTimeout(message),
        DriverError::Transport | DriverError::Rejected(_) => {
            ApplicationError::TargetTransport(message)
        }
        DriverError::UnsafeConfiguration(_)
        | DriverError::UnsupportedEvent
        | DriverError::ResponseTooLarge
        | DriverError::InvalidResponse
        | DriverError::Contract(_)
        | DriverError::MissingSecretReference(_) => ApplicationError::TargetContract(message),
    }
}

#[derive(Debug)]
pub struct EvaluateEvidence<P, E> {
    policy_packs: P,
    evaluations: E,
}

impl<P, E> EvaluateEvidence<P, E>
where
    P: PolicyPackRepository,
    E: EvaluationRepository,
{
    pub fn new(policy_packs: P, evaluations: E) -> Self {
        Self {
            policy_packs,
            evaluations,
        }
    }

    /// Evaluates a finalized evidence bundle against an approved policy pack.
    ///
    /// # Errors
    ///
    /// Returns an error when the policy is missing or unpublishable, or when the
    /// resulting summary cannot be persisted.
    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        policy_pack_id: PolicyPackId,
        evidence: &EvidenceBundle,
    ) -> Result<EvaluationSummary, ApplicationError> {
        let pack = self
            .policy_packs
            .get(organization_id, policy_pack_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(policy_pack_id.to_string()))?;
        pack.ensure_publishable()
            .map_err(|error| ApplicationError::Conflict(error.to_string()))?;
        let summary = evaluate_pack(evidence.eval_run_id, &pack.rules, evidence);
        self.evaluations
            .save_summary(organization_id, &summary)
            .await?;
        Ok(summary)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub active_agents: u32,
    pub policy_packs: u32,
    pub evaluations_30d: u32,
    pub pass_rate: f64,
    pub open_findings: u32,
    pub trace_coverage: f64,
    pub recent_runs: Vec<RunListItem>,
    pub daily_activity: Vec<ActivityPoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunListItem {
    pub id: EvalRunId,
    pub target: String,
    pub policy_pack: String,
    pub verdict: RunVerdict,
    pub passed: usize,
    pub failed: usize,
    pub inconclusive: usize,
    pub duration_ms: u64,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityPoint {
    pub day: String,
    pub passed: u32,
    pub failed: u32,
    pub inconclusive: u32,
}

pub fn summarize_results(results: &[RuleResult]) -> (usize, usize, usize) {
    let passed = results
        .iter()
        .filter(|result| result.status == governance_domain::RuleStatus::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == governance_domain::RuleStatus::Fail)
        .count();
    (
        passed,
        failed,
        results.len().saturating_sub(passed + failed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_results_have_zero_counts() {
        assert_eq!(summarize_results(&[]), (0, 0, 0));
    }
}
