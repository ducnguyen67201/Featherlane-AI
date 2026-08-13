use governance_domain::{EvalRunId, InvocationId, ObservedEvent, ScenarioId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

pub const SCENARIO_SCHEMA_VERSION: &str = "1.0";
pub const MAX_SCENARIO_EVENTS: usize = 50;
pub const MAX_OBSERVATIONS: usize = 1_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TestEvent {
    UserText { text: String },
    Webhook { payload: Value },
    HumanDecision { decision: String },
    Timer { milliseconds: u64 },
    System { payload: Value },
}

impl TestEvent {
    pub fn evidence_input(&self) -> Value {
        match self {
            Self::UserText { text } => json!({"text": text}),
            Self::Webhook { payload } | Self::System { payload } => payload.clone(),
            Self::HumanDecision { decision } => json!({"decision": decision}),
            Self::Timer { milliseconds } => json!({"timer_ms": milliseconds}),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub schema_version: String,
    pub name: String,
    pub events: Vec<TestEvent>,
}

#[derive(Clone, Copy, Debug)]
pub struct RunContext {
    pub eval_run_id: EvalRunId,
    pub invocation_id: InvocationId,
    pub scenario_id: ScenarioId,
}

#[derive(Clone, Debug)]
pub struct TargetSession {
    pub id: String,
    pub traceparent: String,
    pub baggage: String,
    pub context: RunContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetResponseEnvelope {
    pub schema_version: String,
    pub terminal: bool,
    pub terminal_state: Option<String>,
    #[serde(default)]
    pub output: Value,
    #[serde(default, alias = "synthetic_events")]
    pub events: Vec<ObservedEvent>,
    #[serde(default)]
    pub side_effects: Vec<Value>,
}

pub type TargetOutput = TargetResponseEnvelope;

impl TargetResponseEnvelope {
    pub(crate) fn validate(self) -> Result<Self, ScenarioError> {
        if self.schema_version != SCENARIO_SCHEMA_VERSION {
            return Err(ScenarioError::UnsupportedResponseSchema);
        }
        if self.events.len() > MAX_OBSERVATIONS {
            return Err(ScenarioError::TooManyObservations);
        }
        if self.events.iter().any(|event| event.name.trim().is_empty()) {
            return Err(ScenarioError::EmptyObservationName);
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ScenarioError {
    #[error("scenario schema_version must be 1.0")]
    UnsupportedSchema,
    #[error("scenario name is required and must not exceed 120 characters")]
    InvalidName,
    #[error("scenario must contain between 1 and 50 events")]
    InvalidEventCount,
    #[error("scenario text must not exceed 32 KiB")]
    TextTooLarge,
    #[error("scenario JSON payload must not exceed 256 KiB")]
    PayloadTooLarge,
    #[error("target response schema_version must be 1.0")]
    UnsupportedResponseSchema,
    #[error("target response contains more than 1000 observations")]
    TooManyObservations,
    #[error("target observation name must not be empty")]
    EmptyObservationName,
}

/// Validates scenario version, shape, event count, and payload bounds.
///
/// # Errors
///
/// Returns a contract-specific error for an unsupported schema, invalid name,
/// event count, or oversized event content.
pub fn validate_scenario(scenario: &ScenarioDefinition) -> Result<(), ScenarioError> {
    if scenario.schema_version != SCENARIO_SCHEMA_VERSION {
        return Err(ScenarioError::UnsupportedSchema);
    }
    let name = scenario.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ScenarioError::InvalidName);
    }
    if scenario.events.is_empty() || scenario.events.len() > MAX_SCENARIO_EVENTS {
        return Err(ScenarioError::InvalidEventCount);
    }
    for event in &scenario.events {
        match event {
            TestEvent::UserText { text } if text.len() > 32 * 1024 => {
                return Err(ScenarioError::TextTooLarge);
            }
            TestEvent::Webhook { payload } | TestEvent::System { payload }
                if serde_json::to_vec(payload).map_or(true, |bytes| bytes.len() > 256 * 1024) =>
            {
                return Err(ScenarioError::PayloadTooLarge);
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scenario_is_rejected() {
        let scenario = ScenarioDefinition {
            schema_version: "1.0".to_owned(),
            name: "empty".to_owned(),
            events: vec![],
        };
        assert_eq!(
            validate_scenario(&scenario),
            Err(ScenarioError::InvalidEventCount)
        );
    }

    #[test]
    fn synthetic_events_alias_is_supported() {
        let envelope: TargetResponseEnvelope = serde_json::from_value(json!({
            "schema_version": "1.0",
            "terminal": true,
            "terminal_state": "completed",
            "synthetic_events": []
        }))
        .expect("legacy field should deserialize");
        assert!(envelope.events.is_empty());
    }

    #[test]
    fn oversized_text_and_payload_are_rejected() {
        let text = ScenarioDefinition {
            schema_version: "1.0".to_owned(),
            name: "large text".to_owned(),
            events: vec![TestEvent::UserText {
                text: "x".repeat(32 * 1024 + 1),
            }],
        };
        assert_eq!(validate_scenario(&text), Err(ScenarioError::TextTooLarge));

        let payload = ScenarioDefinition {
            schema_version: "1.0".to_owned(),
            name: "large payload".to_owned(),
            events: vec![TestEvent::Webhook {
                payload: json!({"content": "x".repeat(256 * 1024 + 1)}),
            }],
        };
        assert_eq!(
            validate_scenario(&payload),
            Err(ScenarioError::PayloadTooLarge)
        );
    }
}
