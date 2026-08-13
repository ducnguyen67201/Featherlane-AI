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
    config: Result<PolicyImportConfig, String>,
}

#[async_trait]
impl BackgroundWorker<ProcessPolicyImportArgs> for ProcessPolicyImportWorker {
    fn build(context: &AppContext) -> Self {
        let config = PolicyImportConfig::from_env().map_err(|error| error.to_string());
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
        let config = match &self.config {
            Ok(config) => config,
            Err(error) => {
                if let Err(persistence_error) = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        "configuration_missing",
                        "policy import configuration is invalid",
                    )
                    .await
                {
                    tracing::warn!(policy_import_id = %args.policy_import_id, error = %persistence_error, "failed to persist policy import configuration failure");
                }
                return Err(loco_rs::Error::Worker(error.clone()));
            }
        };
        let artifacts = match OpenDalArtifactStore::from_config(config) {
            Ok(artifacts) => artifacts,
            Err(error) => {
                if let Err(persistence_error) = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        "artifact_store_misconfigured",
                        "policy source storage is unavailable",
                    )
                    .await
                {
                    tracing::warn!(policy_import_id = %args.policy_import_id, error = %persistence_error, "failed to persist artifact-store configuration failure");
                }
                return Err(loco_rs::Error::Worker(error.to_string()));
            }
        };
        let parser = SafePolicyDocumentParser::from_config(config);
        let model = match ConfiguredPolicyExtractionModel::from_config(config) {
            Ok(model) => model,
            Err(error) => {
                if let Err(persistence_error) = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        "extraction_provider_misconfigured",
                        "policy extraction provider is unavailable",
                    )
                    .await
                {
                    tracing::warn!(policy_import_id = %args.policy_import_id, error = %persistence_error, "failed to persist extraction-provider configuration failure");
                }
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
