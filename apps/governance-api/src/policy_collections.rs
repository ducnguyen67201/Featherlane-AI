#![allow(clippy::result_large_err, clippy::too_many_lines)]

use std::collections::{BTreeMap, HashSet};

use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use governance_application::{
    ClonePolicyCollectionCommand, CompilePolicyCollectionCommand, CreatePolicyCollectionCommand,
    CreatePolicyImport, NewPolicyImport, PolicyCollectionRepository, PolicyImportAcquisition,
    PolicyImportRepository, SourceConnectionRepository, SourceIngestionRepository,
    clone_policy_collection, compile_policy_collection, create_policy_collection,
    detect_document_format, policy_collection_readiness,
};
use governance_domain::{
    PolicyCollectionId, PolicyInputKind, SourceConnectionId, SourceIngestionBatch,
    SourceIngestionBatchId, SourceIngestionBatchKind, SourceIngestionBatchStatus,
    SourceIngestionItem, SourceIngestionItemId, SourceIngestionItemStatus, SourceType,
};
use governance_persistence::{
    NewSourceSubscription, SeaOrmPolicyCollectionRepository, SeaOrmPolicyImportRepository,
    SeaOrmSourceConnectionRepository, SeaOrmSourceIngestionRepository,
};
use governance_worker::{
    AcquirePolicySourceArgs, AcquirePolicySourceWorker, ProcessPolicyImportArgs,
    ProcessPolicyImportWorker,
};
use loco_rs::{app::AppContext, bgworker::BackgroundWorker};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{console_auth::authenticate, loco_app::problem, policy_imports::application_error};

#[derive(Debug, Deserialize)]
pub struct CreateCollectionRequest {
    key: String,
    version: u32,
    title: String,
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddImportRequest {
    policy_import_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CloneCollectionRequest {
    version: u32,
    title: String,
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadManifest {
    source_type: SourceType,
    jurisdiction: String,
    items: Vec<UploadManifestItem>,
}

#[derive(Debug, Deserialize)]
struct UploadManifestItem {
    client_item_key: String,
    title: String,
    source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UrlBatchRequest {
    items: Vec<UrlBatchItem>,
}

#[derive(Debug, Deserialize)]
pub struct PasteBatchRequest {
    source_type: SourceType,
    jurisdiction: String,
    items: Vec<PasteBatchItem>,
}

#[derive(Debug, Deserialize)]
struct PasteBatchItem {
    client_item_key: String,
    title: String,
    text: String,
    source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderSelectionRequest {
    connection_id: String,
    items: Vec<ProviderSelectionItem>,
}

#[derive(Debug, Deserialize)]
struct ProviderSelectionItem {
    client_item_key: String,
    external_item_id: String,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UrlBatchItem {
    client_item_key: String,
    url: String,
}

fn actor(
    context: &AppContext,
    headers: &HeaderMap,
) -> Result<crate::console_auth::ConsoleActor, Response> {
    let config = context
        .shared_store
        .get::<governance_config::SourceConnectorConfig>()
        .ok_or_else(|| {
            problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "connector services are unavailable",
            )
        })?;
    authenticate(headers, &config)
}

pub async fn create_collection(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Json(request): Json<CreateCollectionRequest>,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let repository = SeaOrmPolicyCollectionRepository::new(context.db);
    match create_policy_collection(
        &repository,
        CreatePolicyCollectionCommand {
            organization_id: crate::default_organization_id(),
            key: request.key,
            version: request.version,
            title: request.title,
            created_by: actor.id,
            idempotency_key: request.idempotency_key,
        },
    )
    .await
    {
        Ok(collection) => (StatusCode::CREATED, Json(collection)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn list_collections(State(context): State<AppContext>, headers: HeaderMap) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    match SeaOrmPolicyCollectionRepository::new(context.db)
        .list(crate::default_organization_id())
        .await
    {
        Ok(collections) => Json(collections).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn get_collection(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let Some(id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    let repository = SeaOrmPolicyCollectionRepository::new(context.db.clone());
    let Some(collection) = (match repository.get(crate::default_organization_id(), id).await {
        Ok(value) => value,
        Err(error) => return application_error(error),
    }) else {
        return problem(StatusCode::NOT_FOUND, "policy collection was not found");
    };
    let members = match repository
        .members(crate::default_organization_id(), id)
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    let batches = match SeaOrmSourceIngestionRepository::new(context.db)
        .list_batches(crate::default_organization_id(), id, 20)
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    Json(serde_json::json!({ "collection": collection, "members": members, "batches": batches }))
        .into_response()
}

pub async fn add_import(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<AddImportRequest>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let (Some(collection_id), Ok(import_uuid)) = (
        parse_collection_id(&id),
        Uuid::parse_str(&request.policy_import_id),
    ) else {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid collection or import identifier",
        );
    };
    let organization_id = crate::default_organization_id();
    let imports = SeaOrmPolicyImportRepository::new(context.db.clone());
    let Some(import) = (match imports
        .get(
            organization_id,
            governance_domain::PolicyImportId(import_uuid),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return application_error(error),
    }) else {
        return problem(StatusCode::NOT_FOUND, "policy import was not found");
    };
    match SeaOrmPolicyCollectionRepository::new(context.db)
        .add_import(organization_id, collection_id, &import)
        .await
    {
        Ok(member) => (StatusCode::CREATED, Json(member)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn remove_import(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path((id, import_id)): Path<(String, String)>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let (Some(collection_id), Ok(import_id)) = (
        parse_collection_id(&id),
        Uuid::parse_str(&import_id).map(governance_domain::PolicyImportId),
    ) else {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid collection or import identifier",
        );
    };
    match SeaOrmPolicyCollectionRepository::new(context.db)
        .remove_import(crate::default_organization_id(), collection_id, import_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn clone_collection(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CloneCollectionRequest>,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(source_collection_id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    match clone_policy_collection(
        &SeaOrmPolicyCollectionRepository::new(context.db.clone()),
        &SeaOrmPolicyImportRepository::new(context.db),
        ClonePolicyCollectionCommand {
            organization_id: crate::default_organization_id(),
            source_collection_id,
            version: request.version,
            title: request.title,
            created_by: actor.id,
            idempotency_key: request.idempotency_key,
        },
    )
    .await
    {
        Ok(collection) => (StatusCode::CREATED, Json(collection)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn readiness(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let Some(id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    match policy_collection_readiness(
        &SeaOrmPolicyCollectionRepository::new(context.db.clone()),
        &SeaOrmPolicyImportRepository::new(context.db),
        crate::default_organization_id(),
        id,
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn compile(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let Some(id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    match compile_policy_collection(
        &SeaOrmPolicyCollectionRepository::new(context.db.clone()),
        &SeaOrmPolicyImportRepository::new(context.db),
        CompilePolicyCollectionCommand {
            organization_id: crate::default_organization_id(),
            policy_collection_id: id,
        },
    )
    .await
    {
        Ok(pack) => (StatusCode::CREATED, Json(pack)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn upload_batch(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(collection_id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    let connector_config = governance_config::SourceConnectorConfig::from_env();
    let import_services = match crate::policy_imports::PolicyImportServices::from_env() {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    let connector_config = match connector_config {
        Ok(value) => value,
        Err(error) => return problem(StatusCode::SERVICE_UNAVAILABLE, &error.to_string()),
    };
    let mut manifest = None;
    let mut files = BTreeMap::new();
    let mut aggregate_bytes = 0_usize;
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "manifest" {
            manifest = field
                .text()
                .await
                .ok()
                .and_then(|value| serde_json::from_str::<UploadManifest>(&value).ok());
            continue;
        }
        let Some(client_key) = name.strip_prefix("file:").map(ToOwned::to_owned) else {
            continue;
        };
        let mut bytes = Vec::new();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    aggregate_bytes = aggregate_bytes.saturating_add(chunk.len());
                    if bytes.len().saturating_add(chunk.len()) > import_services.config.max_bytes
                        || aggregate_bytes > connector_config.max_batch_bytes
                    {
                        return problem(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "source batch exceeds configured limits",
                        );
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => {
                    return problem(StatusCode::BAD_REQUEST, "multipart source batch is invalid");
                }
            }
        }
        files.insert(client_key, bytes);
    }
    let Some(manifest) = manifest else {
        return problem(
            StatusCode::BAD_REQUEST,
            "a valid upload manifest is required",
        );
    };
    if manifest.items.is_empty() || manifest.items.len() > connector_config.max_items_per_batch {
        return problem(
            StatusCode::BAD_REQUEST,
            "source batch item count is invalid",
        );
    }
    let keys: HashSet<_> = manifest
        .items
        .iter()
        .map(|item| item.client_item_key.as_str())
        .collect();
    if keys.len() != manifest.items.len() || keys.iter().any(|key| !files.contains_key(*key)) {
        return problem(
            StatusCode::BAD_REQUEST,
            "manifest keys must be unique and match file parts",
        );
    }
    let organization_id = crate::default_organization_id();
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let items: Vec<_> = manifest
        .items
        .iter()
        .enumerate()
        .map(|(ordinal, item)| SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            client_item_key: item.client_item_key.clone(),
            connection_id: None,
            subscription_id: None,
            external_item_id: None,
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
        policy_collection_id: Some(collection_id),
        kind: SourceIngestionBatchKind::Upload,
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
    let imports = SeaOrmPolicyImportRepository::new(context.db.clone());
    let collections = SeaOrmPolicyCollectionRepository::new(context.db.clone());
    for (request, item) in manifest.items.into_iter().zip(items) {
        if let Err(error) = ingestion.claim_item(organization_id, item.id).await {
            return application_error(error);
        }
        let source_bytes = files.remove(&request.client_item_key).unwrap_or_default();
        let Ok((_, mime)) = detect_document_format(&source_bytes) else {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    None,
                    Some((
                        "unsupported_remote_type",
                        "unsupported policy source format",
                    )),
                )
                .await;
            continue;
        };
        let imported = CreatePolicyImport::new(imports.clone(), import_services.artifacts.clone())
            .execute_prepared(
                NewPolicyImport {
                    organization_id,
                    input_kind: PolicyInputKind::File,
                    source_type: manifest.source_type,
                    title: request.title,
                    jurisdiction: manifest.jurisdiction.clone(),
                    effective_from: None,
                    source_url: request.source_url,
                    original_filename: Some(request.client_item_key.clone()),
                    declared_mime_type: Some(mime.to_owned()),
                    detected_mime_type: mime.to_owned(),
                    content: source_bytes,
                    idempotency_key: Some(format!("batch-item:{}", item.id)),
                    supersedes_import_id: None,
                },
                None,
                PolicyImportAcquisition {
                    ingestion_item_id: Some(item.id),
                    ..PolicyImportAcquisition::default()
                },
            )
            .await;
        let imported = imported.map_err(|error| {
            tracing::warn!(ingestion_item_id = %item.id, error = %error, "policy import creation failed");
            error
        });
        let Ok(import) = imported else {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    None,
                    Some((
                        "import_creation_failed",
                        "policy import could not be created",
                    )),
                )
                .await;
            continue;
        };
        if let Err(error) = collections
            .add_import(organization_id, collection_id, &import)
            .await
        {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    Some(import.id),
                    Some((
                        "collection_link_failed",
                        "source could not be linked to the collection",
                    )),
                )
                .await;
            tracing::warn!(policy_import_id = %import.id, error = %error, "collection membership failed");
            continue;
        }
        let _ = ingestion
            .update_item(
                organization_id,
                item.id,
                SourceIngestionItemStatus::Queued,
                Some(import.id),
                None,
            )
            .await;
        if ProcessPolicyImportWorker::perform_later(
            &context,
            ProcessPolicyImportArgs {
                organization_id,
                policy_import_id: import.id,
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
                    Some(import.id),
                    Some((
                        "queue_unavailable",
                        "source processing queue is unavailable",
                    )),
                )
                .await;
        }
    }
    let batch = match ingestion.recompute_batch(organization_id, batch_id).await {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    (StatusCode::ACCEPTED, Json(batch)).into_response()
}

pub async fn url_batch(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UrlBatchRequest>,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(collection_id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    let config = match governance_config::SourceConnectorConfig::from_env() {
        Ok(value) => value,
        Err(error) => return problem(StatusCode::SERVICE_UNAVAILABLE, &error.to_string()),
    };
    if request.items.is_empty() || request.items.len() > config.max_items_per_batch {
        return problem(StatusCode::BAD_REQUEST, "URL batch item count is invalid");
    }
    let keys: HashSet<_> = request
        .items
        .iter()
        .map(|item| item.client_item_key.as_str())
        .collect();
    if keys.len() != request.items.len() {
        return problem(
            StatusCode::BAD_REQUEST,
            "URL batch client keys must be unique",
        );
    }
    let organization_id = crate::default_organization_id();
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let items: Vec<_> = request
        .items
        .into_iter()
        .enumerate()
        .map(|(ordinal, request)| SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            client_item_key: request.client_item_key,
            connection_id: None,
            subscription_id: None,
            external_item_id: Some(request.url),
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
        policy_collection_id: Some(collection_id),
        kind: SourceIngestionBatchKind::Url,
        status: SourceIngestionBatchStatus::Pending,
        requested_by: actor.id,
        total_count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        succeeded_count: 0,
        failed_count: 0,
        unchanged_count: 0,
        created_at: now,
        updated_at: now,
    };
    let repository = SeaOrmSourceIngestionRepository::new(context.db.clone());
    if let Err(error) = repository.create_batch(&batch, &items).await {
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
            let _ = repository
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
    let batch = repository
        .recompute_batch(organization_id, batch_id)
        .await
        .unwrap_or(batch);
    (StatusCode::ACCEPTED, Json(batch)).into_response()
}

pub async fn paste_batch(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<PasteBatchRequest>,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(collection_id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    let config = match governance_config::SourceConnectorConfig::from_env() {
        Ok(value) => value,
        Err(error) => return problem(StatusCode::SERVICE_UNAVAILABLE, &error.to_string()),
    };
    let services = match crate::policy_imports::PolicyImportServices::from_env() {
        Ok(value) => value,
        Err(error) => return application_error(error),
    };
    let aggregate_bytes = request
        .items
        .iter()
        .map(|item| item.text.len())
        .sum::<usize>();
    let keys: HashSet<_> = request
        .items
        .iter()
        .map(|item| item.client_item_key.as_str())
        .collect();
    if request.items.is_empty()
        || request.items.len() > config.max_items_per_batch
        || keys.len() != request.items.len()
        || aggregate_bytes > config.max_batch_bytes
        || request.items.iter().any(|item| {
            item.text.trim().len() < 12
                || item.text.len() > services.config.max_bytes
                || item.title.trim().is_empty()
                || item.title.chars().count() > 240
        })
    {
        return problem(
            StatusCode::BAD_REQUEST,
            "pasted source batch is invalid or exceeds configured limits",
        );
    }
    let organization_id = crate::default_organization_id();
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let items: Vec<_> = request
        .items
        .iter()
        .enumerate()
        .map(|(ordinal, item)| SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            client_item_key: item.client_item_key.clone(),
            connection_id: None,
            subscription_id: None,
            external_item_id: None,
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
        policy_collection_id: Some(collection_id),
        kind: SourceIngestionBatchKind::Paste,
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
    let imports = SeaOrmPolicyImportRepository::new(context.db.clone());
    let collections = SeaOrmPolicyCollectionRepository::new(context.db.clone());
    for (source, item) in request.items.into_iter().zip(items) {
        if ingestion
            .claim_item(organization_id, item.id)
            .await
            .is_err()
        {
            continue;
        }
        let imported = CreatePolicyImport::new(imports.clone(), services.artifacts.clone())
            .execute_prepared(
                NewPolicyImport {
                    organization_id,
                    input_kind: PolicyInputKind::PastedText,
                    source_type: request.source_type,
                    title: source.title,
                    jurisdiction: request.jurisdiction.clone(),
                    effective_from: None,
                    source_url: source.source_url,
                    original_filename: None,
                    declared_mime_type: Some("text/plain".to_owned()),
                    detected_mime_type: "text/plain".to_owned(),
                    content: source.text.into_bytes(),
                    idempotency_key: Some(format!("batch-item:{}", item.id)),
                    supersedes_import_id: None,
                },
                None,
                PolicyImportAcquisition {
                    ingestion_item_id: Some(item.id),
                    ..PolicyImportAcquisition::default()
                },
            )
            .await;
        let imported = imported.map_err(|error| {
            tracing::warn!(ingestion_item_id = %item.id, error = %error, "pasted policy import creation failed");
            error
        });
        let Ok(import) = imported else {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    None,
                    Some((
                        "import_creation_failed",
                        "pasted source could not be created",
                    )),
                )
                .await;
            continue;
        };
        if collections
            .add_import(organization_id, collection_id, &import)
            .await
            .is_err()
        {
            let _ = ingestion
                .update_item(
                    organization_id,
                    item.id,
                    SourceIngestionItemStatus::Failed,
                    Some(import.id),
                    Some((
                        "collection_link_failed",
                        "source could not be linked to the collection",
                    )),
                )
                .await;
            continue;
        }
        let _ = ingestion
            .update_item(
                organization_id,
                item.id,
                SourceIngestionItemStatus::Queued,
                Some(import.id),
                None,
            )
            .await;
        if ProcessPolicyImportWorker::perform_later(
            &context,
            ProcessPolicyImportArgs {
                organization_id,
                policy_import_id: import.id,
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
                    Some(import.id),
                    Some((
                        "queue_unavailable",
                        "source processing queue is unavailable",
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

pub async fn provider_selection_batch(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ProviderSelectionRequest>,
) -> Response {
    let actor = match actor(&context, &headers) {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(collection_id) = parse_collection_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid collection identifier");
    };
    let Ok(connection_id) = Uuid::parse_str(&request.connection_id).map(SourceConnectionId) else {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid source connection identifier",
        );
    };
    let config = match governance_config::SourceConnectorConfig::from_env() {
        Ok(value) => value,
        Err(error) => return problem(StatusCode::SERVICE_UNAVAILABLE, &error.to_string()),
    };
    if request.items.is_empty() || request.items.len() > config.max_items_per_batch {
        return problem(
            StatusCode::BAD_REQUEST,
            "provider selection item count is invalid",
        );
    }
    let keys: HashSet<_> = request
        .items
        .iter()
        .map(|item| item.client_item_key.as_str())
        .collect();
    let external_ids: HashSet<_> = request
        .items
        .iter()
        .map(|item| item.external_item_id.as_str())
        .collect();
    if keys.len() != request.items.len()
        || external_ids.len() != request.items.len()
        || request.items.iter().any(|item| {
            item.client_item_key.is_empty()
                || item.client_item_key.chars().count() > 128
                || item.external_item_id.trim().is_empty()
                || item.external_item_id.chars().count() > 1024
        })
    {
        return problem(
            StatusCode::BAD_REQUEST,
            "provider selections must be unique and bounded",
        );
    }
    let organization_id = crate::default_organization_id();
    let connections = SeaOrmSourceConnectionRepository::new(context.db.clone());
    let connection = match connections
        .list_connections(organization_id, &actor.id)
        .await
    {
        Ok(values) => values.into_iter().find(|value| value.id == connection_id),
        Err(error) => return application_error(error),
    };
    let Some(connection) = connection else {
        return problem(StatusCode::NOT_FOUND, "source connection was not found");
    };
    if connection.status != governance_domain::SourceConnectionStatus::Active {
        return problem(StatusCode::CONFLICT, "source connection must be active");
    }
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let mut items = Vec::with_capacity(request.items.len());
    for (ordinal, request_item) in request.items.into_iter().enumerate() {
        let subscription = match connections
            .upsert_subscription(NewSourceSubscription {
                organization_id,
                connection_id,
                provider: connection.provider,
                external_item_id: request_item.external_item_id.clone(),
                title_hint: request_item.title,
                actor_id: actor.id.clone(),
            })
            .await
        {
            Ok(value) => value,
            Err(error) => return application_error(error),
        };
        items.push(SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            client_item_key: request_item.client_item_key,
            connection_id: Some(connection_id),
            subscription_id: Some(subscription.id),
            external_item_id: Some(request_item.external_item_id),
            status: SourceIngestionItemStatus::Pending,
            policy_import_id: None,
            failure_code: None,
            failure_detail: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
        });
    }
    let kind = match connection.provider {
        governance_domain::SourceProvider::GoogleDrive => SourceIngestionBatchKind::GoogleDrive,
        governance_domain::SourceProvider::MicrosoftGraph => {
            SourceIngestionBatchKind::MicrosoftGraph
        }
        governance_domain::SourceProvider::Notion => SourceIngestionBatchKind::Notion,
    };
    let batch = SourceIngestionBatch {
        id: batch_id,
        organization_id,
        policy_collection_id: Some(collection_id),
        kind,
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

pub async fn get_batch(
    State(context): State<AppContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = actor(&context, &headers) {
        return response;
    }
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid batch identifier");
    };
    match SeaOrmSourceIngestionRepository::new(context.db)
        .get_batch(
            crate::default_organization_id(),
            SourceIngestionBatchId(uuid),
        )
        .await
    {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => problem(
            StatusCode::NOT_FOUND,
            "source ingestion batch was not found",
        ),
        Err(error) => application_error(error),
    }
}

fn parse_collection_id(value: &str) -> Option<PolicyCollectionId> {
    Uuid::parse_str(value).ok().map(PolicyCollectionId)
}
