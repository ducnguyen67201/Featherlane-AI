use std::path::Path as FilePath;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use governance_application::{
    ApplicationError, DashboardSnapshot, LiveEvaluationRepository, PolicyPackRepository,
    RunListItem, RunTargetEvaluation, RunTargetEvaluationRequest, TargetRepository,
};
use governance_domain::{
    EvalRunId, PolicyPackApproval, PolicyPackId, RunVerdict, TargetId, TraceQualityStatus,
};
use governance_migration::Migrator;
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmPolicyPackRepository, SeaOrmTargetRepository,
};
use governance_targets::{
    CapabilityReport, DefaultDriverRegistry, DriverError, TargetDriverRegistry,
};
use governance_worker::EvaluationWorker;
use loco_rs::{
    Result,
    app::{AppContext, Hooks},
    bgworker::{BackgroundWorker, Queue},
    boot::{BootResult, StartMode, create_app},
    config::Config,
    controller::{AppRoutes, Routes},
    environment::Environment,
    prelude::{get, post},
    task::Tasks,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApprovePolicyPackRequest, CreateEvaluationRequest, CreateTargetRequest, HealthResponse,
    PolicyImportRequest, PolicyPackView,
};

#[derive(Debug)]
pub struct App;

#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        "featherlane-governance"
    }

    fn app_version() -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    fn routes(_context: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes().add_route(api_routes())
    }

    async fn connect_workers(context: &AppContext, queue: &Queue) -> Result<()> {
        queue.register(EvaluationWorker::build(context)).await?;
        Ok(())
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn truncate(_context: &AppContext) -> Result<()> {
        Ok(())
    }

    async fn seed(_context: &AppContext, _base: &FilePath) -> Result<()> {
        Ok(())
    }
}

fn api_routes() -> Routes {
    Routes::new()
        .add("/health", get(loco_health))
        .add("/v1/overview", get(loco_overview))
        .add("/v1/policy-packs", get(loco_policy_packs))
        .add("/v1/policy-packs", post(loco_import_policy_pack))
        .add("/v1/policy-packs/{id}", get(loco_policy_pack))
        .add(
            "/v1/policy-packs/{id}/approve",
            post(loco_approve_policy_pack),
        )
        .add("/v1/targets", get(loco_targets))
        .add("/v1/targets", post(loco_create_target))
        .add("/v1/targets/{id}", get(loco_target))
        .add("/v1/targets/{id}/validate", post(loco_validate_target))
        .add("/v1/evaluations", get(loco_evaluations))
        .add("/v1/evaluations", post(loco_create_evaluation))
        .add("/v1/evaluations/{id}", get(loco_evaluation))
        .add("/v1/corpora/{set_name}", get(loco_corpus))
}

async fn loco_health() -> Json<HealthResponse> {
    super::health().await
}

async fn loco_overview(State(context): State<AppContext>) -> Response {
    let organization_id = super::default_organization_id();
    let targets = SeaOrmTargetRepository::new(context.db.clone());
    let policies = SeaOrmPolicyPackRepository::new(context.db.clone());
    let evaluations = SeaOrmEvaluationRepository::new(context.db);
    let result = async {
        let targets = targets.list(organization_id).await?;
        let policies = policies.list(organization_id).await?;
        let runs = evaluations.list_runs(organization_id).await?;
        let completed = runs
            .iter()
            .filter(|run| run.evidence.trace_quality == TraceQualityStatus::Complete)
            .count();
        let passed = runs
            .iter()
            .filter(|run| run.summary.verdict == RunVerdict::Pass)
            .count();
        let recent_runs = runs
            .iter()
            .take(5)
            .map(|run| RunListItem {
                id: run.summary.eval_run_id,
                target: run.target_name.clone(),
                policy_pack: run.policy_pack_key.clone(),
                verdict: run.summary.verdict,
                passed: run.summary.passed,
                failed: run.summary.failed,
                inconclusive: run.summary.inconclusive,
                duration_ms: run.duration_ms(),
                created_at: super::rfc3339(run.created_at),
            })
            .collect();
        let count = runs.len();
        Ok::<_, ApplicationError>(DashboardSnapshot {
            active_agents: u32::try_from(targets.len()).unwrap_or(u32::MAX),
            policy_packs: u32::try_from(policies.len()).unwrap_or(u32::MAX),
            evaluations_30d: u32::try_from(count).unwrap_or(u32::MAX),
            pass_rate: percentage(passed, count),
            open_findings: u32::try_from(runs.iter().map(|run| run.summary.failed).sum::<usize>())
                .unwrap_or(u32::MAX),
            trace_coverage: percentage(completed, count),
            recent_runs,
            daily_activity: Vec::new(),
        })
    }
    .await;
    match result {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_policy_packs(State(context): State<AppContext>) -> Response {
    let organization_id = super::default_organization_id();
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    let result = async {
        let packs = repository.list(organization_id).await?;
        let counts = repository.source_counts(organization_id).await?;
        let reviewers = repository.latest_reviewers(organization_id).await?;
        Ok::<_, governance_application::ApplicationError>(
            packs
                .iter()
                .map(|pack| {
                    super::policy_pack_view(
                        pack,
                        counts.get(&pack.id).copied().unwrap_or_default(),
                        reviewers.get(&pack.id).map(String::as_str),
                    )
                })
                .collect::<Vec<PolicyPackView>>(),
        )
    }
    .await;
    match result {
        Ok(packs) => Json(packs).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_policy_pack(State(context): State<AppContext>, Path(id): Path<String>) -> Response {
    let Some(id) = parse_policy_pack_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy pack identifier");
    };
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    match repository.get(super::default_organization_id(), id).await {
        Ok(Some(pack)) => Json(pack).into_response(),
        Ok(None) => problem(StatusCode::NOT_FOUND, "policy pack was not found"),
        Err(error) => application_error(error),
    }
}

async fn loco_import_policy_pack(
    State(context): State<AppContext>,
    Json(request): Json<PolicyImportRequest>,
) -> Response {
    let bundle = match super::build_policy_bundle(super::default_organization_id(), &request) {
        Ok(bundle) => bundle,
        Err(error) => return error.into_response(),
    };
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    match repository.save_bundle(&bundle).await {
        Ok(()) => (StatusCode::CREATED, Json(bundle.pack)).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_approve_policy_pack(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<ApprovePolicyPackRequest>,
) -> Response {
    let Some(id) = parse_policy_pack_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy pack identifier");
    };
    if request.reviewer_id.trim().is_empty() {
        return problem(StatusCode::BAD_REQUEST, "reviewer_id is required");
    }
    let approval = PolicyPackApproval {
        reviewer_id: request.reviewer_id,
        notes: request.notes,
        approved_at: OffsetDateTime::now_utc(),
    };
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    match repository
        .approve(super::default_organization_id(), id, &approval)
        .await
    {
        Ok(pack) => Json(pack).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_targets(State(context): State<AppContext>) -> Response {
    let organization_id = super::default_organization_id();
    let targets = SeaOrmTargetRepository::new(context.db.clone());
    let evaluations = SeaOrmEvaluationRepository::new(context.db);
    let result = async {
        let items = targets.list(organization_id).await?;
        let latest = evaluations.latest_by_target(organization_id).await?;
        Ok::<_, ApplicationError>(
            items
                .iter()
                .map(|target| super::target_view(target, latest.get(&target.id)))
                .collect::<Vec<_>>(),
        )
    }
    .await;
    match result {
        Ok(targets) => Json(targets).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_create_target(
    State(context): State<AppContext>,
    Json(request): Json<CreateTargetRequest>,
) -> Response {
    let capability = CapabilityReport {
        target_id: request.key.clone(),
        reachable: false,
        reset_supported: request.reset_endpoint.is_some(),
        trace_context_supported: true,
        issues: Vec::new(),
        checked_at: OffsetDateTime::now_utc(),
    };
    let mut target =
        match super::build_registered_target(super::default_organization_id(), request, capability)
        {
            Ok(target) => target,
            Err(error) => return error.into_response(),
        };
    let drivers = DefaultDriverRegistry::default();
    target.capability = match drivers
        .driver_for(target.manifest.driver_type)
        .validate(&target.manifest)
        .await
    {
        Ok(report) => report,
        Err(error) => return driver_error(error),
    };
    let repository = SeaOrmTargetRepository::new(context.db);
    match repository.create(&target).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(super::target_detail_view(&target, None)),
        )
            .into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_target(State(context): State<AppContext>, Path(id): Path<String>) -> Response {
    let Some(id) = parse_target_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid target identifier");
    };
    let organization_id = super::default_organization_id();
    let targets = SeaOrmTargetRepository::new(context.db.clone());
    let evaluations = SeaOrmEvaluationRepository::new(context.db);
    let result = async {
        let target = targets
            .get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let latest = evaluations.latest_by_target(organization_id).await?;
        Ok::<_, ApplicationError>(super::target_detail_view(&target, latest.get(&id)))
    }
    .await;
    match result {
        Ok(target) => Json(target).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_validate_target(
    State(context): State<AppContext>,
    Path(id): Path<String>,
) -> Response {
    let Some(id) = parse_target_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid target identifier");
    };
    let organization_id = super::default_organization_id();
    let repository = SeaOrmTargetRepository::new(context.db);
    let target = match repository.get(organization_id, id).await {
        Ok(Some(target)) => target,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "target was not found"),
        Err(error) => return application_error(error),
    };
    let drivers = DefaultDriverRegistry::default();
    let report = match drivers
        .driver_for(target.manifest.driver_type)
        .validate(&target.manifest)
        .await
    {
        Ok(report) => report,
        Err(error) => return driver_error(error),
    };
    match repository
        .save_capability_report(organization_id, id, &report)
        .await
    {
        Ok(target) => Json(super::target_detail_view(&target, None)).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_evaluations(State(context): State<AppContext>) -> Response {
    let repository = SeaOrmEvaluationRepository::new(context.db);
    match repository.list_runs(super::default_organization_id()).await {
        Ok(runs) => {
            Json(runs.iter().map(super::evaluation_view).collect::<Vec<_>>()).into_response()
        }
        Err(error) => application_error(error),
    }
}

async fn loco_evaluation(State(context): State<AppContext>, Path(id): Path<String>) -> Response {
    let Some(id) = parse_eval_run_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid evaluation identifier");
    };
    let organization_id = super::default_organization_id();
    let evaluation_repository = SeaOrmEvaluationRepository::new(context.db);
    match evaluation_repository.get_run(organization_id, id).await {
        Ok(Some(run)) => Json(super::evaluation_view(&run)).into_response(),
        Ok(None) => problem(StatusCode::NOT_FOUND, "evaluation was not found"),
        Err(error) => application_error(error),
    }
}

async fn loco_create_evaluation(
    State(context): State<AppContext>,
    Json(request): Json<CreateEvaluationRequest>,
) -> Response {
    let organization_id = super::default_organization_id();
    let use_case = RunTargetEvaluation::new(
        SeaOrmTargetRepository::new(context.db.clone()),
        SeaOrmPolicyPackRepository::new(context.db.clone()),
        SeaOrmEvaluationRepository::new(context.db),
        DefaultDriverRegistry::default(),
    );
    match use_case
        .execute(
            organization_id,
            RunTargetEvaluationRequest {
                target_id: request.target_id,
                policy_pack_id: request.policy_pack_id,
                scenario: request.scenario,
            },
        )
        .await
    {
        Ok(run) => (StatusCode::CREATED, Json(super::evaluation_view(&run))).into_response(),
        Err(error) => application_error(error),
    }
}

async fn loco_corpus(Path(set_name): Path<String>) -> Response {
    match super::corpus(Path(set_name)).await {
        Ok(value) => value.into_response(),
        Err(error) => error.into_response(),
    }
}

fn parse_policy_pack_id(value: &str) -> Option<PolicyPackId> {
    Uuid::parse_str(value).ok().map(PolicyPackId)
}

fn parse_eval_run_id(value: &str) -> Option<EvalRunId> {
    Uuid::parse_str(value).ok().map(EvalRunId)
}

fn parse_target_id(value: &str) -> Option<TargetId> {
    Uuid::parse_str(value).ok().map(TargetId)
}

fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        let numerator = u32::try_from(numerator).unwrap_or(u32::MAX);
        let denominator = u32::try_from(denominator).unwrap_or(u32::MAX);
        100.0 * f64::from(numerator) / f64::from(denominator)
    }
}

#[allow(clippy::needless_pass_by_value)] // Keeps match arms usable as direct error adapters.
fn application_error(error: ApplicationError) -> Response {
    let detail = error.to_string();
    let status = match error {
        ApplicationError::NotFound(_) => StatusCode::NOT_FOUND,
        ApplicationError::InvalidRequest(_) => StatusCode::CONFLICT,
        ApplicationError::Forbidden(_) => StatusCode::FORBIDDEN,
        ApplicationError::Repository(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ApplicationError::TargetTransport(_) => StatusCode::BAD_GATEWAY,
        ApplicationError::TargetTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
        ApplicationError::TargetContract(_) => StatusCode::UNPROCESSABLE_ENTITY,
    };
    problem(status, &detail)
}

#[allow(clippy::needless_pass_by_value)] // Keeps direct driver error mapping concise.
fn driver_error(error: DriverError) -> Response {
    let status = match error {
        DriverError::Timeout => StatusCode::GATEWAY_TIMEOUT,
        DriverError::Transport | DriverError::Rejected(_) => StatusCode::BAD_GATEWAY,
        DriverError::UnsafeConfiguration(_)
        | DriverError::UnsupportedEvent
        | DriverError::ResponseTooLarge
        | DriverError::InvalidResponse
        | DriverError::Contract(_)
        | DriverError::MissingSecretReference(_) => StatusCode::UNPROCESSABLE_ENTITY,
    };
    problem(status, &error.to_string())
}

fn problem(status: StatusCode, detail: &str) -> Response {
    (
        status,
        Json(crate::ProblemDetails {
            problem_type: "https://featherlane.dev/problems/request-failed".to_owned(),
            title: status
                .canonical_reason()
                .unwrap_or("Request failed")
                .to_owned(),
            status: status.as_u16(),
            detail: detail.to_owned(),
        }),
    )
        .into_response()
}
