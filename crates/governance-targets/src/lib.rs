//! Framework-neutral target manifests and HTTP/webhook drivers.

use async_trait::async_trait;
use governance_domain::{EvalRunId, InvocationId, PolicyPackId, RunBoundaryKind, ScenarioId};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverType {
    HttpText,
    Webhook,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetManifest {
    pub target_id: String,
    pub target_version: String,
    pub driver_type: DriverType,
    pub endpoint: String,
    pub reset_endpoint: Option<String>,
    pub status_endpoint: Option<String>,
    #[serde(default)]
    pub terminal_response_key: Option<String>,
    pub auth_secret_ref: Option<String>,
    pub timeout_seconds: u64,
    pub otlp_required: bool,
    pub production_credentials_allowed: bool,
    #[serde(default)]
    pub telemetry_boundary: TelemetryBoundaryConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryBoundaryConfig {
    pub boundary_kind: RunBoundaryKind,
    #[serde(default)]
    pub external_id_attributes: Vec<String>,
    #[serde(default)]
    pub terminal_attribute: Option<String>,
    #[serde(default)]
    pub default_policy_pack_id: Option<PolicyPackId>,
    #[serde(default = "default_settle_seconds")]
    pub settle_seconds: u64,
    #[serde(default)]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub max_duration_seconds: Option<u64>,
    #[serde(default)]
    pub conversation_id_is_task_boundary: bool,
}

const fn default_settle_seconds() -> u64 {
    10
}

impl Default for TelemetryBoundaryConfig {
    fn default() -> Self {
        Self {
            boundary_kind: RunBoundaryKind::ExplicitCi,
            external_id_attributes: vec!["featherlane.external_run.id".to_owned()],
            terminal_attribute: Some("featherlane.run.terminal".to_owned()),
            default_policy_pack_id: None,
            settle_seconds: default_settle_seconds(),
            idle_timeout_seconds: None,
            max_duration_seconds: None,
            conversation_id_is_task_boundary: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TestEvent {
    UserText { text: String },
    Webhook { payload: Value },
    HumanDecision { decision: String },
    Timer { milliseconds: u64 },
    System { payload: Value },
}

#[derive(Clone, Debug)]
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
pub struct TargetOutput {
    pub terminal: bool,
    pub body: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub target_id: String,
    pub reachable: bool,
    pub reset_supported: bool,
    pub trace_context_supported: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("target transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("target rejected request with status {0}")]
    Rejected(StatusCode),
    #[error("target is not safely configured for evaluation: {0}")]
    UnsafeConfiguration(String),
}

#[async_trait]
pub trait TargetDriver: Send + Sync {
    async fn validate(&self, manifest: &TargetManifest) -> Result<CapabilityReport, DriverError>;
    async fn reset(
        &self,
        manifest: &TargetManifest,
        context: &RunContext,
    ) -> Result<(), DriverError>;
    async fn start_session(&self, context: RunContext) -> Result<TargetSession, DriverError>;
    async fn send(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
        event: &TestEvent,
    ) -> Result<TargetOutput, DriverError>;
}

#[derive(Clone, Debug)]
pub struct HttpTargetDriver {
    client: Client,
}

impl HttpTargetDriver {
    /// Builds a driver with the supplied request timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(timeout_seconds: u64) -> Result<Self, DriverError> {
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_seconds))
                .build()?,
        })
    }
}

fn with_correlation_headers(
    builder: reqwest::RequestBuilder,
    context: &RunContext,
) -> reqwest::RequestBuilder {
    let eval_run_id = context.eval_run_id.to_string();
    let invocation_id = context.invocation_id.to_string();
    let scenario_id = context.scenario_id.to_string();
    builder
        .header("x-featherlane-eval-run-id", &eval_run_id)
        .header("x-featherlane-invocation-id", &invocation_id)
        .header("x-featherlane-scenario-id", &scenario_id)
        .header("x-governance-eval-run-id", &eval_run_id)
        .header("x-governance-invocation-id", &invocation_id)
        .header("x-governance-scenario-id", &scenario_id)
}

fn response_is_terminal(manifest: &TargetManifest, body: &Value) -> bool {
    body.get(
        manifest
            .terminal_response_key
            .as_deref()
            .unwrap_or("terminal"),
    )
    .and_then(Value::as_bool)
    .unwrap_or(false)
}

#[async_trait]
impl TargetDriver for HttpTargetDriver {
    async fn validate(&self, manifest: &TargetManifest) -> Result<CapabilityReport, DriverError> {
        if manifest.production_credentials_allowed {
            return Err(DriverError::UnsafeConfiguration(
                "production credentials must be disabled".to_owned(),
            ));
        }
        let response = self.client.get(&manifest.endpoint).send().await;
        let reachable = response
            .as_ref()
            .is_ok_and(|response| response.status().is_success());
        let mut issues = Vec::new();
        if !reachable {
            issues.push("target endpoint did not return a successful response".to_owned());
        }
        if manifest.otlp_required && manifest.status_endpoint.is_none() {
            issues.push("OTLP is required but no terminal/status endpoint is declared".to_owned());
        }
        Ok(CapabilityReport {
            target_id: manifest.target_id.clone(),
            reachable,
            reset_supported: manifest.reset_endpoint.is_some(),
            trace_context_supported: true,
            issues,
        })
    }

    async fn reset(
        &self,
        manifest: &TargetManifest,
        context: &RunContext,
    ) -> Result<(), DriverError> {
        let Some(endpoint) = &manifest.reset_endpoint else {
            return Ok(());
        };
        let response = with_correlation_headers(self.client.post(endpoint), context)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(DriverError::Rejected(response.status()));
        }
        Ok(())
    }

    async fn start_session(&self, context: RunContext) -> Result<TargetSession, DriverError> {
        let trace_id = Uuid::new_v4().simple().to_string();
        let parent_source = Uuid::new_v4().simple().to_string();
        let parent_id = &parent_source[..16];
        Ok(TargetSession {
            id: Uuid::now_v7().to_string(),
            traceparent: format!("00-{trace_id}-{parent_id}-01"),
            baggage: format!(
                "featherlane.eval_run.id={},featherlane.invocation.id={},featherlane.scenario.id={}",
                context.eval_run_id, context.invocation_id, context.scenario_id
            ),
            context,
        })
    }

    async fn send(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
        event: &TestEvent,
    ) -> Result<TargetOutput, DriverError> {
        let payload = match event {
            TestEvent::UserText { text } => json!({"session_id": session.id, "message": text}),
            TestEvent::Webhook { payload } | TestEvent::System { payload } => payload.clone(),
            TestEvent::HumanDecision { decision } => json!({"decision": decision}),
            TestEvent::Timer { milliseconds } => json!({"timer_ms": milliseconds}),
        };
        let response = with_correlation_headers(
            self.client
                .post(&manifest.endpoint)
                .header("traceparent", &session.traceparent)
                .header("baggage", &session.baggage)
                .json(&payload),
            &session.context,
        )
        .send()
        .await?;
        if !response.status().is_success() {
            return Err(DriverError::Rejected(response.status()));
        }
        let body: Value = response.json().await?;
        Ok(TargetOutput {
            terminal: response_is_terminal(manifest, &body),
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generated_traceparent_has_w3c_shape() {
        let driver = HttpTargetDriver::new(1).expect("client should build");
        let session = driver
            .start_session(RunContext {
                eval_run_id: EvalRunId::new(),
                invocation_id: InvocationId::new(),
                scenario_id: ScenarioId::new(),
            })
            .await
            .expect("session should start");
        let parts: Vec<&str> = session.traceparent.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1].len(), 32);
        assert_eq!(parts[2].len(), 16);
        assert!(session.baggage.contains("featherlane.eval_run.id="));
        assert!(session.baggage.contains("featherlane.invocation.id="));
    }

    #[tokio::test]
    async fn production_credentials_are_rejected() {
        let driver = HttpTargetDriver::new(1).expect("client should build");
        let manifest = TargetManifest {
            target_id: "unsafe".to_owned(),
            target_version: "test".to_owned(),
            driver_type: DriverType::HttpText,
            endpoint: "http://127.0.0.1:1".to_owned(),
            reset_endpoint: None,
            status_endpoint: None,
            terminal_response_key: None,
            auth_secret_ref: None,
            timeout_seconds: 1,
            otlp_required: false,
            production_credentials_allowed: true,
            telemetry_boundary: TelemetryBoundaryConfig::default(),
        };
        assert!(matches!(
            driver.validate(&manifest).await,
            Err(DriverError::UnsafeConfiguration(_))
        ));
    }

    #[test]
    fn configured_response_key_marks_a_target_call_terminal() {
        let manifest = TargetManifest {
            target_id: "sandbox".to_owned(),
            target_version: "test".to_owned(),
            driver_type: DriverType::HttpText,
            endpoint: "http://127.0.0.1:1".to_owned(),
            reset_endpoint: None,
            status_endpoint: None,
            terminal_response_key: Some("approval_observed".to_owned()),
            auth_secret_ref: None,
            timeout_seconds: 1,
            otlp_required: false,
            production_credentials_allowed: false,
            telemetry_boundary: TelemetryBoundaryConfig::default(),
        };
        assert!(response_is_terminal(
            &manifest,
            &json!({"approval_observed": true})
        ));
    }
}
