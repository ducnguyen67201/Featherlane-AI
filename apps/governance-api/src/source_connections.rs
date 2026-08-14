#![allow(clippy::result_large_err)]

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use governance_application::SourceConnectionRepository;
use governance_application::SourceIngestionRepository;
use governance_config::{ProviderOAuthConfig, SourceConnectorConfig};
use governance_connectors::{
    CredentialCipher, CredentialContext, EncryptedCredential, GoogleDriveClient,
    MicrosoftGraphClient, NotionClient, hex_digest, new_oauth_proof,
};
use governance_domain::{
    PolicyCollectionId, SourceConnectionId, SourceIngestionBatch, SourceIngestionBatchId,
    SourceIngestionBatchKind, SourceIngestionBatchStatus, SourceIngestionItem,
    SourceIngestionItemId, SourceIngestionItemStatus, SourceProvider, SourceSubscriptionStatus,
};
use governance_persistence::{
    NewOAuthState, NewStoredConnection, SeaOrmSourceConnectionRepository,
    SeaOrmSourceIngestionRepository,
};
use governance_worker::{AcquirePolicySourceArgs, AcquirePolicySourceWorker};
use loco_rs::app::AppContext;
use loco_rs::bgworker::BackgroundWorker;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{console_auth::authenticate, loco_app::problem, policy_imports::application_error};

#[derive(Debug, Deserialize)]
pub struct AuthorizeRequest {
    collection_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SyncRequest {
    collection_id: Option<String>,
    #[serde(default)]
    subscription_ids: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BrowseQuery {
    drive_id: Option<String>,
    parent_id: Option<String>,
    cursor: Option<String>,
    query: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteBrowseItem {
    id: String,
    name: String,
    kind: &'static str,
    mime_type: Option<String>,
    size: Option<u64>,
    modified_at: Option<String>,
    canonical_url: Option<String>,
    drive_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteBrowseResponse {
    items: Vec<RemoteBrowseItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    expires_at_unix: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    expires_in: Option<i64>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    bot_id: Option<String>,
}

fn services(
    context: &AppContext,
    headers: &HeaderMap,
) -> Result<(SourceConnectorConfig, crate::console_auth::ConsoleActor), Response> {
    let config = context
        .shared_store
        .get::<SourceConnectorConfig>()
        .ok_or_else(|| {
            problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "connector services are unavailable",
            )
        })?;
    let actor = authenticate(headers, &config)?;
    Ok((config, actor))
}

pub async fn list(State(context): State<AppContext>, headers: HeaderMap) -> Response {
    let (_, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match SeaOrmSourceConnectionRepository::new(context.db)
        .list_connections(crate::default_organization_id(), &actor.id)
        .await
    {
        Ok(connections) => Json(connections).into_response(),
        Err(error) => application_error(error),
    }
}

#[allow(clippy::too_many_lines)]
pub async fn authorize(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(request): Json<AuthorizeRequest>,
) -> Response {
    let (config, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(provider) = parse_provider(&provider) else {
        return problem(StatusCode::BAD_REQUEST, "unsupported source provider");
    };
    let Some(provider_config) = provider_config(&config, provider) else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "source provider is not configured",
        );
    };
    let Some(active_version) = config.active_key_version else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "connector encryption is not configured",
        );
    };
    let Ok(cipher) = CredentialCipher::from_base64_keys(&config.encryption_keys, active_version)
    else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "connector encryption is not configured",
        );
    };
    let proof = new_oauth_proof();
    let organization_id = crate::default_organization_id();
    let context_aad = CredentialContext {
        organization_id: organization_id.to_string(),
        connection_id: proof.state_hash.clone(),
        provider: provider_name(provider).to_owned(),
    };
    let encrypted = if provider == SourceProvider::Notion {
        None
    } else {
        match cipher.encrypt(
            &context_aad,
            &SecretString::from(proof.pkce_verifier.clone()),
        ) {
            Ok(value) => Some(value),
            Err(_) => {
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "OAuth state encryption failed",
                );
            }
        }
    };
    let collection_id = match request.collection_id {
        Some(value) => match Uuid::parse_str(&value) {
            Ok(value) => Some(PolicyCollectionId(value)),
            Err(_) => return problem(StatusCode::BAD_REQUEST, "invalid originating collection"),
        },
        None => None,
    };
    if let Err(error) = SeaOrmSourceConnectionRepository::new(context.db)
        .store_oauth_state(NewOAuthState {
            state_hash: proof.state_hash.clone(),
            organization_id,
            provider,
            actor_id: actor.id,
            originating_collection_id: collection_id,
            pkce_ciphertext: encrypted.as_ref().map(|value| value.ciphertext.clone()),
            pkce_nonce: encrypted.as_ref().map(|value| value.nonce.to_vec()),
            key_version: encrypted.as_ref().map(|value| value.key_version),
            redirect_uri: provider_config.callback_url.to_string(),
            expires_at: OffsetDateTime::now_utc()
                + time::Duration::seconds(
                    i64::try_from(config.oauth_state_ttl_seconds).unwrap_or(600),
                ),
        })
        .await
    {
        return application_error(error);
    }
    let url = match provider {
        SourceProvider::GoogleDrive => GoogleDriveClient::new(
            provider_config.client_id.clone(),
            provider_config.client_secret.clone(),
            provider_config.callback_url.clone(),
        )
        .authorization_url(&proof),
        SourceProvider::MicrosoftGraph => MicrosoftGraphClient::new(
            provider_config.client_id.clone(),
            provider_config.client_secret.clone(),
            provider_config.callback_url.clone(),
        )
        .authorization_url(&proof),
        SourceProvider::Notion => NotionClient::new(
            provider_config.client_id.clone(),
            provider_config.client_secret.clone(),
            provider_config.callback_url.clone(),
        )
        .authorization_url(&proof.state),
    };
    match url {
        Ok(url) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "authorization_url": url })),
        )
            .into_response(),
        Err(error) => problem(StatusCode::BAD_GATEWAY, &error.to_string()),
    }
}

#[allow(clippy::too_many_lines)]
pub async fn callback(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let (config, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if query.error.is_some() {
        return problem(StatusCode::BAD_REQUEST, "source authorization was denied");
    }
    let (Some(code), Some(state), Some(provider)) =
        (query.code, query.state, parse_provider(&provider))
    else {
        return problem(StatusCode::BAD_REQUEST, "OAuth callback is incomplete");
    };
    let Some(provider_config) = provider_config(&config, provider) else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "source provider is not configured",
        );
    };
    let repository = SeaOrmSourceConnectionRepository::new(context.db);
    let consumed = match repository
        .consume_oauth_state(
            crate::default_organization_id(),
            &hex_digest(state.as_bytes()),
            provider,
            &actor.id,
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    if consumed.redirect_uri != provider_config.callback_url.as_str() {
        return problem(StatusCode::CONFLICT, "OAuth redirect binding changed");
    }
    let pkce_verifier =
        if let (Some(ciphertext), Some(nonce), Some(key_version), Some(active_version)) = (
            consumed.pkce_ciphertext,
            consumed.pkce_nonce,
            consumed.key_version,
            config.active_key_version,
        ) {
            let Ok(nonce): Result<[u8; 12], _> = nonce.try_into() else {
                return problem(StatusCode::CONFLICT, "OAuth state is invalid");
            };
            let Ok(cipher) =
                CredentialCipher::from_base64_keys(&config.encryption_keys, active_version)
            else {
                return problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "connector encryption is unavailable",
                );
            };
            match cipher.decrypt(
                &CredentialContext {
                    organization_id: crate::default_organization_id().to_string(),
                    connection_id: hex_digest(state.as_bytes()),
                    provider: provider_name(provider).to_owned(),
                },
                &EncryptedCredential {
                    ciphertext,
                    nonce,
                    key_version,
                },
            ) {
                Ok(value) => Some(value.plaintext.expose_secret().to_owned()),
                Err(_) => return problem(StatusCode::CONFLICT, "OAuth state is invalid"),
            }
        } else {
            None
        };
    let token =
        match exchange_code(provider, provider_config, &code, pkce_verifier.as_deref()).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let identity = match provider_identity(provider, &token).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let expires_at = token
        .expires_in
        .map(|seconds| OffsetDateTime::now_utc() + time::Duration::seconds(seconds));
    let stored = StoredTokens {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        token_type: token.token_type,
        scope: token.scope.clone(),
        expires_at_unix: expires_at.map(OffsetDateTime::unix_timestamp),
    };
    let Some(active_version) = config.active_key_version else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "connector encryption is unavailable",
        );
    };
    let Ok(cipher) = CredentialCipher::from_base64_keys(&config.encryption_keys, active_version)
    else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "connector encryption is unavailable",
        );
    };
    let connection_seed = token
        .workspace_id
        .clone()
        .or(token.bot_id.clone())
        .unwrap_or(identity.0);
    let existing = match repository
        .list_connections(crate::default_organization_id(), &actor.id)
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    let connection_id = existing
        .iter()
        .find(|connection| {
            connection.provider == provider && connection.provider_account_id == connection_seed
        })
        .map_or_else(SourceConnectionId::new, |connection| connection.id);
    let Ok(encrypted) = cipher.encrypt(
        &CredentialContext {
            organization_id: crate::default_organization_id().to_string(),
            connection_id: connection_id.to_string(),
            provider: provider_name(provider).to_owned(),
        },
        &SecretString::from(serde_json::to_string(&stored).unwrap_or_default()),
    ) else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "credential encryption failed",
        );
    };
    let connection = match repository
        .save_connection(NewStoredConnection {
            id: connection_id,
            organization_id: crate::default_organization_id(),
            provider,
            connected_by: actor.id,
            provider_account_id: connection_seed,
            display_label: token.workspace_name.unwrap_or(identity.1),
            granted_scopes: token
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect(),
            credential_ciphertext: encrypted.ciphertext,
            credential_nonce: encrypted.nonce.to_vec(),
            credential_key_version: encrypted.key_version,
            access_expires_at: expires_at,
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    let redirect = consumed.originating_collection_id.map_or_else(
        || "/policies".to_owned(),
        |id| format!("/policies/collections/{id}"),
    );
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "connection": connection, "redirect": redirect })),
    )
        .into_response()
}

pub async fn picker_token(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (config, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(id) = Uuid::parse_str(&id).map(SourceConnectionId) else {
        return problem(StatusCode::BAD_REQUEST, "invalid connection identifier");
    };
    let (provider, tokens) = match decrypted_tokens(&context, &config, id, &actor.id).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if provider != SourceProvider::GoogleDrive {
        return problem(
            StatusCode::BAD_REQUEST,
            "picker token is only available for Google Drive",
        );
    }
    if tokens
        .expires_at_unix
        .is_some_and(|expires| expires <= OffsetDateTime::now_utc().unix_timestamp() + 30)
    {
        return problem(
            StatusCode::CONFLICT,
            "Google Drive authorization must be refreshed before opening Picker",
        );
    }
    ([(header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({ "access_token": tokens.access_token, "expires_at": tokens.expires_at_unix }))).into_response()
}

pub async fn browse(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<BrowseQuery>,
) -> Response {
    let (config, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(id) = Uuid::parse_str(&id).map(SourceConnectionId) else {
        return problem(StatusCode::BAD_REQUEST, "invalid connection identifier");
    };
    let (provider, tokens) = match decrypted_tokens(&context, &config, id, &actor.id).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let result = match provider {
        SourceProvider::GoogleDrive => {
            return problem(
                StatusCode::BAD_REQUEST,
                "Google Drive selection uses Google Picker",
            );
        }
        SourceProvider::MicrosoftGraph => browse_microsoft(&tokens.access_token, &query).await,
        SourceProvider::Notion => browse_notion(&tokens.access_token, &query).await,
    };
    match result {
        Ok(value) => ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response(),
        Err(response) => response,
    }
}

pub async fn disconnect(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (_, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(id) = Uuid::parse_str(&id).map(SourceConnectionId) else {
        return problem(StatusCode::BAD_REQUEST, "invalid connection identifier");
    };
    match SeaOrmSourceConnectionRepository::new(context.db)
        .disconnect_connection(crate::default_organization_id(), id, &actor.id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => application_error(error),
    }
}

#[allow(clippy::too_many_lines)]
pub async fn sync(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SyncRequest>,
) -> Response {
    let (config, actor) = match services(&context, &headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(connection_id) = Uuid::parse_str(&id).map(SourceConnectionId) else {
        return problem(StatusCode::BAD_REQUEST, "invalid connection identifier");
    };
    let organization_id = crate::default_organization_id();
    let connections = SeaOrmSourceConnectionRepository::new(context.db.clone());
    let connection = match connections
        .list_connections(organization_id, &actor.id)
        .await
    {
        Ok(values) => values.into_iter().find(|value| value.id == connection_id),
        Err(error) => return application_error(error),
    };
    if connection.is_none() {
        return problem(StatusCode::NOT_FOUND, "source connection was not found");
    }
    let collection_id = match request.collection_id {
        Some(value) => match Uuid::parse_str(&value) {
            Ok(value) => Some(PolicyCollectionId(value)),
            Err(_) => return problem(StatusCode::BAD_REQUEST, "invalid destination collection"),
        },
        None => None,
    };
    let requested_ids: std::collections::HashSet<_> = request
        .subscription_ids
        .iter()
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect();
    if requested_ids.len() != request.subscription_ids.len() {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid source subscription identifier",
        );
    }
    let mut subscriptions = match connections
        .list_subscriptions(organization_id, connection_id)
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    subscriptions.retain(|subscription| {
        subscription.status == SourceSubscriptionStatus::Active
            && (requested_ids.is_empty() || requested_ids.contains(&subscription.id.0))
    });
    if subscriptions.is_empty() || subscriptions.len() > config.max_items_per_batch {
        return problem(
            StatusCode::BAD_REQUEST,
            "no active selected sources are available to sync",
        );
    }
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let items: Vec<_> = subscriptions
        .into_iter()
        .enumerate()
        .map(|(ordinal, subscription)| SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            client_item_key: format!("sync:{}", subscription.id),
            connection_id: Some(connection_id),
            subscription_id: Some(subscription.id),
            external_item_id: Some(subscription.external_item_id),
            status: SourceIngestionItemStatus::Pending,
            policy_import_id: None,
            failure_code: None,
            failure_detail: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
        })
        .collect();
    let batch = SourceIngestionBatch {
        id: batch_id,
        organization_id,
        policy_collection_id: collection_id,
        kind: SourceIngestionBatchKind::Sync,
        status: SourceIngestionBatchStatus::Pending,
        requested_by: actor.id,
        total_count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        succeeded_count: 0,
        failed_count: 0,
        unchanged_count: 0,
        created_at: now,
        updated_at: now,
    };
    let ingestion = SeaOrmSourceIngestionRepository::new(context.db.clone());
    if let Err(error) = ingestion.create_batch(&batch, &items).await {
        return application_error(error);
    }
    for item in items {
        if AcquirePolicySourceWorker::perform_later(
            &context,
            AcquirePolicySourceArgs {
                organization_id,
                source_ingestion_item_id: item.id,
            },
        )
        .await
        .is_err()
        {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    None,
                    Some((
                        "queue_unavailable",
                        "source acquisition queue is unavailable",
                    )),
                )
                .await;
        }
    }
    let batch = ingestion
        .recompute_batch(organization_id, batch_id)
        .await
        .unwrap_or(batch);
    (StatusCode::ACCEPTED, Json(batch)).into_response()
}

async fn exchange_code(
    provider: SourceProvider,
    config: &ProviderOAuthConfig,
    code: &str,
    verifier: Option<&str>,
) -> Result<OAuthTokenResponse, Response> {
    let endpoint = match provider {
        SourceProvider::GoogleDrive => "https://oauth2.googleapis.com/token",
        SourceProvider::MicrosoftGraph => {
            "https://login.microsoftonline.com/common/oauth2/v2.0/token"
        }
        SourceProvider::Notion => "https://api.notion.com/v1/oauth/token",
    };
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", config.callback_url.as_str()),
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.expose_secret()),
    ];
    if let Some(verifier) = verifier {
        form.push(("code_verifier", verifier));
    }
    let request = reqwest::Client::new().post(endpoint);
    let response = if provider == SourceProvider::Notion {
        request
            .basic_auth(
                &config.client_id,
                Some(config.client_secret.expose_secret()),
            )
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "code": code,
                "redirect_uri": config.callback_url,
            }))
            .send()
            .await
    } else {
        request.form(&form).send().await
    }
    .map_err(|_| problem(StatusCode::BAD_GATEWAY, "provider token exchange failed"))?;
    if !response.status().is_success() {
        return Err(problem(
            StatusCode::BAD_GATEWAY,
            "provider token exchange failed",
        ));
    }
    response.json().await.map_err(|_| {
        problem(
            StatusCode::BAD_GATEWAY,
            "provider token response was invalid",
        )
    })
}

async fn decrypted_tokens(
    context: &AppContext,
    config: &SourceConnectorConfig,
    id: SourceConnectionId,
    actor_id: &str,
) -> Result<(SourceProvider, StoredTokens), Response> {
    let stored = SeaOrmSourceConnectionRepository::new(context.db.clone())
        .encrypted_credential(crate::default_organization_id(), id, actor_id)
        .await
        .map_err(application_error)?;
    let active_version = config.active_key_version.ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "connector encryption is unavailable",
        )
    })?;
    let nonce: [u8; 12] = stored
        .nonce
        .try_into()
        .map_err(|_| problem(StatusCode::CONFLICT, "stored credential is invalid"))?;
    let cipher = CredentialCipher::from_base64_keys(&config.encryption_keys, active_version)
        .map_err(|_| {
            problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "connector encryption is unavailable",
            )
        })?;
    let decrypted = cipher
        .decrypt(
            &CredentialContext {
                organization_id: crate::default_organization_id().to_string(),
                connection_id: id.to_string(),
                provider: provider_name(stored.provider).to_owned(),
            },
            &EncryptedCredential {
                ciphertext: stored.ciphertext,
                nonce,
                key_version: stored.key_version,
            },
        )
        .map_err(|_| problem(StatusCode::CONFLICT, "stored credential is invalid"))?;
    let tokens = serde_json::from_str(decrypted.plaintext.expose_secret())
        .map_err(|_| problem(StatusCode::CONFLICT, "stored credential is invalid"))?;
    Ok((stored.provider, tokens))
}

#[allow(clippy::too_many_lines)]
async fn browse_microsoft(
    access_token: &str,
    query: &BrowseQuery,
) -> Result<RemoteBrowseResponse, Response> {
    if query
        .drive_id
        .as_ref()
        .is_some_and(|value| value.chars().count() > 256)
        || query
            .parent_id
            .as_ref()
            .is_some_and(|value| value.chars().count() > 256)
        || query
            .cursor
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2048)
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "Microsoft browse parameters are invalid",
        ));
    }
    let mut url = url::Url::parse("https://graph.microsoft.com/v1.0/me/drives")
        .map_err(|_| problem(StatusCode::BAD_GATEWAY, "Microsoft browse is unavailable"))?;
    let listing_drives = query.drive_id.is_none();
    if let Some(drive_id) = &query.drive_id {
        url = url::Url::parse("https://graph.microsoft.com/v1.0/drives/")
            .map_err(|_| problem(StatusCode::BAD_GATEWAY, "Microsoft browse is unavailable"))?;
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| problem(StatusCode::BAD_GATEWAY, "Microsoft browse is unavailable"))?;
        segments.push(drive_id);
        if let Some(parent_id) = &query.parent_id {
            segments.push("items").push(parent_id).push("children");
        } else {
            segments.push("root").push("children");
        }
    }
    url.query_pairs_mut()
        .append_pair("$top", "100")
        .append_pair(
            "$select",
            "id,name,driveType,folder,file,size,lastModifiedDateTime,webUrl",
        );
    if let Some(cursor) = &query.cursor {
        url.query_pairs_mut().append_pair("$skiptoken", cursor);
    }
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| problem(StatusCode::BAD_GATEWAY, "Microsoft browse failed"))?;
    if !response.status().is_success() {
        return Err(problem(StatusCode::BAD_GATEWAY, "Microsoft browse failed"));
    }
    let payload: serde_json::Value = response.json().await.map_err(|_| {
        problem(
            StatusCode::BAD_GATEWAY,
            "Microsoft browse response was invalid",
        )
    })?;
    let items = payload
        .get("value")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            let name = item.get("name")?.as_str()?.to_owned();
            let is_folder = item.get("folder").is_some();
            let kind = if listing_drives {
                "drive"
            } else if is_folder {
                "folder"
            } else {
                "file"
            };
            let mime_type = item
                .pointer("/file/mimeType")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            if kind == "file" && !is_supported_remote_file(&name, mime_type.as_deref()) {
                return None;
            }
            Some(RemoteBrowseItem {
                id: if kind == "file" {
                    format!("{}:{id}", query.drive_id.as_deref().unwrap_or_default())
                } else {
                    id
                },
                name,
                kind,
                mime_type,
                size: item.get("size").and_then(serde_json::Value::as_u64),
                modified_at: item
                    .get("lastModifiedDateTime")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                canonical_url: item
                    .get("webUrl")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                drive_id: if listing_drives {
                    item.get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                } else {
                    query.drive_id.clone()
                },
            })
        })
        .collect();
    let next_cursor = payload
        .get("@odata.nextLink")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| url::Url::parse(value).ok())
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "$skiptoken")
                .map(|(_, value)| value.into_owned())
        });
    Ok(RemoteBrowseResponse { items, next_cursor })
}

async fn browse_notion(
    access_token: &str,
    query: &BrowseQuery,
) -> Result<RemoteBrowseResponse, Response> {
    if query
        .query
        .as_ref()
        .is_some_and(|value| value.chars().count() > 200)
        || query
            .cursor
            .as_ref()
            .is_some_and(|value| value.chars().count() > 256)
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "Notion browse parameters are invalid",
        ));
    }
    let mut body = serde_json::json!({
        "page_size": 100,
        "filter": { "property": "object", "value": "page" },
    });
    if let Some(value) = &query.query {
        body["query"] = serde_json::Value::String(value.clone());
    }
    if let Some(value) = &query.cursor {
        body["start_cursor"] = serde_json::Value::String(value.clone());
    }
    let response = reqwest::Client::new()
        .post("https://api.notion.com/v1/search")
        .bearer_auth(access_token)
        .header("Notion-Version", governance_connectors::NOTION_API_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|_| problem(StatusCode::BAD_GATEWAY, "Notion browse failed"))?;
    if !response.status().is_success() {
        return Err(problem(StatusCode::BAD_GATEWAY, "Notion browse failed"));
    }
    let payload: serde_json::Value = response.json().await.map_err(|_| {
        problem(
            StatusCode::BAD_GATEWAY,
            "Notion browse response was invalid",
        )
    })?;
    let items = payload
        .get("results")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|page| {
            let id = page.get("id")?.as_str()?.to_owned();
            Some(RemoteBrowseItem {
                name: notion_page_title(page.get("properties"))
                    .unwrap_or_else(|| "Untitled Notion page".to_owned()),
                id,
                kind: "file",
                mime_type: Some("text/markdown".to_owned()),
                size: None,
                modified_at: page
                    .get("last_edited_time")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                canonical_url: page
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                drive_id: None,
            })
        })
        .collect();
    Ok(RemoteBrowseResponse {
        items,
        next_cursor: payload
            .get("next_cursor")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn notion_page_title(properties: Option<&serde_json::Value>) -> Option<String> {
    let title = properties?
        .as_object()?
        .values()
        .find_map(|property| property.get("title").and_then(serde_json::Value::as_array))?
        .iter()
        .filter_map(|part| part.get("plain_text").and_then(serde_json::Value::as_str))
        .collect::<String>();
    (!title.trim().is_empty()).then_some(title)
}

fn is_supported_remote_file(name: &str, mime_type: Option<&str>) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        mime_type,
        Some(
            "application/pdf"
                | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                | "text/plain"
                | "text/markdown"
        )
    ) || [".pdf", ".docx", ".txt", ".md", ".markdown"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

async fn provider_identity(
    provider: SourceProvider,
    token: &OAuthTokenResponse,
) -> Result<(String, String), Response> {
    if provider == SourceProvider::Notion {
        let id = token
            .workspace_id
            .clone()
            .or(token.bot_id.clone())
            .ok_or_else(|| {
                problem(
                    StatusCode::BAD_GATEWAY,
                    "provider identity response was invalid",
                )
            })?;
        return Ok((
            id,
            token
                .workspace_name
                .clone()
                .unwrap_or_else(|| provider_label(provider).to_owned()),
        ));
    }
    let endpoint = match provider {
        SourceProvider::GoogleDrive => "https://www.googleapis.com/oauth2/v3/userinfo",
        SourceProvider::MicrosoftGraph => {
            "https://graph.microsoft.com/v1.0/me?$select=id,displayName,userPrincipalName"
        }
        SourceProvider::Notion => unreachable!(),
    };
    let response = reqwest::Client::new()
        .get(endpoint)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|_| problem(StatusCode::BAD_GATEWAY, "provider identity lookup failed"))?;
    if !response.status().is_success() {
        return Err(problem(
            StatusCode::BAD_GATEWAY,
            "provider identity lookup failed",
        ));
    }
    let value: serde_json::Value = response.json().await.map_err(|_| {
        problem(
            StatusCode::BAD_GATEWAY,
            "provider identity response was invalid",
        )
    })?;
    let id = value
        .get(if provider == SourceProvider::GoogleDrive {
            "sub"
        } else {
            "id"
        })
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            problem(
                StatusCode::BAD_GATEWAY,
                "provider identity response was invalid",
            )
        })?
        .to_owned();
    let label = if provider == SourceProvider::GoogleDrive {
        value.get("email")
    } else {
        value
            .get("displayName")
            .or_else(|| value.get("userPrincipalName"))
    }
    .and_then(serde_json::Value::as_str)
    .filter(|value| !value.is_empty())
    .unwrap_or(provider_label(provider))
    .to_owned();
    Ok((id, label))
}

fn provider_config(
    config: &SourceConnectorConfig,
    provider: SourceProvider,
) -> Option<&ProviderOAuthConfig> {
    match provider {
        SourceProvider::GoogleDrive => config.google.as_ref(),
        SourceProvider::MicrosoftGraph => config.microsoft.as_ref(),
        SourceProvider::Notion => config.notion.as_ref(),
    }
}
fn parse_provider(value: &str) -> Option<SourceProvider> {
    match value {
        "google_drive" => Some(SourceProvider::GoogleDrive),
        "microsoft_graph" => Some(SourceProvider::MicrosoftGraph),
        "notion" => Some(SourceProvider::Notion),
        _ => None,
    }
}
fn provider_name(provider: SourceProvider) -> &'static str {
    match provider {
        SourceProvider::GoogleDrive => "google_drive",
        SourceProvider::MicrosoftGraph => "microsoft_graph",
        SourceProvider::Notion => "notion",
    }
}
fn provider_label(provider: SourceProvider) -> &'static str {
    match provider {
        SourceProvider::GoogleDrive => "Google Drive",
        SourceProvider::MicrosoftGraph => "Microsoft 365",
        SourceProvider::Notion => "Notion",
    }
}
