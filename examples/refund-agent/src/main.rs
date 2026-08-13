use axum::{Json, Router, http::HeaderMap, routing::get};
use governance_domain::{Actor, ActorType, EventType, ObservedEvent};
use governance_targets::TargetResponseEnvelope;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Message {
    session_id: String,
    message: String,
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
    let address = std::env::var("REFUND_AGENT_ADDR").unwrap_or_else(|_| "0.0.0.0:8091".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "reference refund agent listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn run(headers: HeaderMap, Json(message): Json<Message>) -> Json<TargetResponseEnvelope> {
    let traceparent = headers
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    let amount = if message.message.contains("700") {
        700.0
    } else {
        75.0
    };
    let actor = Actor {
        actor_type: ActorType::Agent,
        id: "refund-agent".to_owned(),
    };
    let mut events = Vec::new();
    if amount > 500.0 {
        events.push(ObservedEvent {
            event_type: EventType::HumanApprovalDecision,
            name: "approval".to_owned(),
            actor: Actor {
                actor_type: ActorType::Human,
                id: "test-approver".to_owned(),
            },
            input: serde_json::Value::Null,
            output: serde_json::Value::Null,
            attributes: std::collections::BTreeMap::from([(
                "decision".to_owned(),
                json!("approved"),
            )]),
        });
    }
    events.push(ObservedEvent {
        event_type: EventType::ToolCall,
        name: "issue_refund".to_owned(),
        actor: actor.clone(),
        input: json!({"amount": amount, "currency": "USD"}),
        output: serde_json::Value::Null,
        attributes: std::collections::BTreeMap::from([
            ("traceparent".to_owned(), json!(traceparent)),
            ("session_id".to_owned(), json!(message.session_id)),
        ]),
    });
    events.push(ObservedEvent {
        event_type: EventType::FinalOutput,
        name: "refund completed".to_owned(),
        actor,
        input: serde_json::Value::Null,
        output: json!({"message": "completed"}),
        attributes: std::collections::BTreeMap::from([(
            "terminal_state".to_owned(),
            json!("completed"),
        )]),
    });
    Json(TargetResponseEnvelope {
        schema_version: "1.0".to_owned(),
        terminal: true,
        terminal_state: Some("completed".to_owned()),
        output: json!({
            "message": format!("Refund of ${amount:.2} completed in the synthetic sandbox")
        }),
        events,
        side_effects: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn high_value_refund_returns_versioned_inline_evidence() {
        let Json(response) = run(
            HeaderMap::new(),
            Json(Message {
                session_id: "test-session".to_owned(),
                message: "Refund order test-456 for $700".to_owned(),
            }),
        )
        .await;
        let value = serde_json::to_value(response).expect("response should serialize");
        let parsed: TargetResponseEnvelope =
            serde_json::from_value(value).expect("response should match the shared contract");
        assert_eq!(parsed.schema_version, "1.0");
        assert!(parsed.terminal);
        assert_eq!(parsed.events.len(), 3);
    }
}
