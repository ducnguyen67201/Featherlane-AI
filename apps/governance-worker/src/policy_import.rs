use governance_application::{PolicyImportRepository, ProcessPolicyImport};
use governance_config::PolicyImportConfig;
use governance_domain::{OrganizationId, PolicyImportId, PolicyImportStatus};
use governance_ingestion::{
    ConfiguredPolicyExtractionModel, OpenDalArtifactStore, SafePolicyDocumentParser,
};
use governance_persistence::SeaOrmPolicyImportRepository;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessPolicyImportArgs {
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
}

#[derive(Clone, Debug)]
pub struct ProcessPolicyImportWorker {
    repository: SeaOrmPolicyImportRepository,
    config: PolicyImportConfig,
}

#[async_trait]
impl BackgroundWorker<ProcessPolicyImportArgs> for ProcessPolicyImportWorker {
    fn build(context: &AppContext) -> Self {
        let config = PolicyImportConfig::from_env()
            .unwrap_or_else(|error| panic!("invalid policy import configuration: {error}"));
        Self {
            repository: SeaOrmPolicyImportRepository::new(context.db.clone()),
            config,
        }
    }

    async fn perform(&self, args: ProcessPolicyImportArgs) -> Result<()> {
        tracing::info!(
            policy_import_id = %args.policy_import_id,
            organization_id = %args.organization_id,
            "policy source extraction started"
        );
        let artifacts = match OpenDalArtifactStore::from_config(&self.config) {
            Ok(artifacts) => artifacts,
            Err(error) => {
                let _ = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        "artifact_store_misconfigured",
                        "policy source storage is unavailable",
                    )
                    .await;
                return Err(loco_rs::Error::Worker(error.to_string()));
            }
        };
        let parser = SafePolicyDocumentParser::from_config(&self.config);
        let model = match ConfiguredPolicyExtractionModel::from_config(&self.config) {
            Ok(model) => model,
            Err(error) => {
                let _ = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        "extraction_provider_misconfigured",
                        "policy extraction provider is unavailable",
                    )
                    .await;
                return Err(loco_rs::Error::Worker(error.to_string()));
            }
        };
        let import = ProcessPolicyImport::new(self.repository.clone(), artifacts, parser, model)
            .execute(args.organization_id, args.policy_import_id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        tracing::info!(
            policy_import_id = %import.id,
            organization_id = %import.organization_id,
            status = ?import.status,
            candidate_count = import.candidate_count,
            "policy source extraction completed"
        );
        Ok(())
    }
}
