//! Application use cases and infrastructure-independent repository contracts.

use async_trait::async_trait;
use governance_domain::{
    EvalRunId, EvaluationSummary, EvidenceBundle, OrganizationId, PolicyBundle, PolicyPack,
    PolicyPackApproval, PolicyPackId, PolicyPackStatusChange, RuleResult, RunVerdict,
};
use governance_evaluator::evaluate_pack;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod evaluation_runs;
mod policy_import;

pub use evaluation_runs::*;
pub use policy_import::*;

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
    #[error("resource state conflicts with this operation: {0}")]
    Conflict(String),
    #[error("required service is unavailable: {0}")]
    Unavailable(String),
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
    async fn disable(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        change: &PolicyPackStatusChange,
    ) -> Result<PolicyPack, ApplicationError>;
    async fn enable(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        change: &PolicyPackStatusChange,
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
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
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
