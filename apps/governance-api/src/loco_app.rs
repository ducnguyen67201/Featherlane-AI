use std::path::Path as FilePath;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use governance_application::{DashboardSnapshot, EvaluateEvidence, PolicyPackRepository};
use governance_domain::{PolicyPackApproval, PolicyPackId, ReviewStatus};
use governance_migration::Migrator;
use governance_persistence::{SeaOrmEvaluationRepository, SeaOrmPolicyPackRepository};
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
    ApprovePolicyPackRequest, CorpusView, CreateEvaluationRequest, EvaluationView, HealthResponse,
    PolicyImportRequest, PolicyPackView, TargetView,
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
        .add("/v1/evaluations", get(loco_evaluations))
        .add("/v1/evaluations", post(loco_create_evaluation))
        .add("/v1/evaluations/{id}", get(loco_evaluation))
        .add("/v1/corpus/open-us-law", get(loco_corpus))
}

async fn loco_health() -> Json<HealthResponse> {
    super::health().await
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

async fn loco_targets() -> Json<Vec<TargetView>> {
    super::targets(axum::extract::State(super::demo_state())).await
}

async fn loco_evaluations() -> Json<Vec<EvaluationView>> {
    super::evaluations(axum::extract::State(super::demo_state())).await
}

async fn loco_evaluation(Path(id): Path<String>) -> Response {
    match super::evaluation(axum::extract::State(super::demo_state()), Path(id)).await {
        Ok(value) => value.into_response(),
        Err(error) => error.into_response(),
    }
}

async fn loco_create_evaluation(
    State(context): State<AppContext>,
    Json(request): Json<CreateEvaluationRequest>,
) -> Response {
    let organization_id = super::default_organization_id();
    let policy_repository = SeaOrmPolicyPackRepository::new(context.db.clone());
    let pack = if let Some(id) = request.policy_pack_id {
        policy_repository.get(organization_id, id).await
    } else {
        policy_repository.list(organization_id).await.map(|packs| {
            packs
                .into_iter()
                .find(|pack| pack.status == ReviewStatus::Approved)
        })
    };
    let pack = match pack {
        Ok(Some(pack)) => pack,
        Ok(None) => {
            return problem(
                StatusCode::PRECONDITION_REQUIRED,
                "no approved database policy pack is available",
            );
        }
        Err(error) => return database_error(error),
    };
    let evidence = super::demo_evidence(
        organization_id,
        request.simulate_missing_approval,
        request.simulate_missing_trace,
    );
    let evaluation_repository = SeaOrmEvaluationRepository::new(context.db);
    match EvaluateEvidence::new(policy_repository, evaluation_repository)
        .execute(organization_id, pack.id, &evidence)
        .await
    {
        Ok(summary) => (
            StatusCode::CREATED,
            Json(super::evaluation_view(request, &pack, &evidence, &summary)),
        )
            .into_response(),
        Err(error) => database_error(error),
    }
}

async fn loco_corpus() -> (StatusCode, Json<CorpusView>) {
    (StatusCode::OK, super::corpus().await)
}

fn parse_policy_pack_id(value: &str) -> Option<PolicyPackId> {
    Uuid::parse_str(value).ok().map(PolicyPackId)
}

#[allow(clippy::needless_pass_by_value)] // Keeps match arms usable as direct error adapters.
fn database_error(error: governance_application::ApplicationError) -> Response {
    let detail = error.to_string();
    let status = match error {
        governance_application::ApplicationError::NotFound(_) => StatusCode::NOT_FOUND,
        governance_application::ApplicationError::InvalidRequest(_) => StatusCode::CONFLICT,
        governance_application::ApplicationError::Forbidden(_) => StatusCode::FORBIDDEN,
        governance_application::ApplicationError::Repository(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    problem(status, &detail)
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
