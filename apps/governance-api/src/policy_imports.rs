#![allow(
    clippy::manual_let_else,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_lines
)]

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use governance_application::{
    ApplicationError, CandidateEdit, CompilePolicyImportCommand, CreatePolicyImport,
    ManualCandidateCommand, NewPolicyImport, PolicyImportRepository, ReviewCandidateCommand,
    SourceArtifactStore, VerifySourceCommand, add_manual_policy_candidate, compile_policy_import,
    detect_document_format, refresh_import_readiness, review_policy_candidate,
};
use governance_config::PolicyImportConfig;
use governance_domain::{
    ParsedDocument, PolicyCandidateId, PolicyCandidateStatus, PolicyImport, PolicyImportCoverage,
    PolicyImportId, PolicyImportStatus, PolicyInputKind, PolicyPackId, RuleSuggestion, Severity,
    SourceId, SourceLocator, SourceType, SourceVerificationStatus,
};
use governance_ingestion::{ConfiguredPolicyExtractionModel, OpenDalArtifactStore};
use governance_persistence::SeaOrmPolicyImportRepository;
use governance_worker::{ProcessPolicyImportArgs, ProcessPolicyImportWorker};
use loco_rs::{app::AppContext, bgworker::BackgroundWorker};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct PolicyImportServices {
    config: PolicyImportConfig,
    artifacts: OpenDalArtifactStore,
    model: ConfiguredPolicyExtractionModel,
}

impl PolicyImportServices {
    pub(crate) fn from_env() -> Result<Self, ApplicationError> {
        let config = PolicyImportConfig::from_env()
            .map_err(|error| ApplicationError::Unavailable(error.to_string()))?;
        let artifacts = OpenDalArtifactStore::from_config(&config)?;
        let model = ConfiguredPolicyExtractionModel::from_config(&config)?;
        Ok(Self {
            config,
            artifacts,
            model,
        })
    }
}

fn services(context: &AppContext) -> Result<PolicyImportServices, ApplicationError> {
    context
        .shared_store
        .get::<PolicyImportServices>()
        .ok_or_else(|| {
            ApplicationError::Unavailable("policy import services were not initialized".to_owned())
        })
}

use crate::loco_app::problem;

#[derive(Debug, Deserialize)]
pub struct ImportListQuery {
    limit: Option<u64>,
    status: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CandidateListQuery {
    limit: Option<usize>,
    status: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SourceContextQuery {
    candidate_id: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifySourceRequest {
    decision: String,
    reviewer_id: String,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Deserialize)]
pub struct ReviewCandidateRequest {
    decision: PolicyCandidateStatus,
    reviewer_id: String,
    #[serde(default)]
    notes: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    expected_updated_at: Option<OffsetDateTime>,
    candidate: Option<CandidateEdit>,
}

#[derive(Debug, Deserialize)]
pub struct ManualCandidateRequest {
    reviewer_id: String,
    statement: String,
    source_excerpt: String,
    locator: SourceLocator,
    #[serde(default)]
    applicability: Value,
    #[serde(default)]
    exceptions: Vec<String>,
    #[serde(default)]
    required_evidence: Vec<String>,
    suggested_severity: Severity,
    suggested_rule: RuleSuggestion,
}

#[derive(Debug, Deserialize)]
pub struct CompileImportRequest {
    key: String,
    version: u32,
    title: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PolicyImportView {
    pub id: PolicyImportId,
    pub policy_source_id: governance_domain::PolicySourceId,
    pub revision: u32,
    pub supersedes_import_id: Option<PolicyImportId>,
    pub status: PolicyImportStatus,
    pub input_kind: PolicyInputKind,
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub effective_from: Option<OffsetDateTime>,
    pub source_url: Option<String>,
    pub original_filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub detected_mime_type: String,
    pub byte_length: u64,
    pub content_sha256: String,
    pub parser_kind: Option<String>,
    pub parser_version: Option<String>,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    pub page_count: Option<u32>,
    pub coverage: PolicyImportCoverage,
    pub candidate_count: u32,
    pub verification_status: SourceVerificationStatus,
    pub verified_by: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub verified_at: Option<OffsetDateTime>,
    pub verification_notes: Option<String>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub compiled_source_id: Option<SourceId>,
    pub compiled_policy_pack_id: Option<PolicyPackId>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

impl From<PolicyImport> for PolicyImportView {
    fn from(import: PolicyImport) -> Self {
        Self {
            id: import.id,
            policy_source_id: import.policy_source_id,
            revision: import.revision,
            supersedes_import_id: import.supersedes_import_id,
            status: import.status,
            input_kind: import.input_kind,
            source_type: import.source_type,
            title: import.title,
            jurisdiction: import.jurisdiction,
            effective_from: import.effective_from,
            source_url: import.source_url,
            original_filename: import.original_filename,
            declared_mime_type: import.declared_mime_type,
            detected_mime_type: import.detected_mime_type,
            byte_length: import.byte_length,
            content_sha256: import.content_sha256,
            parser_kind: import.parser_kind,
            parser_version: import.parser_version,
            model_provider: import.model_provider,
            model_name: import.model_name,
            prompt_version: import.prompt_version,
            page_count: import.page_count,
            coverage: import.coverage,
            candidate_count: import.candidate_count,
            verification_status: import.verification_status,
            verified_by: import.verified_by,
            verified_at: import.verified_at,
            verification_notes: import.verification_notes,
            failure_code: import.failure_code,
            failure_detail: import.failure_detail,
            compiled_source_id: import.compiled_source_id,
            compiled_policy_pack_id: import.compiled_policy_pack_id,
            created_at: import.created_at,
            updated_at: import.updated_at,
            completed_at: import.completed_at,
        }
    }
}

pub async fn create_policy_import(
    State(context): State<AppContext>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let services = match services(&context) {
        Ok(services) => services,
        Err(error) => return application_error(error),
    };
    if !services.config.llm_enabled
        || matches!(&services.model, ConfiguredPolicyExtractionModel::Disabled)
    {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "policy extraction is disabled; configure POLICY_LLM_ENABLED and an approved provider",
        );
    }
    let mut title = None;
    let mut source_type = None;
    let mut jurisdiction = None;
    let mut effective_from = None;
    let mut source_url = None;
    let mut supersedes_import_id = None;
    let mut file = None;
    let mut pasted_text = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => return problem(StatusCode::BAD_REQUEST, &error.to_string()),
        };
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().map(sanitize_filename);
                let declared_mime = field.content_type().map(ToOwned::to_owned);
                let mut field = field;
                let mut bytes = Vec::new();
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            if bytes.len().saturating_add(chunk.len()) > services.config.max_bytes {
                                return problem(
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    "policy source exceeds the 25 MiB limit",
                                );
                            }
                            bytes.extend_from_slice(&chunk);
                        }
                        Ok(None) => break,
                        Err(error) => {
                            return problem(StatusCode::BAD_REQUEST, &error.to_string());
                        }
                    }
                }
                file = Some((bytes, filename, declared_mime));
            }
            "text" => match field.text().await {
                Ok(value) => pasted_text = Some(value),
                Err(error) => return problem(StatusCode::BAD_REQUEST, &error.to_string()),
            },
            "title" => title = field.text().await.ok(),
            "source_type" => source_type = field.text().await.ok(),
            "jurisdiction" => jurisdiction = field.text().await.ok(),
            "effective_from" => effective_from = field.text().await.ok(),
            "source_url" => source_url = field.text().await.ok(),
            "supersedes_import_id" => supersedes_import_id = field.text().await.ok(),
            _ => {}
        }
    }
    if file.is_some()
        == pasted_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        return problem(
            StatusCode::BAD_REQUEST,
            "provide exactly one file or pasted policy text",
        );
    }
    let title = title.unwrap_or_default();
    let jurisdiction = jurisdiction.unwrap_or_default();
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if title.chars().count() > 500
        || jurisdiction.chars().count() > 200
        || source_url
            .as_deref()
            .is_some_and(|value| value.chars().count() > 2_048)
        || idempotency_key.chars().count() > 320
    {
        return problem(
            StatusCode::BAD_REQUEST,
            "policy import metadata exceeds the configured limit",
        );
    }
    let source_type = match source_type.and_then(|value| enum_from_form(&value).ok()) {
        Some(value) => value,
        None => return problem(StatusCode::BAD_REQUEST, "source_type is invalid"),
    };
    let parsed_effective_from = match effective_from.filter(|value| !value.trim().is_empty()) {
        Some(value) => match OffsetDateTime::parse(&value, &Rfc3339) {
            Ok(value) => Some(value),
            Err(_) => return problem(StatusCode::BAD_REQUEST, "effective_from must be RFC 3339"),
        },
        None => None,
    };
    let supersedes_import_id = match supersedes_import_id.filter(|value| !value.trim().is_empty()) {
        Some(value) => match parse_import_id(&value) {
            Some(value) => Some(value),
            None => {
                return problem(
                    StatusCode::BAD_REQUEST,
                    "supersedes_import_id must be a policy import identifier",
                );
            }
        },
        None => None,
    };
    let (content, original_filename, declared_mime_type, input_kind) = match file {
        Some((content, filename, mime)) => (content, filename, mime, PolicyInputKind::File),
        None => (
            pasted_text.unwrap_or_default().into_bytes(),
            None,
            Some("text/plain".to_owned()),
            PolicyInputKind::PastedText,
        ),
    };
    if content.len() > services.config.max_bytes {
        return problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "policy source exceeds the 25 MiB limit",
        );
    }
    let (_, detected_mime_type) = match detect_document_format(&content) {
        Ok(detected) => detected,
        Err(_) => {
            return problem(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "only PDF, DOCX, UTF-8 TXT, and pasted text are supported",
            );
        }
    };
    let idempotency_key = match headers.get("idempotency-key") {
        Some(value) => match value.to_str() {
            Ok(value) => Some(value.to_owned()),
            Err(_) => return problem(StatusCode::BAD_REQUEST, "Idempotency-Key is invalid"),
        },
        None => None,
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    let import = match CreatePolicyImport::new(repository.clone(), services.artifacts)
        .execute(NewPolicyImport {
            organization_id: crate::default_organization_id(),
            input_kind,
            source_type,
            title,
            jurisdiction,
            effective_from: parsed_effective_from,
            source_url: source_url.filter(|value| !value.trim().is_empty()),
            original_filename,
            declared_mime_type,
            detected_mime_type: detected_mime_type.to_owned(),
            content,
            idempotency_key,
            supersedes_import_id,
        })
        .await
    {
        Ok(import) => import,
        Err(error) => return application_error(error),
    };
    if let Err(_error) = ProcessPolicyImportWorker::perform_later(
        &context,
        ProcessPolicyImportArgs {
            organization_id: import.organization_id,
            policy_import_id: import.id,
        },
    )
    .await
    {
        let _ = repository
            .mark_failure(
                import.organization_id,
                import.id,
                PolicyImportStatus::FailedRetryable,
                "queue_unavailable",
                "the extraction queue is temporarily unavailable",
            )
            .await;
        tracing::warn!(
            error_class = "queue_unavailable",
            "policy import enqueue failed"
        );
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the policy extraction queue is temporarily unavailable",
        );
    }
    let location = format!("/v1/policy-imports/{}", import.id);
    (
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(PolicyImportView::from(import)),
    )
        .into_response()
}

pub async fn list_policy_imports(
    State(context): State<AppContext>,
    Query(query): Query<ImportListQuery>,
) -> Response {
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    let status: Option<PolicyImportStatus> =
        match query.status.as_deref().map(enum_from_form).transpose() {
            Ok(status) => status,
            Err(_) => {
                return problem(
                    StatusCode::BAD_REQUEST,
                    "invalid policy import status filter",
                );
            }
        };
    let cursor = match query.cursor.as_deref() {
        Some(cursor) => match parse_import_id(cursor) {
            Some(cursor) => Some(cursor),
            None => {
                return problem(StatusCode::BAD_REQUEST, "invalid policy import cursor");
            }
        },
        None => None,
    };
    let limit = query.limit.unwrap_or(25).min(100);
    let imports = match repository
        .list(crate::default_organization_id(), limit, status, cursor)
        .await
    {
        Ok(imports) => imports,
        Err(error) => return application_error(error),
    };
    let imports = imports
        .into_iter()
        .map(PolicyImportView::from)
        .collect::<Vec<_>>();
    Json(imports).into_response()
}

pub async fn get_policy_import(
    State(context): State<AppContext>,
    Path(id): Path<String>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    match repository.get(crate::default_organization_id(), id).await {
        Ok(Some(import)) => Json(PolicyImportView::from(import)).into_response(),
        Ok(None) => problem(StatusCode::NOT_FOUND, "policy import was not found"),
        Err(error) => application_error(error),
    }
}

pub async fn list_policy_candidates(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Query(query): Query<CandidateListQuery>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db);
    let status: Option<PolicyCandidateStatus> =
        match query.status.as_deref().map(enum_from_form).transpose() {
            Ok(status) => status,
            Err(_) => return problem(StatusCode::BAD_REQUEST, "invalid candidate status filter"),
        };
    match repository
        .list_candidates(crate::default_organization_id(), id)
        .await
    {
        Ok(candidates) => Json(
            candidates
                .into_iter()
                .skip_while(|candidate| {
                    query
                        .cursor
                        .as_deref()
                        .is_some_and(|cursor| candidate.id.to_string() != cursor)
                })
                .skip(usize::from(query.cursor.is_some()))
                .filter(|candidate| status.is_none_or(|status| candidate.status == status))
                .take(query.limit.unwrap_or(100).min(100))
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn review_candidate(
    State(context): State<AppContext>,
    Path((id, candidate_id)): Path<(String, String)>,
    Json(request): Json<ReviewCandidateRequest>,
) -> Response {
    let (Some(id), Some(candidate_id)) = (parse_import_id(&id), parse_candidate_id(&candidate_id))
    else {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid policy candidate identifier",
        );
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db);
    match review_policy_candidate(
        &repository,
        ReviewCandidateCommand {
            organization_id: crate::default_organization_id(),
            policy_import_id: id,
            candidate_id,
            decision: request.decision,
            reviewer_id: request.reviewer_id,
            notes: request.notes,
            expected_updated_at: request.expected_updated_at,
            edit: request.candidate,
        },
    )
    .await
    {
        Ok(candidate) => Json(candidate).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn add_manual_candidate(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<ManualCandidateRequest>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    if request.source_excerpt.trim().is_empty() {
        return problem(
            StatusCode::BAD_REQUEST,
            "manual candidate source excerpt is required",
        );
    }
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    let document = match normalized_document(&repository, &context, id).await {
        Ok((_, document)) => document,
        Err(response) => return response,
    };
    let grounded = document.segments.iter().any(|segment| {
        segment.text.contains(&request.source_excerpt)
            && request
                .locator
                .page
                .is_none_or(|page| segment.page == Some(page))
            && request
                .locator
                .section
                .as_ref()
                .is_none_or(|section| segment.section.as_ref() == Some(section))
    });
    if !grounded {
        return problem(
            StatusCode::BAD_REQUEST,
            "manual candidate excerpt and locator must match the immutable normalized source",
        );
    }
    match add_manual_policy_candidate(
        &repository,
        ManualCandidateCommand {
            organization_id: crate::default_organization_id(),
            policy_import_id: id,
            reviewer_id: request.reviewer_id,
            statement: request.statement,
            source_excerpt: request.source_excerpt,
            locator: request.locator,
            applicability: request.applicability,
            exceptions: request.exceptions,
            required_evidence: request.required_evidence,
            suggested_severity: request.suggested_severity,
            suggested_rule: request.suggested_rule,
        },
    )
    .await
    {
        Ok(candidate) => (StatusCode::CREATED, Json(candidate)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn verify_source(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<VerifySourceRequest>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    let verified = match request.decision.as_str() {
        "verified" => true,
        "rejected" => false,
        _ => {
            return problem(
                StatusCode::BAD_REQUEST,
                "decision must be verified or rejected",
            );
        }
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db);
    let command = VerifySourceCommand {
        organization_id: crate::default_organization_id(),
        policy_import_id: id,
        verified,
        reviewer_id: request.reviewer_id,
        notes: request.notes,
    };
    if command.reviewer_id.trim().is_empty()
        || command.reviewer_id.chars().count() > 320
        || command.notes.chars().count() > 4_000
    {
        return problem(
            StatusCode::BAD_REQUEST,
            "reviewer_id is required and reviewer fields must stay within their limits",
        );
    }
    if let Err(error) = repository
        .verify_source(&command, OffsetDateTime::now_utc())
        .await
    {
        return application_error(error);
    }
    match refresh_import_readiness(&repository, command.organization_id, id).await {
        Ok(import) => Json(PolicyImportView::from(import)).into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn retry_policy_import(
    State(context): State<AppContext>,
    Path(id): Path<String>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    let organization_id = crate::default_organization_id();
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    let import = match repository
        .transition(
            organization_id,
            id,
            PolicyImportStatus::FailedRetryable,
            PolicyImportStatus::Queued,
        )
        .await
    {
        Ok(import) => import,
        Err(error) => return application_error(error),
    };
    if let Err(_error) = ProcessPolicyImportWorker::perform_later(
        &context,
        ProcessPolicyImportArgs {
            organization_id,
            policy_import_id: id,
        },
    )
    .await
    {
        let _ = repository
            .mark_failure(
                organization_id,
                id,
                PolicyImportStatus::FailedRetryable,
                "queue_unavailable",
                "the extraction queue is temporarily unavailable",
            )
            .await;
        tracing::warn!(
            error_class = "queue_unavailable",
            "policy import retry enqueue failed"
        );
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "the policy extraction queue is temporarily unavailable",
        );
    }
    (StatusCode::ACCEPTED, Json(PolicyImportView::from(import))).into_response()
}

pub async fn compile_import(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<CompileImportRequest>,
) -> Response {
    let Some(id) = parse_import_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy import identifier");
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db);
    let already_compiled = match repository
        .get_compiled_pack(crate::default_organization_id(), id)
        .await
    {
        Ok(value) => value.is_some(),
        Err(error) => return application_error(error),
    };
    match compile_policy_import(
        &repository,
        CompilePolicyImportCommand {
            organization_id: crate::default_organization_id(),
            policy_import_id: id,
            key: request.key,
            version: request.version,
            title: request.title,
        },
    )
    .await
    {
        Ok(pack) => (
            if already_compiled {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            },
            Json(pack),
        )
            .into_response(),
        Err(error) => application_error(error),
    }
}

pub async fn source_context(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Query(query): Query<SourceContextQuery>,
) -> Response {
    let (Some(id), Some(candidate_id)) = (
        parse_import_id(&id),
        parse_candidate_id(&query.candidate_id),
    ) else {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid policy candidate identifier",
        );
    };
    let repository = SeaOrmPolicyImportRepository::new(context.db.clone());
    let candidate = match repository
        .get_candidate(crate::default_organization_id(), id, candidate_id)
        .await
    {
        Ok(Some(candidate)) => candidate,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "policy candidate was not found"),
        Err(error) => return application_error(error),
    };
    let document = match normalized_document(&repository, &context, id).await {
        Ok((_, document)) => document,
        Err(response) => return response,
    };
    let segment = document.segments.iter().find(|segment| {
        segment.text.contains(&candidate.source_excerpt)
            && candidate
                .locator
                .page
                .is_none_or(|page| segment.page == Some(page))
    });
    let Some(segment) = segment else {
        return problem(
            StatusCode::CONFLICT,
            "candidate excerpt is no longer grounded in the normalized source",
        );
    };
    Json(json!({
        "candidate_id": candidate.id,
        "format": document.format,
        "locator": candidate.locator,
        "excerpt": candidate.source_excerpt,
        "context": bounded_source_context(&segment.text, &candidate.source_excerpt, 4_000),
    }))
    .into_response()
}

async fn normalized_document(
    repository: &SeaOrmPolicyImportRepository,
    context: &AppContext,
    id: PolicyImportId,
) -> Result<(PolicyImport, ParsedDocument), Response> {
    let import = repository
        .get(crate::default_organization_id(), id)
        .await
        .map_err(application_error)?
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "policy import was not found"))?;
    let normalized_key = import.normalized_object_key.clone().ok_or_else(|| {
        problem(
            StatusCode::CONFLICT,
            "normalized source is not available yet",
        )
    })?;
    let artifacts = services(context).map_err(application_error)?.artifacts;
    let normalized = artifacts
        .get(&normalized_key)
        .await
        .map_err(application_error)?;
    let document = serde_json::from_slice(&normalized).map_err(|_| {
        problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "normalized source is invalid",
        )
    })?;
    Ok((import, document))
}

fn bounded_source_context(source: &str, excerpt: &str, max_characters: usize) -> String {
    let characters: Vec<char> = source.chars().collect();
    if characters.len() <= max_characters {
        return source.to_owned();
    }
    let excerpt_byte = source.find(excerpt).unwrap_or_default();
    let excerpt_character = source[..excerpt_byte].chars().count();
    let excerpt_length = excerpt.chars().count();
    let padding = max_characters.saturating_sub(excerpt_length) / 2;
    let start = excerpt_character.saturating_sub(padding);
    let end = (start + max_characters).min(characters.len());
    characters[start..end].iter().collect()
}

fn parse_import_id(value: &str) -> Option<PolicyImportId> {
    Uuid::parse_str(value).ok().map(PolicyImportId)
}

fn parse_candidate_id(value: &str) -> Option<PolicyCandidateId> {
    Uuid::parse_str(value).ok().map(PolicyCandidateId)
}

fn enum_from_form<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_value(Value::String(value.to_owned()))
}

fn sanitize_filename(filename: &str) -> String {
    let sanitized: String = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("policy-source")
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect();
    if sanitized.trim().is_empty() {
        "policy-source".to_owned()
    } else {
        sanitized
    }
}

fn application_error(error: governance_application::ApplicationError) -> Response {
    let status = match &error {
        governance_application::ApplicationError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        governance_application::ApplicationError::Conflict(_) => StatusCode::CONFLICT,
        governance_application::ApplicationError::NotFound(_) => StatusCode::NOT_FOUND,
        governance_application::ApplicationError::Forbidden(_) => StatusCode::FORBIDDEN,
        governance_application::ApplicationError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        governance_application::ApplicationError::Repository(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    problem(status, &error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_path_from_filename() {
        assert_eq!(
            sanitize_filename("../../refund-policy.txt"),
            "refund-policy.txt"
        );
        assert_eq!(sanitize_filename("\0\n"), "policy-source");
    }

    #[test]
    fn parses_closed_source_type() {
        let parsed: SourceType = enum_from_form("company_policy").expect("type should parse");
        assert_eq!(parsed, SourceType::CompanyPolicy);
        assert!(enum_from_form::<SourceType>("script").is_err());
    }

    #[test]
    fn source_context_is_bounded_and_keeps_excerpt() {
        let source = format!(
            "{}required approval{}",
            "a".repeat(5_000),
            "b".repeat(5_000)
        );
        let context = bounded_source_context(&source, "required approval", 1_000);
        assert!(context.chars().count() <= 1_000);
        assert!(context.contains("required approval"));
    }
}
