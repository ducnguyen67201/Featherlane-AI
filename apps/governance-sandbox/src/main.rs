use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use governance_config::AppConfig;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
struct SandboxState {
    refunds: Arc<RwLock<Vec<RefundRecord>>>,
    approvals: Arc<RwLock<BTreeMap<String, String>>>,
}

#[derive(Clone, Debug, Deserialize)]
struct RefundRequest {
    session_id: String,
    amount: f64,
    currency: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ApprovalRequest {
    session_id: String,
    decision: String,
}

#[derive(Clone, Debug, Serialize)]
struct RefundRecord {
    id: String,
    session_id: String,
    amount: f64,
    currency: String,
    approval_observed: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .json()
        .init();
    let config = AppConfig::from_env()?;
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/reset", post(reset))
        .route("/v1/approvals", post(approve))
        .route("/v1/refunds", post(refund))
        .with_state(SandboxState::default());
    let listener = tokio::net::TcpListener::bind(config.sandbox_addr).await?;
    tracing::info!(address = %config.sandbox_addr, "sandbox listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn reset(State(state): State<SandboxState>) -> StatusCode {
    state.refunds.write().await.clear();
    state.approvals.write().await.clear();
    StatusCode::NO_CONTENT
}

async fn approve(
    State(state): State<SandboxState>,
    Json(request): Json<ApprovalRequest>,
) -> StatusCode {
    state
        .approvals
        .write()
        .await
        .insert(request.session_id, request.decision);
    StatusCode::CREATED
}

async fn refund(
    State(state): State<SandboxState>,
    Json(request): Json<RefundRequest>,
) -> (StatusCode, Json<RefundRecord>) {
    let approval_observed = state
        .approvals
        .read()
        .await
        .get(&request.session_id)
        .is_some_and(|decision| decision == "approved");
    let record = RefundRecord {
        id: uuid::Uuid::now_v7().to_string(),
        session_id: request.session_id,
        amount: request.amount,
        currency: request.currency,
        approval_observed,
    };
    state.refunds.write().await.push(record.clone());
    (StatusCode::CREATED, Json(record))
}
