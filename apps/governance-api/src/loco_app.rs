use std::path::Path as FilePath;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use governance_application::{
    CancelEvaluationRun, CompleteEvaluationRun, CompletionRequest, CreateEvaluationRun,
    CreateEvaluationRunRequest, DashboardSnapshot, EvaluationRepository, EvaluationRunRepository,
    EvidenceBundleRepository, PolicyPackRepository, RotateTelemetryIngestKey,
    TelemetryIngestKeyRepository,
};
use governance_domain::{EvalRunId, PolicyPackApproval, PolicyPackId, PolicyPackStatusChange};
use governance_migration::Migrator;
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository,
};
use governance_worker::ProcessPolicyImportWorker;
use loco_rs::{
    Result,
    app::{AppContext, Hooks},
    bgworker::{BackgroundWorker, Queue},
    boot::{BootResult, StartMode, create_app},
    config::{Config, WorkerMode},
    controller::{AppRoutes, Routes},
    environment::Environment,
    prelude::{get, patch, post},
    task::Tasks,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApprovePolicyPackRequest, CompleteEvaluationRequest, CreateEvaluationRequest,
    CreatedEvaluationRun, EvaluationRunDetail, HealthResponse, PolicyImportRequest,
    PolicyPackLifecycleRequest, PolicyPackView, RotateTelemetryKeyRequest, TargetView,
};

#[derive(Clone, Debug)]
struct RuntimeEndpoints {
    otlp_http: String,
}

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

    async fn after_context(context: AppContext) -> Result<AppContext> {
        let policy_imports = crate::policy_imports::PolicyImportServices::from_env()
            .map_err(|error| loco_rs::Error::Message(error.to_string()))?;
        let otlp_http = match std::env::var("GOVERNANCE_OTLP_HTTP_ENDPOINT") {
            Ok(endpoint) if !endpoint.trim().is_empty() => endpoint,
            _ if matches!(
                context.environment,
                Environment::Development | Environment::Test
            ) =>
            {
                "http://localhost:4318/v1/traces".to_owned()
            }
            _ => {
                return Err(loco_rs::Error::Message(
                    "GOVERNANCE_OTLP_HTTP_ENDPOINT is required outside development".to_owned(),
                ));
            }
        };
        context.shared_store.insert(policy_imports);
        context.shared_store.insert(RuntimeEndpoints { otlp_http });
        Ok(context)
    }

    fn routes(_context: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes().add_route(api_routes())
    }

    async fn connect_workers(context: &AppContext, queue: &Queue) -> Result<()> {
        queue
            .register(ProcessPolicyImportWorker::build(context))
            .await?;
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
        .add("/v1/contracts/event-types", get(loco_event_types))
        .add("/v1/policy-packs", get(loco_policy_packs))
        .add("/v1/policy-packs", post(loco_import_policy_pack))
        .add("/v1/policy-packs/{id}", get(loco_policy_pack))
        .add(
            "/v1/policy-packs/{id}/approve",
            post(loco_approve_policy_pack),
        )
        .add(
            "/v1/policy-packs/{id}/disable",
            post(loco_disable_policy_pack),
        )
        .add(
            "/v1/policy-packs/{id}/enable",
            post(loco_enable_policy_pack),
        )
        .add(
            "/v1/policy-imports",
            get(crate::policy_imports::list_policy_imports),
        )
        .add(
            "/v1/policy-imports",
            post(crate::policy_imports::create_policy_import),
        )
        .add(
            "/v1/policy-imports/{id}",
            get(crate::policy_imports::get_policy_import),
        )
        .add(
            "/v1/policy-imports/{id}/candidates",
            get(crate::policy_imports::list_policy_candidates),
        )
        .add(
            "/v1/policy-imports/{id}/candidates",
            post(crate::policy_imports::add_manual_candidate),
        )
        .add(
            "/v1/policy-imports/{id}/candidates/{candidate_id}",
            patch(crate::policy_imports::review_candidate),
        )
        .add(
            "/v1/policy-imports/{id}/source-context",
            get(crate::policy_imports::source_context),
        )
        .add(
            "/v1/policy-imports/{id}/verify-source",
            post(crate::policy_imports::verify_source),
        )
        .add(
            "/v1/policy-imports/{id}/retry",
            post(crate::policy_imports::retry_policy_import),
        )
        .add(
            "/v1/policy-imports/{id}/compile",
            post(crate::policy_imports::compile_import),
        )
        .add("/v1/targets", get(loco_targets))
        .add("/v1/evaluations", get(loco_evaluations))
        .add("/v1/evaluations", post(loco_create_evaluation))
        .add("/v1/evaluations/{id}", get(loco_evaluation))
        .add(
            "/v1/evaluations/{id}/complete",
            post(loco_complete_evaluation),
        )
        .add("/v1/evaluations/{id}/cancel", post(loco_cancel_evaluation))
        .add(
            "/v1/targets/{id}/telemetry-key/rotate",
            post(loco_rotate_telemetry_key),
        )
        .add(
            "/v1/targets/{id}/telemetry-key/revoke",
            post(loco_revoke_telemetry_key),
        )
        .add("/v1/corpora/{set_name}", get(loco_corpus))
}

async fn loco_health(State(context): State<AppContext>) -> Json<HealthResponse> {
    let durable_worker = context.config.workers.mode == WorkerMode::BackgroundQueue
        && context.config.queue.is_some();
    Json(HealthResponse {
        status: if durable_worker { "ok" } else { "degraded" },
        service: "governance-api",
        version: env!("CARGO_PKG_VERSION"),
        policy_import_worker: if durable_worker {
            "durable_queue"
        } else {
            "not_durable"
        },
    })
}

async fn loco_event_types() -> Json<Vec<governance_domain::EventType>> {
    use governance_domain::EventType;

    Json(vec![
        EventType::ScenarioInput,
        EventType::AgentStart,
        EventType::ModelCall,
        EventType::ModelResult,
        EventType::ToolCall,
        EventType::ToolResult,
        EventType::Retrieval,
        EventType::Handoff,
        EventType::GuardrailDecision,
        EventType::HumanApprovalRequest,
        EventType::HumanApprovalDecision,
        EventType::FinalOutput,
        EventType::SideEffect,
        EventType::Retry,
        EventType::Error,
        EventType::Timeout,
        EventType::Cancellation,
        EventType::Unclassified,
    ])
}

async fn loco_overview(State(context): State<AppContext>) -> Response {
    let mut snapshot: DashboardSnapshot =
        super::overview(axum::extract::State(super::demo_state()))
            .await
            .0;
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    match repository.list(super::default_organization_id()).await {
        Ok(packs) => {
            snapshot.policy_packs = u32::try_from(packs.len()).unwrap_or(u32::MAX);
            Json(snapshot).into_response()
        }
        Err(error) => database_error(error),
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
        Err(error) => database_error(error),
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
        Err(error) => database_error(error),
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
        Err(error) => database_error(error),
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
        Err(error) => database_error(error),
    }
}

async fn loco_disable_policy_pack(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<PolicyPackLifecycleRequest>,
) -> Response {
    transition_policy_pack(context, id, request, false).await
}

async fn loco_enable_policy_pack(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<PolicyPackLifecycleRequest>,
) -> Response {
    transition_policy_pack(context, id, request, true).await
}

async fn transition_policy_pack(
    context: AppContext,
    id: String,
    request: PolicyPackLifecycleRequest,
    enable: bool,
) -> Response {
    let Some(id) = parse_policy_pack_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid policy pack identifier");
    };
    if request.actor_id.trim().is_empty() {
        return problem(StatusCode::BAD_REQUEST, "actor_id is required");
    }
    let change = PolicyPackStatusChange {
        actor_id: request.actor_id,
        notes: request.notes,
        changed_at: OffsetDateTime::now_utc(),
    };
    let repository = SeaOrmPolicyPackRepository::new(context.db);
    let result = if enable {
        repository
            .enable(super::default_organization_id(), id, &change)
            .await
    } else {
        repository
            .disable(super::default_organization_id(), id, &change)
            .await
    };
    match result {
        Ok(pack) => Json(pack).into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_targets() -> Json<Vec<TargetView>> {
    super::targets(axum::extract::State(super::demo_state())).await
}

async fn loco_evaluations(State(context): State<AppContext>) -> Response {
    let repository = SeaOrmEvaluationRunRepository::new(context.db);
    match repository.list_runs(super::default_organization_id()).await {
        Ok(runs) => Json(runs).into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_evaluation(State(context): State<AppContext>, Path(id): Path<String>) -> Response {
    let Some(id) = parse_eval_run_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid evaluation identifier");
    };
    let organization_id = super::default_organization_id();
    let runs = SeaOrmEvaluationRunRepository::new(context.db.clone());
    let run = match runs.get_run(organization_id, id).await {
        Ok(Some(run)) => run,
        Ok(None) => return problem(StatusCode::NOT_FOUND, "evaluation was not found"),
        Err(error) => return database_error(error),
    };
    let summaries = SeaOrmEvaluationRepository::new(context.db);
    let summary = match summaries.get_summary(organization_id, id).await {
        Ok(summary) => summary,
        Err(error) => return database_error(error),
    };
    let evidence = match runs.get_bundle(organization_id, id).await {
        Ok(evidence) => evidence,
        Err(error) => return database_error(error),
    };
    Json(EvaluationRunDetail {
        run,
        summary,
        evidence,
    })
    .into_response()
}

async fn loco_create_evaluation(
    State(context): State<AppContext>,
    Json(request): Json<CreateEvaluationRequest>,
) -> Response {
    let Some(endpoint) = context
        .shared_store
        .get::<RuntimeEndpoints>()
        .map(|endpoints| endpoints.otlp_http)
    else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "telemetry endpoint configuration was not initialized",
        );
    };
    let organization_id = super::default_organization_id();
    let policy_repository = SeaOrmPolicyPackRepository::new(context.db.clone());
    let runs = SeaOrmEvaluationRunRepository::new(context.db.clone());
    let command = CreateEvaluationRunRequest {
        organization_id,
        target_id: request.target_id,
        target_version: request.target_version,
        policy_pack_id: request.policy_pack_id,
        scenario_id: request.scenario_id,
        rule_ids: request.rule_ids,
        boundary_kind: request.boundary_kind,
        external_run_id: request.external_run_id,
        invocation_id: request.invocation_id,
        max_duration_seconds: request.max_duration_seconds,
    };
    match CreateEvaluationRun::new(policy_repository, runs.clone(), runs)
        .execute(command)
        .await
    {
        Ok(run) => (
            StatusCode::CREATED,
            Json(CreatedEvaluationRun::new(run, endpoint)),
        )
            .into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_complete_evaluation(
    State(context): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<CompleteEvaluationRequest>,
) -> Response {
    let Some(eval_run_id) = parse_eval_run_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid evaluation identifier");
    };
    let repository = SeaOrmEvaluationRunRepository::new(context.db);
    let command = CompletionRequest {
        organization_id: super::default_organization_id(),
        eval_run_id,
        reason: request.reason,
        terminal_state: request.terminal_state,
        settle_seconds: request.settle_seconds,
    };
    match CompleteEvaluationRun::new(repository.clone(), repository)
        .execute(command)
        .await
    {
        Ok(run) => (StatusCode::ACCEPTED, Json(run)).into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_cancel_evaluation(
    State(context): State<AppContext>,
    Path(id): Path<String>,
) -> Response {
    let Some(eval_run_id) = parse_eval_run_id(&id) else {
        return problem(StatusCode::BAD_REQUEST, "invalid evaluation identifier");
    };
    let repository = SeaOrmEvaluationRunRepository::new(context.db);
    match CancelEvaluationRun::new(repository)
        .execute(super::default_organization_id(), eval_run_id)
        .await
    {
        Ok(run) => Json(run).into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_rotate_telemetry_key(
    State(context): State<AppContext>,
    Path(target_id): Path<String>,
    Json(request): Json<RotateTelemetryKeyRequest>,
) -> Response {
    let repository = SeaOrmEvaluationRunRepository::new(context.db);
    match RotateTelemetryIngestKey::new(repository)
        .execute(
            super::default_organization_id(),
            target_id,
            request.expires_at,
        )
        .await
    {
        Ok(key) => (StatusCode::CREATED, Json(key)).into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_revoke_telemetry_key(
    State(context): State<AppContext>,
    Path(target_id): Path<String>,
) -> Response {
    let repository = SeaOrmEvaluationRunRepository::new(context.db);
    match repository
        .revoke_target_keys(
            super::default_organization_id(),
            &target_id,
            OffsetDateTime::now_utc(),
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => database_error(error),
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

#[allow(clippy::needless_pass_by_value)] // Keeps match arms usable as direct error adapters.
pub(crate) fn database_error(error: governance_application::ApplicationError) -> Response {
    let detail = error.to_string();
    let status = match error {
        governance_application::ApplicationError::NotFound(_) => StatusCode::NOT_FOUND,
        governance_application::ApplicationError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        governance_application::ApplicationError::Conflict(_) => StatusCode::CONFLICT,
        governance_application::ApplicationError::Forbidden(_) => StatusCode::FORBIDDEN,
        governance_application::ApplicationError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        governance_application::ApplicationError::Repository(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    problem(status, &detail)
}

pub(crate) fn problem(status: StatusCode, detail: &str) -> Response {
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
