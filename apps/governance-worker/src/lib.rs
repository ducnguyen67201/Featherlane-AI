//! Loco background-worker contract for deterministic evaluation jobs.

use governance_application::{EvaluateFinalizedRun, FinalizeEvaluationRun};
use governance_domain::{EvalRunId, OrganizationId};
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository,
};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

mod policy_import;

pub use policy_import::{ProcessPolicyImportArgs, ProcessPolicyImportWorker};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvaluationWorkerArgs {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
}

#[derive(Clone, Debug)]
pub struct EvaluationWorker {
    policy_packs: SeaOrmPolicyPackRepository,
    runs: SeaOrmEvaluationRunRepository,
    evaluations: SeaOrmEvaluationRepository,
}

#[async_trait]
impl BackgroundWorker<EvaluationWorkerArgs> for EvaluationWorker {
    fn build(context: &AppContext) -> Self {
        Self {
            policy_packs: SeaOrmPolicyPackRepository::new(context.db.clone()),
            runs: SeaOrmEvaluationRunRepository::new(context.db.clone()),
            evaluations: SeaOrmEvaluationRepository::new(context.db.clone()),
        }
    }

    async fn perform(&self, args: EvaluationWorkerArgs) -> Result<()> {
        let summary = EvaluateFinalizedRun::new(
            self.policy_packs.clone(),
            self.runs.clone(),
            self.runs.clone(),
            self.evaluations.clone(),
        )
        .execute(args.organization_id, args.eval_run_id)
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FinalizationWorkerArgs {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
}

#[derive(Clone, Debug)]
pub struct FinalizationWorker {
    runs: SeaOrmEvaluationRunRepository,
}

#[async_trait]
impl BackgroundWorker<FinalizationWorkerArgs> for FinalizationWorker {
    fn build(context: &AppContext) -> Self {
        Self {
            runs: SeaOrmEvaluationRunRepository::new(context.db.clone()),
        }
    }

    async fn perform(&self, args: FinalizationWorkerArgs) -> Result<()> {
        let bundle = FinalizeEvaluationRun::new(
            self.runs.clone(),
            self.runs.clone(),
            self.runs.clone(),
            self.runs.clone(),
        )
        .execute(args.organization_id, args.eval_run_id)
        .await
        .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        tracing::info!(
            eval_run_id = %bundle.eval_run_id,
            evidence_sha256 = %bundle.evidence_sha256,
            "governance evidence finalized"
        );
        Ok(())
    }
}
