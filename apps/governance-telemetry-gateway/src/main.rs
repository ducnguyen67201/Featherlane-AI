use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use governance_config::AppConfig;
use governance_domain::{EvalRunId, InvocationId, OrganizationId, ScenarioId};
use governance_telemetry::{
    NormalizationContext, RedactionPolicy, TraceSpan, assess_trace_quality, normalize_spans,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
struct GatewayState {
    events: Arc<RwLock<Vec<governance_domain::NormalizedEvent>>>,
}

#[derive(Clone, Debug, Deserialize)]
struct TraceEnvelope {
    organization_id: OrganizationId,
    eval_run_id: EvalRunId,
    invocation_id: InvocationId,
    scenario_id: ScenarioId,
    spans: Vec<TraceSpan>,
}

#[derive(Clone, Debug, Serialize)]
struct IngestResponse {
    accepted: usize,
    trace_quality: governance_domain::TraceQualityStatus,
    defects: Vec<governance_domain::TraceDefect>,
}

#[derive(Clone, Debug, Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let config = AppConfig::from_env()?;
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/traces", post(ingest))
        .with_state(GatewayState::default());
    let listener = tokio::net::TcpListener::bind(config.gateway_addr).await?;
    tracing::info!(address = %config.gateway_addr, "telemetry gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "governance-telemetry-gateway",
    })
}

async fn ingest(
    State(state): State<GatewayState>,
    Json(envelope): Json<TraceEnvelope>,
) -> Json<IngestResponse> {
    let context = NormalizationContext {
        organization_id: envelope.organization_id,
        eval_run_id: envelope.eval_run_id,
        invocation_id: envelope.invocation_id,
        scenario_id: envelope.scenario_id,
    };
    let normalized = normalize_spans(context, envelope.spans, &RedactionPolicy::default());
    let (trace_quality, defects) = assess_trace_quality(&normalized);
    let accepted = normalized.len();
    state.events.write().await.extend(normalized);
    Json(IngestResponse {
        accepted,
        trace_quality,
        defects,
    })
}
