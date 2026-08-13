use std::sync::{Arc, OnceLock};

pub mod loco_app;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use governance_application::{ActivityPoint, DashboardSnapshot, RunListItem, StoredEvaluationRun};
use governance_corpus::PINNED_SNAPSHOT;
use governance_domain::{
    CompiledRule, EvalRunId, EventType, NormalizedEvent, Obligation, ObligationId, OrganizationId,
    PolicyBundle, PolicyPack, PolicyPackId, ReviewStatus, ReviewerApproval, RunVerdict, Source,
    SourceConfidence, SourceId, SourceLocator, SourceType, TargetId, TraceQualityStatus,
};
use governance_policy::{PolicyDocument, compile_policy_document};
use governance_targets::{
    CapabilityReport, DriverType, EvidenceMode, RegisteredTarget, ScenarioDefinition,
    TargetEnvironment, TargetManifest, validate_registration,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct AppState {
    store: Arc<RwLock<DemoStore>>,
}

#[derive(Clone, Debug)]
struct DemoStore {
    targets: Vec<TargetView>,
    runs: Vec<EvaluationView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub driver: String,
    pub environment: String,
    pub status: String,
    pub issues: Vec<String>,
    pub checked_at: String,
    pub latest_trace_quality: Option<TraceQualityStatus>,
    pub last_evaluated: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetDetailView {
    #[serde(flatten)]
    pub summary: TargetView,
    pub manifest: TargetManifest,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvaluationView {
    pub id: EvalRunId,
    pub target: String,
    pub target_version: String,
    pub policy_pack: String,
    pub verdict: RunVerdict,
    pub passed: usize,
    pub failed: usize,
    pub inconclusive: usize,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
    pub created_at: String,
    pub trace_quality: TraceQualityStatus,
    pub findings: Vec<FindingView>,
    pub timeline: Vec<TimelineItem>,
    pub summary: governance_domain::EvaluationSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindingView {
    pub rule_id: String,
    pub severity: String,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimelineItem {
    pub sequence: u64,
    pub event_type: EventType,
    pub name: String,
    pub actor: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PolicyPackView {
    pub id: String,
    pub key: String,
    pub title: String,
    pub version: u32,
    pub status: String,
    pub rules: usize,
    pub source_count: usize,
    pub reviewer: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CorpusView {
    pub set_name: String,
    pub dataset: String,
    pub snapshot: String,
    pub snapshot_date: String,
    pub files: u32,
    pub total_bytes: u64,
    pub license: String,
    pub imported_jurisdictions: Vec<JurisdictionView>,
    pub attribution: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct JurisdictionView {
    pub code: String,
    pub corpus_type: String,
    pub sections: u64,
    pub confidence: SourceConfidence,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateEvaluationRequest {
    pub target_id: TargetId,
    pub policy_pack_id: PolicyPackId,
    pub scenario: ScenarioDefinition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateTargetRequest {
    pub name: String,
    pub key: String,
    pub version: String,
    pub environment: TargetEnvironment,
    pub driver_type: DriverType,
    pub endpoint: String,
    #[serde(default)]
    pub reset_endpoint: Option<String>,
    #[serde(default)]
    pub auth_secret_ref: Option<String>,
    #[serde(default = "default_target_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PolicyImportRequest {
    pub key: String,
    pub version: u32,
    pub title: String,
    pub rules: Vec<CompiledRule>,
    pub sources: Vec<PolicySourceImport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PolicySourceImport {
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    pub content_sha256: String,
    pub confidence: SourceConfidence,
    pub obligations: Vec<ObligationImport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObligationImport {
    pub key: String,
    pub statement: String,
    pub locator: SourceLocator,
    #[serde(default)]
    pub applicability: Value,
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    pub reviewer_id: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub reviewed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApprovePolicyPackRequest {
    pub reviewer_id: String,
    #[serde(default)]
    pub notes: String,
}

fn default_target_timeout() -> u64 {
    30
}

#[derive(Clone, Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    title: &'static str,
    detail: String,
}

impl ApiError {
    fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            title: "Invalid request",
            detail: detail.into(),
        }
    }

    fn not_found(resource: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            title: "Resource not found",
            detail: resource.to_owned(),
        }
    }

    fn internal(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            title: "Internal error",
            detail: detail.into(),
        }
    }
}

pub(crate) fn default_organization_id() -> OrganizationId {
    OrganizationId(Uuid::from_u128(1))
}

pub(crate) fn build_registered_target(
    organization_id: OrganizationId,
    request: CreateTargetRequest,
    capability: CapabilityReport,
) -> Result<RegisteredTarget, ApiError> {
    let manifest = TargetManifest {
        schema_version: "1.0".to_owned(),
        target_id: request.key,
        target_version: request.version,
        driver_type: request.driver_type,
        endpoint: request.endpoint,
        reset_endpoint: request.reset_endpoint,
        auth_secret_ref: request.auth_secret_ref,
        timeout_seconds: request.timeout_seconds,
        evidence_mode: EvidenceMode::Inline,
        production_credentials_allowed: false,
    };
    validate_registration(&request.name, request.environment, &manifest)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(RegisteredTarget {
        id: TargetId::new(),
        organization_id,
        name: request.name.trim().to_owned(),
        environment: request.environment,
        manifest,
        capability,
        created_at: OffsetDateTime::now_utc(),
    })
}

pub(crate) fn target_view(
    target: &RegisteredTarget,
    latest: Option<&StoredEvaluationRun>,
) -> TargetView {
    TargetView {
        id: target.id.to_string(),
        name: target.name.clone(),
        version: target.manifest.target_version.clone(),
        driver: enum_label(target.manifest.driver_type),
        environment: enum_label(target.environment),
        status: if target.capability.reachable {
            "healthy"
        } else {
            "degraded"
        }
        .to_owned(),
        issues: target.capability.issues.clone(),
        checked_at: rfc3339(target.capability.checked_at),
        latest_trace_quality: latest.map(|run| run.evidence.trace_quality),
        last_evaluated: latest.map(|run| rfc3339(run.completed_at)),
    }
}

pub(crate) fn target_detail_view(
    target: &RegisteredTarget,
    latest: Option<&StoredEvaluationRun>,
) -> TargetDetailView {
    TargetDetailView {
        summary: target_view(target, latest),
        manifest: target.manifest.clone(),
    }
}

fn enum_label<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

pub(crate) fn build_policy_bundle(
    organization_id: OrganizationId,
    request: &PolicyImportRequest,
) -> Result<PolicyBundle, ApiError> {
    if request.key.trim().is_empty() || request.rules.is_empty() || request.sources.is_empty() {
        return Err(ApiError::bad_request(
            "key, at least one rule, and at least one source are required",
        ));
    }
    let canonical =
        serde_json::to_vec(request).map_err(|error| ApiError::internal(error.to_string()))?;
    let pack = compile_policy_document(
        organization_id,
        PolicyDocument {
            key: request.key.clone(),
            version: request.version,
            title: request.title.clone(),
            status: ReviewStatus::Draft,
            rules: request.rules.clone(),
        },
        format!("{:x}", Sha256::digest(canonical)),
    )
    .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let mut sources = Vec::with_capacity(request.sources.len());
    let mut obligations = Vec::new();
    for imported_source in &request.sources {
        if !valid_sha256(&imported_source.content_sha256) {
            return Err(ApiError::bad_request(
                "every source content_sha256 must be a 64-character hexadecimal digest",
            ));
        }
        let source_id = SourceId::new();
        sources.push(Source {
            id: source_id,
            organization_id,
            source_type: imported_source.source_type,
            title: imported_source.title.clone(),
            jurisdiction: imported_source.jurisdiction.clone(),
            effective_from: None,
            content_sha256: imported_source.content_sha256.clone(),
            confidence: imported_source.confidence,
        });
        for imported_obligation in &imported_source.obligations {
            if !valid_sha256(&imported_obligation.locator.excerpt_sha256) {
                return Err(ApiError::bad_request(
                    "every obligation excerpt_sha256 must be a 64-character hexadecimal digest",
                ));
            }
            let review = match (
                imported_obligation.reviewer_id.as_ref(),
                imported_obligation.reviewed_at,
            ) {
                (Some(reviewer_id), Some(reviewed_at)) => Some(ReviewerApproval {
                    status: ReviewStatus::Approved,
                    reviewer_id: reviewer_id.clone(),
                    reviewed_at,
                }),
                (None, None) => None,
                _ => {
                    return Err(ApiError::bad_request(
                        "obligation reviewer_id and reviewed_at must be supplied together",
                    ));
                }
            };
            obligations.push(Obligation {
                id: ObligationId::new(),
                organization_id,
                source_id,
                key: imported_obligation.key.clone(),
                statement: imported_obligation.statement.clone(),
                locator: imported_obligation.locator.clone(),
                applicability: imported_obligation.applicability.clone(),
                exceptions: imported_obligation.exceptions.clone(),
                required_evidence: imported_obligation.required_evidence.clone(),
                review,
            });
        }
    }
    let obligation_keys: std::collections::BTreeSet<&str> =
        obligations.iter().map(|item| item.key.as_str()).collect();
    if let Some(rule) = request
        .rules
        .iter()
        .find(|rule| !obligation_keys.contains(rule.obligation_key.as_str()))
    {
        return Err(ApiError::bad_request(format!(
            "rule {} references missing obligation {}",
            rule.id, rule.obligation_key
        )));
    }
    Ok(PolicyBundle {
        pack,
        sources,
        obligations,
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn policy_pack_view(
    pack: &PolicyPack,
    source_count: usize,
    reviewer: Option<&str>,
) -> PolicyPackView {
    PolicyPackView {
        id: pack.id.to_string(),
        key: pack.key.clone(),
        title: pack.title.clone(),
        version: pack.version,
        status: format!("{:?}", pack.status).to_ascii_lowercase(),
        rules: pack.rules.len(),
        source_count,
        reviewer: reviewer.unwrap_or("Awaiting approval").to_owned(),
    }
}

pub(crate) fn evaluation_view(run: &StoredEvaluationRun) -> EvaluationView {
    EvaluationView {
        id: run.summary.eval_run_id,
        target: run.target_name.clone(),
        target_version: run.target_version.clone(),
        policy_pack: run.policy_pack_key.clone(),
        verdict: run.summary.verdict,
        passed: run.summary.passed,
        failed: run.summary.failed,
        inconclusive: run.summary.inconclusive,
        duration_ms: run.duration_ms(),
        cost_usd: None,
        created_at: rfc3339(run.created_at),
        trace_quality: run.evidence.trace_quality,
        findings: run
            .summary
            .results
            .iter()
            .map(|result| FindingView {
                rule_id: result.rule_id.clone(),
                severity: format!("{:?}", result.severity).to_ascii_lowercase(),
                status: format!("{:?}", result.status).to_ascii_lowercase(),
                message: result.message.clone(),
            })
            .collect(),
        timeline: run.evidence.events.iter().map(timeline_item).collect(),
        summary: run.summary.clone(),
    }
}

pub(crate) fn rfc3339(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ProblemDetails {
            problem_type: "https://featherlane.dev/problems/request-failed".to_owned(),
            title: self.title.to_owned(),
            status: self.status.as_u16(),
            detail: self.detail,
        };
        (self.status, Json(body)).into_response()
    }
}

/// Builds the in-memory router used by integration tests and local embedding.
///
/// # Errors
///
/// Returns an error if application state cannot be constructed.
pub fn app() -> Result<Router, ApiError> {
    let state = demo_state();

    Ok(Router::new()
        .route("/health", get(health))
        .route("/v1/overview", get(overview))
        .route("/v1/targets", get(targets))
        .route("/v1/evaluations", get(evaluations))
        .route("/v1/evaluations/{id}", get(evaluation))
        .route("/v1/corpora/{set_name}", get(corpus))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        ))
}

pub(crate) fn demo_state() -> AppState {
    static STATE: OnceLock<AppState> = OnceLock::new();
    STATE.get_or_init(build_demo_state).clone()
}

fn build_demo_state() -> AppState {
    let store = DemoStore {
        targets: demo_targets(),
        runs: demo_runs(),
    };
    AppState {
        store: Arc::new(RwLock::new(store)),
    }
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "governance-api",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub(crate) async fn overview(State(state): State<AppState>) -> Json<DashboardSnapshot> {
    let store = state.store.read().await;
    let recent_runs = store
        .runs
        .iter()
        .take(5)
        .map(|run| RunListItem {
            id: run.id,
            target: run.target.clone(),
            policy_pack: run.policy_pack.clone(),
            verdict: run.verdict,
            passed: run.passed,
            failed: run.failed,
            inconclusive: run.inconclusive,
            duration_ms: run.duration_ms,
            created_at: run.created_at.clone(),
        })
        .collect();
    Json(DashboardSnapshot {
        active_agents: u32::try_from(store.targets.len()).unwrap_or(u32::MAX),
        policy_packs: 0,
        evaluations_30d: 184,
        pass_rate: 96.8,
        open_findings: 7,
        trace_coverage: 94.2,
        recent_runs,
        daily_activity: demo_activity(),
    })
}

pub(crate) async fn targets(State(state): State<AppState>) -> Json<Vec<TargetView>> {
    Json(state.store.read().await.targets.clone())
}

pub(crate) async fn evaluations(State(state): State<AppState>) -> Json<Vec<EvaluationView>> {
    Json(state.store.read().await.runs.clone())
}

pub(crate) async fn evaluation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EvaluationView>, ApiError> {
    let store = state.store.read().await;
    store
        .runs
        .iter()
        .find(|run| run.id.to_string() == id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found(&format!("evaluation {id}")))
}

pub(crate) async fn corpus(Path(set_name): Path<String>) -> Result<Json<CorpusView>, ApiError> {
    corpus_catalog()
        .into_iter()
        .find(|corpus| corpus.set_name == set_name)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(&format!("corpus {set_name}")))
}

fn corpus_catalog() -> Vec<CorpusView> {
    vec![CorpusView {
        set_name: "open-us-law".to_owned(),
        dataset: "Open US Law".to_owned(),
        snapshot: PINNED_SNAPSHOT.to_owned(),
        snapshot_date: "2026-07-21".to_owned(),
        files: 105,
        total_bytes: 1_169_714_039,
        license: "CC BY 4.0".to_owned(),
        imported_jurisdictions: vec![
            JurisdictionView {
                code: "US-CA".to_owned(),
                corpus_type: "statutes".to_owned(),
                sections: 98_664,
                confidence: SourceConfidence::SnapshotOfficialProvenance,
                status: "verification_required".to_owned(),
            },
            JurisdictionView {
                code: "US-FED".to_owned(),
                corpus_type: "statutes".to_owned(),
                sections: 54_853,
                confidence: SourceConfidence::OfficialVerified,
                status: "verified".to_owned(),
            },
            JurisdictionView {
                code: "US-GA".to_owned(),
                corpus_type: "statutes".to_owned(),
                sections: 28_154,
                confidence: SourceConfidence::Quarantined,
                status: "blocked".to_owned(),
            },
        ],
        attribution: "Structured US primary-law data from the Open US Law corpus by Vaquill AI, used under CC BY 4.0.".to_owned(),
    }]
}

pub(crate) fn timeline_item(event: &NormalizedEvent) -> TimelineItem {
    TimelineItem {
        sequence: event.sequence,
        event_type: event.event_type,
        name: event.name.clone(),
        actor: event.actor.id.clone(),
        outcome: if event.event_type == EventType::Error {
            "error"
        } else {
            "observed"
        }
        .to_owned(),
    }
}

fn demo_targets() -> Vec<TargetView> {
    Vec::new()
}

fn demo_runs() -> Vec<EvaluationView> {
    Vec::new()
}

fn demo_activity() -> Vec<ActivityPoint> {
    [
        ("Aug 7", 18, 1, 2),
        ("Aug 8", 22, 0, 1),
        ("Aug 9", 17, 2, 0),
        ("Aug 10", 28, 1, 3),
        ("Aug 11", 31, 0, 1),
        ("Aug 12", 26, 2, 2),
        ("Aug 13", 34, 1, 1),
    ]
    .into_iter()
    .map(|(day, passed, failed, inconclusive)| ActivityPoint {
        day: day.to_owned(),
        passed,
        failed,
        inconclusive,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn health_route_is_available() {
        let response = app()
            .expect("router should build")
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_evaluation_returns_problem_response() {
        let response = app()
            .expect("router should build")
            .oneshot(
                Request::builder()
                    .uri("/v1/evaluations/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn corpus_is_resolved_by_set_name() {
        let response = app()
            .expect("router should build")
            .oneshot(
                Request::builder()
                    .uri("/v1/corpora/open-us-law")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_corpus_set_returns_not_found() {
        let response = app()
            .expect("router should build")
            .oneshot(
                Request::builder()
                    .uri("/v1/corpora/not-imported")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn import_fixture_builds_one_draft_database_aggregate() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/policies/refund-governance.import.json");
        let input = fs::read_to_string(path).expect("policy fixture should be readable");
        let request: PolicyImportRequest =
            serde_json::from_str(&input).expect("policy fixture should match the import contract");
        let bundle = build_policy_bundle(default_organization_id(), &request)
            .expect("policy aggregate should compile");

        assert_eq!(bundle.pack.status, ReviewStatus::Draft);
        assert_eq!(bundle.pack.rules.len(), 1);
        assert_eq!(bundle.sources.len(), 1);
        assert_eq!(bundle.obligations.len(), 1);
        assert_eq!(bundle.pack.content_sha256.len(), 64);
    }

    #[test]
    fn import_rejects_a_rule_without_a_persisted_obligation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/policies/refund-governance.import.json");
        let input = fs::read_to_string(path).expect("policy fixture should be readable");
        let mut request: PolicyImportRequest =
            serde_json::from_str(&input).expect("policy fixture should match the import contract");
        request.rules[0].obligation_key = "MISSING".to_owned();

        assert!(build_policy_bundle(default_organization_id(), &request).is_err());
    }
}
