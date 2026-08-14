use std::{fmt, time::Duration};

use governance_application::{
    CreatePolicyImport, NewPolicyImport, PolicyCollectionRepository, PolicyImportAcquisition,
    PreparedPolicyArtifactInput, ProcessPolicyImport, SourceConnectionRepository,
    SourceIngestionRepository, SubscriptionRevisionDecision, sha256_hex,
};
use governance_config::{PolicyImportConfig, SourceConnectorConfig};
use governance_connectors::{
    ConnectorRetry, CredentialCipher, CredentialContext, EncryptedCredential,
    GoogleDriveSourceClient, MicrosoftGraphSourceClient, NotionSourceClient,
    OAuthClientCredentials, PolicySourceConnector, SafeFetchConfig, SafeUrlFetcher,
    refresh_provider_token,
};
use governance_domain::{
    OrganizationId, PolicyImportStatus, PolicyInputKind, SourceConnectionStatus,
    SourceIngestionItemId, SourceIngestionItemStatus, SourceProvider, SourceType,
};
use governance_ingestion::{
    ConfiguredPolicyExtractionModel, OpenDalArtifactStore, SafePolicyDocumentParser,
};
use governance_persistence::{
    SeaOrmPolicyCollectionRepository, SeaOrmPolicyImportRepository,
    SeaOrmSourceConnectionRepository, SeaOrmSourceIngestionRepository,
};
use loco_rs::prelude::*;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Deserialize, Serialize)]
struct StoredTokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    expires_at_unix: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AcquirePolicySourceArgs {
    pub organization_id: OrganizationId,
    pub source_ingestion_item_id: SourceIngestionItemId,
}

#[derive(Clone)]
pub struct AcquirePolicySourceWorker {
    context: AppContext,
}

impl fmt::Debug for AcquirePolicySourceWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcquirePolicySourceWorker")
            .finish_non_exhaustive()
    }
}

#[allow(clippy::single_match_else, clippy::too_many_lines)]
#[async_trait]
impl BackgroundWorker<AcquirePolicySourceArgs> for AcquirePolicySourceWorker {
    fn build(context: &AppContext) -> Self {
        Self {
            context: context.clone(),
        }
    }

    async fn perform(&self, args: AcquirePolicySourceArgs) -> Result<()> {
        let import_config = PolicyImportConfig::from_env()
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let connector_config = SourceConnectorConfig::from_env()
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let ingestion = SeaOrmSourceIngestionRepository::new(self.context.db.clone());
        let item = ingestion
            .claim_item(args.organization_id, args.source_ingestion_item_id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let external_item_id = item.external_item_id.clone().ok_or_else(|| {
            loco_rs::Error::Worker("ingestion item is missing its external identifier".to_owned())
        })?;
        let (batch, _) = ingestion
            .get_batch(args.organization_id, item.batch_id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?
            .ok_or_else(|| loco_rs::Error::Worker("ingestion batch disappeared".to_owned()))?;
        let connections = SeaOrmSourceConnectionRepository::new(self.context.db.clone());
        let acquisition_setup: std::result::Result<_, String> = async {
            let result = match item.connection_id {
                Some(connection_id) => {
                    let stored = connections
                        .encrypted_credential(
                            args.organization_id,
                            connection_id,
                            &batch.requested_by,
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    let active_version = connector_config
                        .active_key_version
                        .ok_or_else(|| "connector encryption is unavailable".to_owned())?;
                    let nonce: [u8; 12] = stored
                        .nonce
                        .try_into()
                        .map_err(|_| "stored credential is invalid".to_owned())?;
                    let cipher = CredentialCipher::from_base64_keys(
                        &connector_config.encryption_keys,
                        active_version,
                    )
                    .map_err(|error| error.to_string())?;
                    let decrypted = cipher
                        .decrypt(
                            &CredentialContext {
                                organization_id: args.organization_id.to_string(),
                                connection_id: connection_id.to_string(),
                                provider: provider_name(stored.provider).to_owned(),
                            },
                            &EncryptedCredential {
                                ciphertext: stored.ciphertext,
                                nonce,
                                key_version: stored.key_version,
                            },
                        )
                        .map_err(|error| error.to_string())?;
                    let mut tokens: StoredTokens =
                        serde_json::from_str(decrypted.plaintext.expose_secret())
                            .map_err(|_| "stored credential is invalid".to_owned())?;
                    if tokens.expires_at_unix.is_some_and(|expires| {
                        expires <= OffsetDateTime::now_utc().unix_timestamp() + 60
                    }) {
                        let refresh_token = tokens.refresh_token.as_deref().ok_or_else(|| {
                            "source connection requires reauthorization".to_owned()
                        })?;
                        let provider = provider_config(&connector_config, stored.provider)
                            .ok_or_else(|| "source provider is not configured".to_owned())?;
                        let refreshed = refresh_provider_token(
                            stored.provider,
                            &OAuthClientCredentials {
                                client_id: provider.client_id.clone(),
                                client_secret: provider.client_secret.clone(),
                            },
                            refresh_token,
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                        tokens.access_token = refreshed.access_token;
                        if refreshed.refresh_token.is_some() {
                            tokens.refresh_token = refreshed.refresh_token;
                        }
                        if refreshed.token_type.is_some() {
                            tokens.token_type = refreshed.token_type;
                        }
                        if refreshed.scope.is_some() {
                            tokens.scope = refreshed.scope;
                        }
                        tokens.expires_at_unix = refreshed
                            .expires_in
                            .map(|seconds| OffsetDateTime::now_utc().unix_timestamp() + seconds);
                        let encrypted =
                            cipher
                                .encrypt(
                                    &CredentialContext {
                                        organization_id: args.organization_id.to_string(),
                                        connection_id: connection_id.to_string(),
                                        provider: provider_name(stored.provider).to_owned(),
                                    },
                                    &SecretString::from(serde_json::to_string(&tokens).map_err(
                                        |_| "refreshed credential is invalid".to_owned(),
                                    )?),
                                )
                                .map_err(|error| error.to_string())?;
                        connections
                            .update_encrypted_credential(
                                args.organization_id,
                                connection_id,
                                &batch.requested_by,
                                governance_persistence::StoredEncryptedCredential {
                                    provider: stored.provider,
                                    ciphertext: encrypted.ciphertext,
                                    nonce: encrypted.nonce.to_vec(),
                                    key_version: encrypted.key_version,
                                },
                                tokens.expires_at_unix.and_then(|value| {
                                    OffsetDateTime::from_unix_timestamp(value).ok()
                                }),
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    let token = SecretString::from(tokens.access_token);
                    let connector: Box<dyn PolicySourceConnector> = match stored.provider {
                        SourceProvider::GoogleDrive => Box::new(
                            GoogleDriveSourceClient::new(token, import_config.max_bytes)
                                .map_err(|error| error.to_string())?,
                        ),
                        SourceProvider::MicrosoftGraph => Box::new(
                            MicrosoftGraphSourceClient::new(token, import_config.max_bytes)
                                .map_err(|error| error.to_string())?,
                        ),
                        SourceProvider::Notion => Box::new(
                            NotionSourceClient::new(token, import_config.max_bytes)
                                .map_err(|error| error.to_string())?,
                        ),
                    };
                    let input_kind = match stored.provider {
                        SourceProvider::GoogleDrive => PolicyInputKind::GoogleDrive,
                        SourceProvider::MicrosoftGraph => PolicyInputKind::MicrosoftGraph,
                        SourceProvider::Notion => PolicyInputKind::Notion,
                    };
                    (connector.acquire(&external_item_id).await, input_kind)
                }
                None => {
                    let fetcher = SafeUrlFetcher::new(SafeFetchConfig {
                        max_bytes: import_config.max_bytes,
                        max_redirects: connector_config.max_redirects,
                        connect_timeout: Duration::from_secs(
                            u64::try_from(connector_config.connect_timeout_seconds).unwrap_or(5),
                        ),
                        response_timeout: Duration::from_secs(
                            u64::try_from(connector_config.response_timeout_seconds).unwrap_or(30),
                        ),
                    })
                    .map_err(|error| error.to_string())?;
                    (
                        fetcher.acquire(&external_item_id).await,
                        PolicyInputKind::Url,
                    )
                }
            };
            Ok(result)
        }
        .await;
        let (artifact, input_kind) = match acquisition_setup {
            Ok(value) => value,
            Err(detail) => {
                let _ = ingestion
                    .update_item(
                        args.organization_id,
                        item.id,
                        SourceIngestionItemStatus::Failed,
                        None,
                        Some((
                            "acquisition_setup_failed",
                            "source acquisition could not start",
                        )),
                    )
                    .await;
                let _ = ingestion
                    .recompute_batch(args.organization_id, item.batch_id)
                    .await;
                return Err(loco_rs::Error::Worker(detail));
            }
        };
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(error) => {
                if error.retry == ConnectorRetry::Reauthorize
                    && let Some(connection_id) = item.connection_id
                {
                    let _ = connections
                        .mark_connection_failure(
                            args.organization_id,
                            connection_id,
                            &batch.requested_by,
                            SourceConnectionStatus::ReauthorizationRequired,
                            error.code,
                        )
                        .await;
                }
                let _ = ingestion
                    .update_item(
                        args.organization_id,
                        item.id,
                        SourceIngestionItemStatus::Failed,
                        None,
                        Some((error.code, "remote source acquisition failed")),
                    )
                    .await;
                let _ = ingestion
                    .recompute_batch(args.organization_id, item.batch_id)
                    .await;
                return Err(loco_rs::Error::Worker(error.to_string()));
            }
        };
        let mut supersedes_import_id = None;
        if let Some(subscription_id) = item.subscription_id {
            let subscriptions = connections
                .list_subscriptions(
                    args.organization_id,
                    item.connection_id.ok_or_else(|| {
                        loco_rs::Error::Worker("subscription is missing its connection".to_owned())
                    })?,
                )
                .await
                .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
            let subscription = subscriptions
                .into_iter()
                .find(|value| value.id == subscription_id)
                .ok_or_else(|| {
                    loco_rs::Error::Worker("source subscription disappeared".to_owned())
                })?;
            match connections
                .reserve_revision(
                    args.organization_id,
                    subscription_id,
                    &artifact.external_revision,
                    &sha256_hex(&artifact.raw_content),
                )
                .await
                .map_err(|error| loco_rs::Error::Worker(error.to_string()))?
            {
                SubscriptionRevisionDecision::Unchanged => {
                    ingestion
                        .update_item(
                            args.organization_id,
                            item.id,
                            SourceIngestionItemStatus::Unchanged,
                            subscription.last_import_id,
                            None,
                        )
                        .await
                        .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
                    ingestion
                        .recompute_batch(args.organization_id, item.batch_id)
                        .await
                        .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
                    return Ok(());
                }
                SubscriptionRevisionDecision::BlockedPendingReview => {
                    ingestion
                        .update_item(
                            args.organization_id,
                            item.id,
                            SourceIngestionItemStatus::Blocked,
                            subscription.last_import_id,
                            Some((
                                "update_waiting_for_review",
                                "the previous revision must finish review first",
                            )),
                        )
                        .await
                        .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
                    ingestion
                        .recompute_batch(args.organization_id, item.batch_id)
                        .await
                        .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
                    return Ok(());
                }
                SubscriptionRevisionDecision::CreateRevision => {
                    supersedes_import_id = subscription.last_import_id;
                }
                SubscriptionRevisionDecision::CreateInitial => {}
            }
        }
        let external_revision = artifact.external_revision.clone();
        let external_modified_at = artifact.external_modified_at;
        let artifacts = OpenDalArtifactStore::from_config(&import_config)
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let repository = SeaOrmPolicyImportRepository::new(self.context.db.clone());
        let prepared = artifact
            .prepared
            .map(|prepared| PreparedPolicyArtifactInput {
                kind: prepared.kind,
                processor: prepared.processor,
                processor_version: prepared.processor_version,
                mime_type: prepared.mime_type,
                content: prepared.content,
                metadata: prepared.metadata,
                created_by: item.client_item_key.clone(),
            });
        let declared_mime_type = artifact.declared_mime_type.clone();
        let processing_mime = prepared
            .as_ref()
            .map_or_else(
                || declared_mime_type.clone(),
                |value| Some(value.mime_type.clone()),
            )
            .unwrap_or_else(|| "text/plain".to_owned());
        let import = CreatePolicyImport::new(repository.clone(), artifacts.clone())
            .execute_prepared(
                NewPolicyImport {
                    organization_id: args.organization_id,
                    input_kind,
                    source_type: SourceType::CompanyPolicy,
                    title: artifact.title,
                    jurisdiction: "internal".to_owned(),
                    effective_from: None,
                    source_url: artifact.canonical_url,
                    original_filename: artifact.original_filename,
                    declared_mime_type,
                    detected_mime_type: processing_mime,
                    content: artifact.raw_content,
                    idempotency_key: Some(format!("source-item:{}", item.id)),
                    supersedes_import_id,
                },
                prepared,
                PolicyImportAcquisition {
                    ingestion_item_id: Some(item.id),
                    source_subscription_id: item.subscription_id,
                    external_revision: Some(external_revision.clone()),
                    external_modified_at,
                },
            )
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        if let Some(subscription_id) = item.subscription_id {
            connections
                .record_subscription_import(
                    args.organization_id,
                    subscription_id,
                    import.id,
                    import.policy_source_id,
                    external_revision,
                    external_modified_at,
                    import.source_url.clone(),
                    import.title.clone(),
                    import.declared_mime_type.clone(),
                )
                .await
                .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        }
        if let Some(collection_id) = batch.policy_collection_id {
            SeaOrmPolicyCollectionRepository::new(self.context.db.clone())
                .add_import(args.organization_id, collection_id, &import)
                .await
                .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        }
        ingestion
            .update_item(
                args.organization_id,
                item.id,
                SourceIngestionItemStatus::Queued,
                Some(import.id),
                None,
            )
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let parser = SafePolicyDocumentParser::from_config(&import_config);
        let model = ConfiguredPolicyExtractionModel::from_config(&import_config)
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        ingestion
            .update_item(
                args.organization_id,
                item.id,
                SourceIngestionItemStatus::Processing,
                Some(import.id),
                None,
            )
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        let processed = ProcessPolicyImport::new(repository, artifacts, parser, model)
            .execute(args.organization_id, import.id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        ingestion
            .update_item(
                args.organization_id,
                item.id,
                if processed.status == PolicyImportStatus::NeedsOcr {
                    SourceIngestionItemStatus::Blocked
                } else {
                    SourceIngestionItemStatus::ReviewRequired
                },
                Some(import.id),
                None,
            )
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        ingestion
            .recompute_batch(args.organization_id, item.batch_id)
            .await
            .map_err(|error| loco_rs::Error::Worker(error.to_string()))?;
        Ok(())
    }
}

fn provider_name(provider: SourceProvider) -> &'static str {
    match provider {
        SourceProvider::GoogleDrive => "google_drive",
        SourceProvider::MicrosoftGraph => "microsoft_graph",
        SourceProvider::Notion => "notion",
    }
}

fn provider_config(
    config: &SourceConnectorConfig,
    provider: SourceProvider,
) -> Option<&governance_config::ProviderOAuthConfig> {
    match provider {
        SourceProvider::GoogleDrive => config.google.as_ref(),
        SourceProvider::MicrosoftGraph => config.microsoft.as_ref(),
        SourceProvider::Notion => config.notion.as_ref(),
    }
}
