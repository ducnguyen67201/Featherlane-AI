use governance_application::{
    PolicyImportRepository, ProcessPolicyImport, SourceIngestionRepository,
};
use governance_config::PolicyImportConfig;
use governance_domain::{
    OrganizationId, PolicyImportId, PolicyImportStatus, SourceIngestionItemStatus,
};
use governance_ingestion::{
    ConfiguredPolicyExtractionModel, OpenDalArtifactStore, SafePolicyDocumentParser,
};
use governance_persistence::{SeaOrmPolicyImportRepository, SeaOrmSourceIngestionRepository};
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
    ingestion: SeaOrmSourceIngestionRepository,
    setup: PolicyImportWorkerSetup,
}

#[derive(Clone, Debug)]
enum PolicyImportWorkerSetup {
    Ready {
        artifacts: OpenDalArtifactStore,
        parser: SafePolicyDocumentParser,
        model: Box<ConfiguredPolicyExtractionModel>,
    },
    Failed {
        code: &'static str,
        detail: &'static str,
        error: String,
    },
}

#[allow(clippy::too_many_lines)]
#[async_trait]
impl BackgroundWorker<ProcessPolicyImportArgs> for ProcessPolicyImportWorker {
    fn build(context: &AppContext) -> Self {
        let setup = PolicyImportConfig::from_env().map_or_else(
            |error| PolicyImportWorkerSetup::Failed {
                code: "configuration_missing",
                detail: "policy import configuration is invalid",
                error: error.to_string(),
            },
            |config| {
                let artifacts = OpenDalArtifactStore::from_config(&config).map_err(|error| {
                    PolicyImportWorkerSetup::Failed {
                        code: "artifact_store_misconfigured",
                        detail: "policy source storage is unavailable",
                        error: error.to_string(),
                    }
                });
                let model =
                    ConfiguredPolicyExtractionModel::from_config(&config).map_err(|error| {
                        PolicyImportWorkerSetup::Failed {
                            code: "extraction_provider_misconfigured",
                            detail: "policy extraction provider is unavailable",
                            error: error.to_string(),
                        }
                    });
                match (artifacts, model) {
                    (Ok(artifacts), Ok(model)) => PolicyImportWorkerSetup::Ready {
                        artifacts,
                        parser: SafePolicyDocumentParser::from_config(&config),
                        model: Box::new(model),
                    },
                    (Err(error), _) | (_, Err(error)) => error,
                }
            },
        );
        Self {
            repository: SeaOrmPolicyImportRepository::new(context.db.clone()),
            ingestion: SeaOrmSourceIngestionRepository::new(context.db.clone()),
            setup,
        }
    }

    async fn perform(&self, args: ProcessPolicyImportArgs) -> Result<()> {
        tracing::info!(
            policy_import_id = %args.policy_import_id,
            organization_id = %args.organization_id,
            "policy source extraction started"
        );
        let (artifacts, parser, model) = match &self.setup {
            PolicyImportWorkerSetup::Ready {
                artifacts,
                parser,
                model,
            } => (artifacts.clone(), parser.clone(), model.as_ref().clone()),
            PolicyImportWorkerSetup::Failed {
                code,
                detail,
                error,
            } => {
                if let Err(persistence_error) = self
                    .repository
                    .mark_failure(
                        args.organization_id,
                        args.policy_import_id,
                        PolicyImportStatus::FailedRetryable,
                        code,
                        detail,
                    )
                    .await
                {
                    tracing::warn!(policy_import_id = %args.policy_import_id, error = %persistence_error, "failed to persist policy import configuration failure");
                }
                if let Ok(Some(import)) = self
                    .repository
                    .get(args.organization_id, args.policy_import_id)
                    .await
                    && let Some(item_id) = import.ingestion_item_id
                    && let Ok(item) = self
                        .ingestion
                        .update_item(
                            args.organization_id,
                            item_id,
                            SourceIngestionItemStatus::Failed,
                            Some(args.policy_import_id),
                            Some((code, detail)),
                        )
                        .await
                {
                    let _ = self
                        .ingestion
                        .recompute_batch(args.organization_id, item.batch_id)
                        .await;
                }
                return Err(loco_rs::Error::Worker(error.clone()));
            }
        };
        let queued_import = self
            .repository
            .get(args.organization_id, args.policy_import_id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        if let Some(item_id) = queued_import
            .as_ref()
            .and_then(|import| import.ingestion_item_id)
        {
            let _ = self
                .ingestion
                .update_item(
                    args.organization_id,
                    item_id,
                    SourceIngestionItemStatus::Processing,
                    Some(args.policy_import_id),
                    None,
                )
                .await;
        }
        let result = ProcessPolicyImport::new(self.repository.clone(), artifacts, parser, model)
            .execute(args.organization_id, args.policy_import_id)
            .await;
        let import = match result {
            Ok(import) => import,
            Err(error) => {
                if let Some(item_id) = queued_import.and_then(|import| import.ingestion_item_id)
                    && let Ok(item) = self
                        .ingestion
                        .update_item(
                            args.organization_id,
                            item_id,
                            SourceIngestionItemStatus::Failed,
                            Some(args.policy_import_id),
                            Some(("processing_failed", "policy source processing failed")),
                        )
                        .await
                {
                    let _ = self
                        .ingestion
                        .recompute_batch(args.organization_id, item.batch_id)
                        .await;
                }
                return Err(loco_rs::Error::Worker(error.to_string()));
            }
        };
        if let Some(item_id) = import.ingestion_item_id {
            let item_status = if import.status == PolicyImportStatus::NeedsOcr {
                SourceIngestionItemStatus::Blocked
            } else {
                SourceIngestionItemStatus::ReviewRequired
            };
            if let Ok(item) = self
                .ingestion
                .update_item(
                    import.organization_id,
                    item_id,
                    item_status,
                    Some(import.id),
                    None,
                )
                .await
            {
                let _ = self
                    .ingestion
                    .recompute_batch(import.organization_id, item.batch_id)
                    .await;
            }
        }
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
