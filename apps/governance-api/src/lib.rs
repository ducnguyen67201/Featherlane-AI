mod console_auth;
pub mod loco_app;
mod policy_collections;
mod policy_imports;
mod source_connections;

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use governance_application::{DashboardSnapshot, StoredEvaluationRun};
use governance_corpus::PINNED_SNAPSHOT;
use governance_domain::{
    CompletionReason, EvalRunId, EvaluationRun, EvaluationSummary, EventType, EvidenceBundle,
    InvocationId, NormalizedEvent, OrganizationId, PolicyBundle, PolicyPack, PolicyPackId,
    RunBoundaryKind, RunVerdict, ScenarioId, SourceConfidence, TargetId, TraceQualityStatus,
};
use governance_targets::{
    CapabilityReport, DriverType, EvidenceMode, RegisteredTarget, ScenarioDefinition,
    TargetEnvironment, TargetManifest, TelemetryBoundaryConfig, validate_registration,
    validate_telemetry_boundary,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

pub use governance_policy::{ObligationImport, PolicyImportRequest, PolicySourceImport};

#[derive(Clone, Debug, Serialize)]
pub struct TargetView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub driver: DriverType,
    pub environment: TargetEnvironment,
    pub status: String,
    pub issues: Vec<String>,
    pub checked_at: String,
    pub latest_trace_quality: Option<TraceQualityStatus>,
    pub last_evaluated: Option<String>,
    pub auto_evaluation_enabled: bool,
    pub automatic_boundary_kind: Option<RunBoundaryKind>,
    pub default_policy_pack_id: Option<PolicyPackId>,
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
#[serde(untagged)]
pub enum CreateEvaluationRequest {
    Live(CreateLiveEvaluationRequest),
    Correlated(CreateCorrelatedEvaluationRequest),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateLiveEvaluationRequest {
    pub target_id: TargetId,
    pub policy_pack_id: PolicyPackId,
    pub scenario: ScenarioDefinition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateCorrelatedEvaluationRequest {
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
    pub status_endpoint: Option<String>,
    #[serde(default)]
    pub terminal_response_key: Option<String>,
    #[serde(default)]
    pub auth_secret_ref: Option<String>,
    #[serde(default = "default_target_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub otlp_required: bool,
    #[serde(default)]
    pub telemetry_boundary: governance_targets::TelemetryBoundaryConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConfigureTelemetryBoundaryRequest {
    pub telemetry_boundary: TelemetryBoundaryConfig,
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

fn default_target_timeout() -> u64 {
    30
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
    pub source_connectors: SourceConnectorAvailability,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceConnectorAvailability {
    pub google_drive: &'static str,
    pub microsoft_graph: &'static str,
    pub notion: &'static str,
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
        target_version: request.version.trim().to_owned(),
        driver_type: request.driver_type,
        endpoint: request.endpoint,
        reset_endpoint: request.reset_endpoint,
        status_endpoint: request.status_endpoint,
        terminal_response_key: request.terminal_response_key,
        auth_secret_ref: request.auth_secret_ref,
        timeout_seconds: request.timeout_seconds,
        evidence_mode: EvidenceMode::Inline,
        otlp_required: request.otlp_required,
        production_credentials_allowed: false,
        telemetry_boundary: request.telemetry_boundary,
    };
    validate_registration(&request.name, &manifest)
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
    let telemetry_boundary = &target.manifest.telemetry_boundary;
    let auto_evaluation_enabled = telemetry_boundary.default_policy_pack_id.is_some()
        && validate_telemetry_boundary(telemetry_boundary).is_ok();
    TargetView {
        id: target.id.to_string(),
        name: target.name.clone(),
        version: target.manifest.target_version.clone(),
        driver: target.manifest.driver_type,
        environment: target.environment,
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
        auto_evaluation_enabled,
        automatic_boundary_kind: auto_evaluation_enabled
            .then_some(telemetry_boundary.boundary_kind),
        default_policy_pack_id: telemetry_boundary.default_policy_pack_id,
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
pub fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/overview", get(overview))
        .route("/v1/targets", get(targets))
        .route("/v1/evaluations", get(evaluations))
        .route("/v1/evaluations/{id}", get(evaluation))
        .route("/v1/corpora/{set_name}", get(corpus))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        )
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "governance-api",
        version: env!("CARGO_PKG_VERSION"),
        policy_import_worker: "not_checked",
        source_connectors: SourceConnectorAvailability {
            google_drive: "unavailable",
            microsoft_graph: "unavailable",
            notion: "unavailable",
        },
    })
}

pub(crate) async fn overview() -> Json<DashboardSnapshot> {
    Json(DashboardSnapshot {
        active_agents: 0,
        policy_packs: 0,
        evaluations_30d: 0,
        pass_rate: 0.0,
        open_findings: 0,
        trace_coverage: 0.0,
        recent_runs: Vec::new(),
        daily_activity: Vec::new(),
    })
}

pub(crate) async fn targets() -> Json<Vec<TargetView>> {
    Json(Vec::new())
}

pub(crate) async fn evaluations() -> Json<Vec<EvaluationView>> {
    Json(Vec::new())
}

pub(crate) async fn evaluation(Path(id): Path<String>) -> Result<Json<EvaluationView>, ApiError> {
    Err(ApiError::not_found(&format!("evaluation {id}")))
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

#[cfg(test)]
mod tests;
use std::collections::BTreeMap;
