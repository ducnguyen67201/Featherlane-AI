//! Loco background-worker contract for deterministic evaluation jobs.

use governance_application::EvaluateEvidence;
use governance_domain::{EvidenceBundle, OrganizationId, PolicyPackId};
use governance_persistence::{SeaOrmEvaluationRepository, SeaOrmPolicyPackRepository};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvaluationWorkerArgs {
    pub organization_id: OrganizationId,
    pub policy_pack_id: PolicyPackId,
    pub evidence: EvidenceBundle,
}

#[derive(Clone, Debug)]
pub struct EvaluationWorker {
    policy_packs: SeaOrmPolicyPackRepository,
    evaluations: SeaOrmEvaluationRepository,
}

#[async_trait]
impl BackgroundWorker<EvaluationWorkerArgs> for EvaluationWorker {
    fn build(context: &AppContext) -> Self {
        Self {
            policy_packs: SeaOrmPolicyPackRepository::new(context.db.clone()),
            evaluations: SeaOrmEvaluationRepository::new(context.db.clone()),
        }
    }

    async fn perform(&self, args: EvaluationWorkerArgs) -> Result<()> {
        let summary = EvaluateEvidence::new(self.policy_packs.clone(), self.evaluations.clone())
            .execute(args.organization_id, args.policy_pack_id, &args.evidence)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        tracing::info!(
            eval_run_id = %summary.eval_run_id,
            verdict = ?summary.verdict,
            "governance evaluation completed"
        );
        Ok(())
    }
}
