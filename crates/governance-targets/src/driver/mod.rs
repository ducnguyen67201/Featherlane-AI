mod http_text;
mod webhook;

use std::{env, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, redirect::Policy};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    CapabilityReport, DriverType, RunContext, TargetManifest, TargetOutput, TargetResponseEnvelope,
    TargetSession, TestEvent, validate_manifest,
};

pub use http_text::HttpTextDriver;
pub use webhook::WebhookDriver;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub trait SecretResolver: Send + Sync + std::fmt::Debug {
    /// Resolves a server-side secret reference immediately before a request.
    ///
    /// # Errors
    ///
    /// Returns an error when the named secret is unavailable.
    fn resolve(&self, reference: &str) -> Result<String, DriverError>;
}

#[derive(Clone, Debug, Default)]
pub struct EnvironmentSecretResolver;

impl SecretResolver for EnvironmentSecretResolver {
    fn resolve(&self, reference: &str) -> Result<String, DriverError> {
        env::var(reference).map_err(|_| DriverError::MissingSecretReference(reference.to_owned()))
    }
}

#[async_trait]
pub trait TargetDriver: Send + Sync {
    async fn validate(&self, manifest: &TargetManifest) -> Result<CapabilityReport, DriverError>;
    fn start_session(&self, context: RunContext) -> TargetSession;
    async fn reset(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
    ) -> Result<(), DriverError>;
    async fn send(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
        event: &TestEvent,
    ) -> Result<TargetOutput, DriverError>;
}

pub trait TargetDriverRegistry: Send + Sync {
    fn driver_for(&self, driver_type: DriverType) -> &dyn TargetDriver;
}

#[derive(Clone, Debug)]
pub struct DefaultDriverRegistry {
    http_text: HttpTextDriver,
    webhook: WebhookDriver,
}

impl DefaultDriverRegistry {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self {
            http_text: HttpTextDriver::new(resolver.clone()),
            webhook: WebhookDriver::new(resolver),
        }
    }
}

impl Default for DefaultDriverRegistry {
    fn default() -> Self {
        Self::new(Arc::new(EnvironmentSecretResolver))
    }
}

impl TargetDriverRegistry for DefaultDriverRegistry {
    fn driver_for(&self, driver_type: DriverType) -> &dyn TargetDriver {
        match driver_type {
            DriverType::HttpText => &self.http_text,
            DriverType::Webhook => &self.webhook,
        }
    }
}

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("target request timed out")]
    Timeout,
    #[error("target transport failed")]
    Transport,
    #[error("target rejected request with status {0}")]
    Rejected(StatusCode),
    #[error("target is not safely configured: {0}")]
    UnsafeConfiguration(String),
    #[error("target event is not supported by the selected driver")]
    UnsupportedEvent,
    #[error("target response exceeded the 2 MiB limit")]
    ResponseTooLarge,
    #[error("target response was not valid integration JSON")]
    InvalidResponse,
    #[error("target response violated the integration contract: {0}")]
    Contract(String),
    #[error("configured secret reference {0} is unavailable")]
    MissingSecretReference(String),
}

pub(crate) async fn validate_common(
    resolver: &dyn SecretResolver,
    manifest: &TargetManifest,
) -> Result<CapabilityReport, DriverError> {
    validate_manifest(manifest)
        .map_err(|error| DriverError::UnsafeConfiguration(error.to_string()))?;
    let client = client(manifest.timeout_seconds)?;
    let request = authorize(client.get(&manifest.endpoint), resolver, manifest)?;
    let response = request.send().await;
    let reachable = response
        .as_ref()
        .is_ok_and(|response| response.status().is_success());
    let mut issues = Vec::new();
    if !reachable {
        issues.push("target endpoint did not return a successful readiness response".to_owned());
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
        checked_at: OffsetDateTime::now_utc(),
    })
}

pub(crate) fn start_session_common(context: RunContext) -> TargetSession {
    let trace_id = Uuid::new_v4().simple().to_string();
    let parent_source = Uuid::new_v4().simple().to_string();
    let parent_id = &parent_source[..16];
    TargetSession {
        id: Uuid::now_v7().to_string(),
        traceparent: format!("00-{trace_id}-{parent_id}-01"),
        baggage: format!(
            "featherlane.eval_run.id={},featherlane.invocation.id={},featherlane.scenario.id={}",
            context.eval_run_id, context.invocation_id, context.scenario_id
        ),
        context,
    }
}

pub(crate) async fn reset_common(
    resolver: &dyn SecretResolver,
    manifest: &TargetManifest,
    session: &TargetSession,
) -> Result<(), DriverError> {
    let Some(endpoint) = &manifest.reset_endpoint else {
        return Ok(());
    };
    let request = correlated(client(manifest.timeout_seconds)?.post(endpoint), session);
    let response = authorize(request, resolver, manifest)?
        .send()
        .await
        .map_err(map_reqwest)?;
    if !response.status().is_success() {
        return Err(DriverError::Rejected(response.status()));
    }
    Ok(())
}

pub(crate) async fn send_json(
    resolver: &dyn SecretResolver,
    manifest: &TargetManifest,
    session: &TargetSession,
    payload: &Value,
) -> Result<TargetOutput, DriverError> {
    let request = correlated(
        client(manifest.timeout_seconds)?.post(&manifest.endpoint),
        session,
    )
    .json(payload);
    let response = authorize(request, resolver, manifest)?
        .send()
        .await
        .map_err(map_reqwest)?;
    if !response.status().is_success() {
        return Err(DriverError::Rejected(response.status()));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(DriverError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let envelope: TargetResponseEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| DriverError::InvalidResponse)?;
    envelope
        .validate()
        .map_err(|error| DriverError::Contract(error.to_string()))
}

fn client(timeout_seconds: u64) -> Result<Client, DriverError> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .redirect(Policy::none())
        .build()
        .map_err(map_reqwest)
}

fn authorize(
    request: RequestBuilder,
    resolver: &dyn SecretResolver,
    manifest: &TargetManifest,
) -> Result<RequestBuilder, DriverError> {
    match &manifest.auth_secret_ref {
        Some(reference) => resolver
            .resolve(reference)
            .map(|secret| request.bearer_auth(secret)),
        None => Ok(request),
    }
}

fn correlated(request: RequestBuilder, session: &TargetSession) -> RequestBuilder {
    let eval_run_id = session.context.eval_run_id.to_string();
    let invocation_id = session.context.invocation_id.to_string();
    let scenario_id = session.context.scenario_id.to_string();
    request
        .header("traceparent", &session.traceparent)
        .header("baggage", &session.baggage)
        .header("x-featherlane-eval-run-id", &eval_run_id)
        .header("x-featherlane-invocation-id", &invocation_id)
        .header("x-featherlane-scenario-id", &scenario_id)
        .header("x-governance-eval-run-id", &eval_run_id)
        .header("x-governance-invocation-id", &invocation_id)
        .header("x-governance-scenario-id", &scenario_id)
}

#[allow(clippy::needless_pass_by_value)] // Required as a direct `map_err` adapter.
fn map_reqwest(error: reqwest::Error) -> DriverError {
    if error.is_timeout() {
        DriverError::Timeout
    } else {
        DriverError::Transport
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode as AxumStatusCode},
        routing::{get, post},
    };
    use governance_domain::{EvalRunId, InvocationId, ScenarioId};
    use serde_json::json;

    use super::*;

    #[derive(Clone, Debug, Default)]
    struct Capture(Arc<Mutex<Vec<(String, HeaderMap, Value)>>>);

    #[derive(Debug)]
    struct FakeSecretResolver;

    impl SecretResolver for FakeSecretResolver {
        fn resolve(&self, _reference: &str) -> Result<String, DriverError> {
            Ok("test-bearer-value".to_owned())
        }
    }

    fn test_manifest(
        address: std::net::SocketAddr,
        path: &str,
        driver_type: DriverType,
    ) -> TargetManifest {
        TargetManifest {
            schema_version: "1.0".to_owned(),
            target_id: "test-agent".to_owned(),
            target_version: "git:test".to_owned(),
            driver_type,
            endpoint: format!("http://{address}{path}"),
            reset_endpoint: None,
            status_endpoint: None,
            terminal_response_key: None,
            auth_secret_ref: None,
            timeout_seconds: 5,
            evidence_mode: crate::EvidenceMode::Inline,
            otlp_required: false,
            production_credentials_allowed: false,
            telemetry_boundary: crate::TelemetryBoundaryConfig::default(),
        }
    }

    #[tokio::test]
    async fn generated_traceparent_has_w3c_shape() {
        let session = start_session_common(RunContext {
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        });
        let parts: Vec<&str> = session.traceparent.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1].len(), 32);
        assert_eq!(parts[2].len(), 16);
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn http_text_sends_contract_headers_auth_and_body_after_reset() {
        async fn readiness() -> AxumStatusCode {
            AxumStatusCode::OK
        }
        async fn reset(State(capture): State<Capture>, headers: HeaderMap) -> AxumStatusCode {
            capture.0.lock().expect("capture lock").push((
                "reset".to_owned(),
                headers,
                Value::Null,
            ));
            AxumStatusCode::NO_CONTENT
        }
        async fn message(
            State(capture): State<Capture>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            capture
                .0
                .lock()
                .expect("capture lock")
                .push(("message".to_owned(), headers, body));
            Json(json!({
                "schema_version": "1.0",
                "terminal": true,
                "terminal_state": "completed",
                "events": [{
                    "event_type": "final_output",
                    "name": "done",
                    "actor": {"actor_type": "agent", "id": "test-agent"},
                    "attributes": {"terminal_state": "completed"}
                }]
            }))
        }

        let capture = Capture::default();
        let app = Router::new()
            .route("/messages", get(readiness).post(message))
            .route("/reset", post(reset))
            .with_state(capture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener");
        let address = listener.local_addr().expect("test address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let mut manifest = test_manifest(address, "/messages", DriverType::HttpText);
        manifest.reset_endpoint = Some(format!("http://{address}/reset"));
        manifest.auth_secret_ref = Some("TEST_TARGET_TOKEN".to_owned());
        let driver = HttpTextDriver::new(Arc::new(FakeSecretResolver));
        assert!(
            driver
                .validate(&manifest)
                .await
                .expect("readiness")
                .reachable
        );
        let session = driver.start_session(RunContext {
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        });
        driver.reset(&manifest, &session).await.expect("reset");
        let output = driver
            .send(
                &manifest,
                &session,
                &TestEvent::UserText {
                    text: "hello".to_owned(),
                },
            )
            .await
            .expect("send");
        assert!(output.terminal);

        let requests = capture.0.lock().expect("capture lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].0, "reset");
        assert_eq!(requests[1].0, "message");
        assert_eq!(
            requests[1].2,
            json!({"session_id": session.id, "message": "hello"})
        );
        for (_, headers, _) in requests.iter() {
            assert_eq!(headers["traceparent"], session.traceparent);
            assert_eq!(
                headers["x-governance-eval-run-id"],
                session.context.eval_run_id.to_string()
            );
            assert_eq!(
                headers["x-governance-scenario-id"],
                session.context.scenario_id.to_string()
            );
            assert_eq!(headers["authorization"], "Bearer test-bearer-value");
        }
        drop(requests);
        server.abort();
    }

    #[tokio::test]
    async fn webhook_preserves_the_configured_json_payload() {
        async fn webhook(
            State(capture): State<Capture>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            capture
                .0
                .lock()
                .expect("capture lock")
                .push(("webhook".to_owned(), headers, body));
            Json(json!({
                "schema_version": "1.0",
                "terminal": true,
                "terminal_state": "completed",
                "events": [{
                    "event_type": "final_output",
                    "name": "done",
                    "actor": {"actor_type": "agent", "id": "workflow"}
                }]
            }))
        }

        let capture = Capture::default();
        let app = Router::new()
            .route("/webhook", post(webhook))
            .with_state(capture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener");
        let address = listener.local_addr().expect("test address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let manifest = test_manifest(address, "/webhook", DriverType::Webhook);
        let driver = WebhookDriver::new(Arc::new(FakeSecretResolver));
        let session = driver.start_session(RunContext {
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        });
        let payload = json!({"ticket": "T-1", "nested": {"ready": true}});

        driver
            .send(
                &manifest,
                &session,
                &TestEvent::Webhook {
                    payload: payload.clone(),
                },
            )
            .await
            .expect("webhook should complete");

        assert_eq!(capture.0.lock().expect("capture lock")[0].2, payload);
        server.abort();
    }

    #[tokio::test]
    async fn response_failures_are_bounded_and_do_not_leak_response_bodies() {
        async fn rejected() -> (AxumStatusCode, &'static str) {
            (AxumStatusCode::UNAUTHORIZED, "test-bearer-value")
        }
        async fn invalid() -> &'static str {
            "not-json"
        }
        async fn oversized() -> String {
            "x".repeat(MAX_RESPONSE_BYTES + 1)
        }
        async fn slow() -> Json<Value> {
            tokio::time::sleep(Duration::from_millis(1_100)).await;
            Json(json!({
                "schema_version": "1.0",
                "terminal": true,
                "events": []
            }))
        }

        let app = Router::new()
            .route("/rejected", post(rejected))
            .route("/invalid", post(invalid))
            .route("/oversized", post(oversized))
            .route("/slow", post(slow));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener");
        let address = listener.local_addr().expect("test address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server");
        });
        let driver = HttpTextDriver::new(Arc::new(FakeSecretResolver));
        let session = driver.start_session(RunContext {
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        });
        let event = TestEvent::UserText {
            text: "hello".to_owned(),
        };

        let mut manifest = test_manifest(address, "/rejected", DriverType::HttpText);
        let error = driver
            .send(&manifest, &session, &event)
            .await
            .expect_err("non-success must fail");
        assert!(matches!(
            error,
            DriverError::Rejected(StatusCode::UNAUTHORIZED)
        ));
        assert!(!error.to_string().contains("test-bearer-value"));

        manifest.endpoint = format!("http://{address}/invalid");
        assert!(matches!(
            driver.send(&manifest, &session, &event).await,
            Err(DriverError::InvalidResponse)
        ));

        manifest.endpoint = format!("http://{address}/oversized");
        assert!(matches!(
            driver.send(&manifest, &session, &event).await,
            Err(DriverError::ResponseTooLarge)
        ));

        manifest.endpoint = format!("http://{address}/slow");
        manifest.timeout_seconds = 1;
        assert!(matches!(
            driver.send(&manifest, &session, &event).await,
            Err(DriverError::Timeout)
        ));
        server.abort();
    }
}
