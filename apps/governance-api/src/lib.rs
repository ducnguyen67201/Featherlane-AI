use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub mod loco_app;
mod policy_imports;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use governance_application::{ActivityPoint, DashboardSnapshot, RunListItem};
use governance_corpus::PINNED_SNAPSHOT;
use governance_domain::{
    CompletionReason, EvalRunId, EvaluationRun, EvaluationSummary, EventType, EvidenceBundle,
    InvocationId, OrganizationId, PolicyBundle, PolicyPack, PolicyPackId, RunBoundaryKind,
    RunVerdict, ScenarioId, SourceConfidence, TraceQualityStatus,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

pub use governance_policy::{ObligationImport, PolicyImportRequest, PolicySourceImport};

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
    pub trace_coverage: f64,
    pub last_evaluated: String,
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
    pub cost_usd: f64,
    pub created_at: String,
    pub trace_quality: TraceQualityStatus,
    pub findings: Vec<FindingView>,
    pub timeline: Vec<TimelineItem>,
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

#[derive(Clone, Debug, Deserialize)]
pub struct CreateEvaluationRequest {
    pub policy_pack_id: PolicyPackId,
    #[serde(default = "default_target")]
    pub target_id: String,
    #[serde(default = "default_target_version")]
    pub target_version: String,
    #[serde(default)]
    pub scenario_id: ScenarioId,
    #[serde(default)]
    pub rule_ids: Vec<String>,
    #[serde(default = "default_boundary_kind")]
    pub boundary_kind: RunBoundaryKind,
    #[serde(default)]
    pub external_run_id: Option<String>,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    #[serde(default = "default_max_duration_seconds")]
    pub max_duration_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CompleteEvaluationRequest {
    #[serde(default = "default_completion_reason")]
    pub reason: CompletionReason,
    pub terminal_state: Option<String>,
    #[serde(default = "default_settle_seconds")]
    pub settle_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvaluationRunDetail {
    pub run: EvaluationRun,
    pub summary: Option<EvaluationSummary>,
    pub evidence: Option<EvidenceBundle>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreatedEvaluationRun {
    #[serde(flatten)]
    pub run: EvaluationRun,
    pub telemetry: EvaluationTelemetryInstructions,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvaluationTelemetryInstructions {
    pub endpoint: String,
    pub protocol: &'static str,
    pub attributes: BTreeMap<String, String>,
}

impl CreatedEvaluationRun {
    pub fn new(run: EvaluationRun, endpoint: String) -> Self {
        let attributes = BTreeMap::from([
            ("featherlane.eval_run.id".to_owned(), run.id.to_string()),
            (
                "featherlane.invocation.id".to_owned(),
                run.primary_invocation_id.to_string(),
            ),
            (
                "featherlane.scenario.id".to_owned(),
                run.scenario_id.to_string(),
            ),
        ]);
        Self {
            run,
            telemetry: EvaluationTelemetryInstructions {
                endpoint,
                protocol: "otlp_http",
                attributes,
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RotateTelemetryKeyRequest {
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApprovePolicyPackRequest {
    pub reviewer_id: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PolicyPackLifecycleRequest {
    pub actor_id: String,
    #[serde(default)]
    pub notes: String,
}

fn default_target() -> String {
    "refund-agent-staging".to_owned()
}

fn default_target_version() -> String {
    "unversioned".to_owned()
}

const fn default_boundary_kind() -> RunBoundaryKind {
    RunBoundaryKind::ExplicitCi
}

const fn default_completion_reason() -> CompletionReason {
    CompletionReason::Explicit
}

const fn default_settle_seconds() -> u64 {
    10
}

const fn default_max_duration_seconds() -> u64 {
    3_600
}

#[derive(Clone, Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub policy_import_worker: &'static str,
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
            title: "Invalid policy import",
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
}

pub(crate) fn default_organization_id() -> OrganizationId {
    OrganizationId(Uuid::from_u128(1))
}

pub(crate) fn build_policy_bundle(
    organization_id: OrganizationId,
    request: &PolicyImportRequest,
) -> Result<PolicyBundle, ApiError> {
    governance_policy::build_policy_bundle(organization_id, request)
        .map_err(|error| ApiError::bad_request(error.to_string()))
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
        policy_import_worker: "not_checked",
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

fn demo_targets() -> Vec<TargetView> {
    vec![
        TargetView {
            id: "refund-agent-staging".to_owned(),
            name: "Refund Agent".to_owned(),
            version: "git:4e6a9c1".to_owned(),
            driver: "HTTP text".to_owned(),
            environment: "Staging".to_owned(),
            status: "healthy".to_owned(),
            trace_coverage: 98.4,
            last_evaluated: "4 minutes ago".to_owned(),
        },
        TargetView {
            id: "claims-workflow".to_owned(),
            name: "Claims Workflow".to_owned(),
            version: "git:9bd21f0".to_owned(),
            driver: "Webhook".to_owned(),
            environment: "Staging".to_owned(),
            status: "degraded".to_owned(),
            trace_coverage: 82.7,
            last_evaluated: "31 minutes ago".to_owned(),
        },
        TargetView {
            id: "support-triage".to_owned(),
            name: "Support Triage".to_owned(),
            version: "git:2c51aa4".to_owned(),
            driver: "HTTP text".to_owned(),
            environment: "Preview".to_owned(),
            status: "healthy".to_owned(),
            trace_coverage: 96.1,
            last_evaluated: "2 hours ago".to_owned(),
        },
    ]
}

fn demo_runs() -> Vec<EvaluationView> {
    [
        ("Refund Agent", RunVerdict::Pass, 14, 0, 0, 2_184, 0.21),
        (
            "Claims Workflow",
            RunVerdict::Inconclusive,
            9,
            0,
            3,
            5_821,
            0.46,
        ),
        ("Support Triage", RunVerdict::Fail, 11, 2, 0, 3_405, 0.29),
        ("Refund Agent", RunVerdict::Pass, 14, 0, 0, 2_361, 0.22),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (target, verdict, passed, failed, inconclusive, duration, cost))| EvaluationView {
            id: EvalRunId::new(),
            target: target.to_owned(),
            target_version: format!("git:demo-{index}"),
            policy_pack: "agent-operational-governance-v1".to_owned(),
            verdict,
            passed,
            failed,
            inconclusive,
            duration_ms: duration,
            cost_usd: cost,
            created_at: format!("2026-08-13T0{}:00:00Z", 4_usize.saturating_sub(index)),
            trace_quality: if verdict == RunVerdict::Inconclusive {
                TraceQualityStatus::Degraded
            } else {
                TraceQualityStatus::Complete
            },
            findings: vec![],
            timeline: vec![],
        },
    )
    .collect()
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
    use governance_domain::ReviewStatus;
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
