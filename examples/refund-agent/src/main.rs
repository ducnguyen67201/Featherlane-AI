use axum::{Json, Router, http::HeaderMap, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct Message {
    session_id: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    session_id: String,
    terminal: bool,
    message: String,
    synthetic_events: Vec<Value>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .json()
        .init();
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/messages", get(|| async { "ready" }).post(run));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8091").await?;
    tracing::info!(address = "0.0.0.0:8091", "reference refund agent listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn run(headers: HeaderMap, Json(message): Json<Message>) -> Json<AgentResponse> {
    let traceparent = headers
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    let amount = if message.message.contains("700") {
        700.0
    } else {
        75.0
    };
    let mut events = vec![json!({
        "event_type": "scenario_input",
        "name": "refund request",
        "input": {"message": message.message},
        "traceparent": traceparent
    })];
    if amount > 500.0 {
        events.push(json!({
            "event_type": "human_approval_decision",
            "name": "approval",
            "attributes": {"decision": "approved"}
        }));
    }
    events.extend([
        json!({"event_type": "tool_call", "name": "issue_refund", "input": {"amount": amount, "currency": "USD"}}),
        json!({"event_type": "final_output", "name": "refund completed", "attributes": {"terminal_state": "completed"}}),
    ]);
    Json(AgentResponse {
        session_id: message.session_id,
        terminal: true,
        message: format!("Refund of ${amount:.2} completed in the synthetic sandbox"),
        synthetic_events: events,
    })
}
