use std::{collections::BTreeMap, io::Read};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use flate2::read::GzDecoder;
use governance_application::{
    ApplicationError, IngestTelemetryBatch, TelemetryIngestIdentity, TelemetryIngestKeyRepository,
};
use governance_config::{AppConfig, TelemetryConfig};
use governance_persistence::{SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository};
use governance_telemetry::{ObservedSpan, RedactionPolicy, SpanLink, TelemetryLimits};
use opentelemetry_proto::tonic::{
    collector::trace::v1::{
        ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse,
    },
    common::v1::{AnyValue, KeyValue, any_value},
    trace::v1::{ResourceSpans, Span},
};
use prost::Message;
use sea_orm::Database;
use serde::Serialize;
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[derive(Clone, Debug)]
struct GatewayState {
    repository: SeaOrmEvaluationRunRepository,
    policy_packs: SeaOrmPolicyPackRepository,
    telemetry: TelemetryConfig,
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
    let database = Database::connect(&config.database_url).await?;
    let state = GatewayState {
        repository: SeaOrmEvaluationRunRepository::new(database.clone()),
        policy_packs: SeaOrmPolicyPackRepository::new(database),
        telemetry: config.telemetry.clone(),
    };
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(config.gateway_addr).await?;
    tracing::info!(address = %config.gateway_addr, "telemetry gateway listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn router(state: GatewayState) -> Router {
    let compressed_limit = state.telemetry.max_compressed_bytes;
    Router::new()
        .route("/health", get(health))
        .route("/v1/traces", post(ingest))
        .with_state(state)
        .layer(DefaultBodyLimit::max(compressed_limit))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::new(
            header::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ))
        .layer(TraceLayer::new_for_http())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health() -> impl IntoResponse {
    axum::Json(Health {
        status: "ok",
        service: "governance-telemetry-gateway",
    })
}

async fn ingest(State(state): State<GatewayState>, headers: HeaderMap, body: Bytes) -> Response {
    match ingest_inner(&state, &headers, &body).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn ingest_inner(
    state: &GatewayState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, GatewayError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap_or_default();
    if !matches!(content_type, "application/x-protobuf" | "application/json") {
        return Err(GatewayError::UnsupportedMediaType);
    }
    let identity = authenticate(&state.repository, headers).await?;
    let decoded = decode_body(headers, body, state.telemetry.max_decoded_bytes)?;
    let request = match content_type {
        "application/x-protobuf" => ExportTraceServiceRequest::decode(decoded.as_slice())
            .map_err(|_| GatewayError::InvalidPayload)?,
        "application/json" => {
            serde_json::from_slice(&decoded).map_err(|_| GatewayError::InvalidPayload)?
        }
        _ => unreachable!(),
    };
    let converted = convert_request(request);
    let conversion_rejected = converted.rejected;
    let service = IngestTelemetryBatch::new(
        state.policy_packs.clone(),
        state.repository.clone(),
        state.repository.clone(),
        state.repository.clone(),
        state.repository.clone(),
        RedactionPolicy::default(),
        TelemetryLimits {
            max_spans_per_request: state.telemetry.max_spans_per_request,
            max_spans_per_run: state.telemetry.max_spans_per_run,
            max_attributes_per_span: state.telemetry.max_attributes_per_span,
            max_string_bytes: state.telemetry.max_string_bytes,
        },
        u64::try_from(state.telemetry.default_settle_seconds).unwrap_or(10),
        u64::try_from(state.telemetry.default_idle_timeout_seconds).unwrap_or(300),
        u64::try_from(state.telemetry.max_run_duration_seconds).unwrap_or(86_400),
    );
    let outcome = service
        .execute(&identity, converted.spans)
        .await
        .map_err(|error| ingest_error(&error))?;
    let rejected = conversion_rejected.saturating_add(outcome.rejected);
    let response = ExportTraceServiceResponse {
        partial_success: (rejected > 0).then(|| ExportTracePartialSuccess {
            rejected_spans: i64::try_from(rejected).unwrap_or(i64::MAX),
            error_message: "one or more spans failed validation or correlation".to_owned(),
        }),
    };
    if content_type == "application/x-protobuf" {
        let mut encoded = Vec::with_capacity(response.encoded_len());
        response
            .encode(&mut encoded)
            .map_err(|_| GatewayError::Unavailable("response encoding failed".to_owned()))?;
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-protobuf")],
            encoded,
        )
            .into_response())
    } else {
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_vec(&response)
                .map_err(|_| GatewayError::Unavailable("response encoding failed".to_owned()))?,
        )
            .into_response())
    }
}

async fn authenticate(
    repository: &SeaOrmEvaluationRunRepository,
    headers: &HeaderMap,
) -> Result<TelemetryIngestIdentity, GatewayError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| value.starts_with("flt_") && value.len() >= 12)
        .ok_or(GatewayError::Unauthorized)?;
    let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
    repository
        .resolve_key(&token[..12], &digest, OffsetDateTime::now_utc())
        .await
        .map_err(|error| GatewayError::Unavailable(error.to_string()))?
        .ok_or(GatewayError::Unauthorized)
}

fn decode_body(
    headers: &HeaderMap,
    body: &[u8],
    decoded_limit: usize,
) -> Result<Vec<u8>, GatewayError> {
    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("identity");
    let decoded = match encoding {
        "identity" | "" => body.to_vec(),
        "gzip" => {
            let decoder = GzDecoder::new(body);
            let mut bounded = decoder.take(
                u64::try_from(decoded_limit)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            );
            let mut output = Vec::new();
            bounded
                .read_to_end(&mut output)
                .map_err(|_| GatewayError::InvalidPayload)?;
            output
        }
        _ => return Err(GatewayError::UnsupportedEncoding),
    };
    if decoded.len() > decoded_limit {
        return Err(GatewayError::PayloadTooLarge);
    }
    Ok(decoded)
}

struct ConvertedRequest {
    spans: Vec<ObservedSpan>,
    rejected: usize,
}

fn convert_request(request: ExportTraceServiceRequest) -> ConvertedRequest {
    let mut converted = ConvertedRequest {
        spans: Vec::new(),
        rejected: 0,
    };
    for resource_spans in request.resource_spans {
        convert_resource_spans(resource_spans, &mut converted);
    }
    converted
}

fn convert_resource_spans(resource_spans: ResourceSpans, converted: &mut ConvertedRequest) {
    let resource_attributes = resource_spans
        .resource
        .map(|resource| key_values(resource.attributes))
        .unwrap_or_default();
    for scope_spans in resource_spans.scope_spans {
        let scope = scope_spans.scope.and_then(|scope| {
            if scope.name.is_empty() {
                None
            } else if scope.version.is_empty() {
                Some(scope.name)
            } else {
                Some(format!("{}@{}", scope.name, scope.version))
            }
        });
        for span in scope_spans.spans {
            match convert_span(span, resource_attributes.clone(), scope.clone()) {
                Some(span) => converted.spans.push(span),
                None => converted.rejected += 1,
            }
        }
    }
}

fn convert_span(
    span: Span,
    resource_attributes: BTreeMap<String, Value>,
    instrumentation_scope: Option<String>,
) -> Option<ObservedSpan> {
    if span.trace_id.len() != 16
        || span.span_id.len() != 8
        || span.trace_id.iter().all(|byte| *byte == 0)
        || span.span_id.iter().all(|byte| *byte == 0)
        || (!span.parent_span_id.is_empty()
            && (span.parent_span_id.len() != 8
                || span.parent_span_id.iter().all(|byte| *byte == 0)))
        || span.links.iter().any(|link| {
            link.trace_id.len() != 16
                || link.span_id.len() != 8
                || link.trace_id.iter().all(|byte| *byte == 0)
                || link.span_id.iter().all(|byte| *byte == 0)
        })
        || span.name.is_empty()
        || span.start_time_unix_nano == 0
    {
        return None;
    }
    let started_at = timestamp(span.start_time_unix_nano)?;
    let ended_at = (span.end_time_unix_nano != 0)
        .then(|| timestamp(span.end_time_unix_nano))
        .flatten();
    let parent_span_id =
        (!span.parent_span_id.is_empty()).then(|| hex::encode(span.parent_span_id));
    let links = span
        .links
        .into_iter()
        .map(|link| SpanLink {
            trace_id: hex::encode(link.trace_id),
            span_id: hex::encode(link.span_id),
        })
        .collect();
    Some(ObservedSpan {
        trace_id: hex::encode(span.trace_id),
        span_id: hex::encode(span.span_id),
        parent_span_id,
        links,
        name: span.name,
        started_at,
        ended_at,
        attributes: key_values(span.attributes),
        resource_attributes,
        instrumentation_scope,
        status: span.status.map(|status| match status.code {
            1 => "ok".to_owned(),
            2 => "error".to_owned(),
            _ => "unset".to_owned(),
        }),
    })
}

fn timestamp(nanos: u64) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos)).ok()
}

fn key_values(values: Vec<KeyValue>) -> BTreeMap<String, Value> {
    values
        .into_iter()
        .filter(|entry| !entry.key.is_empty())
        .filter_map(|entry| entry.value.map(|value| (entry.key, any_value(value))))
        .collect()
}

fn any_value(value: AnyValue) -> Value {
    match value.value {
        Some(any_value::Value::StringValue(value)) => Value::String(value),
        Some(any_value::Value::BoolValue(value)) => Value::Bool(value),
        Some(any_value::Value::IntValue(value)) => Value::Number(value.into()),
        Some(any_value::Value::DoubleValue(value)) => {
            Number::from_f64(value).map_or(Value::Null, Value::Number)
        }
        Some(any_value::Value::ArrayValue(value)) => {
            Value::Array(value.values.into_iter().map(any_value).collect())
        }
        Some(any_value::Value::KvlistValue(value)) => {
            Value::Object(key_values(value.values).into_iter().collect::<Map<_, _>>())
        }
        Some(any_value::Value::BytesValue(_) | any_value::Value::StringValueStrindex(_)) | None => {
            Value::Null
        }
    }
}

#[derive(Debug)]
enum GatewayError {
    Unauthorized,
    UnsupportedMediaType,
    UnsupportedEncoding,
    InvalidPayload,
    PayloadTooLarge,
    Conflict,
    Unavailable(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "invalid telemetry ingest key"),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported OTLP content type",
            ),
            Self::UnsupportedEncoding => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported content encoding",
            ),
            Self::InvalidPayload => (StatusCode::BAD_REQUEST, "invalid OTLP trace payload"),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "OTLP payload too large"),
            Self::Conflict => (StatusCode::CONFLICT, "telemetry correlation conflict"),
            Self::Unavailable(ref detail) => {
                tracing::warn!(error = %detail, "telemetry ingestion unavailable");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "telemetry ingestion unavailable",
                )
            }
        };
        (status, message).into_response()
    }
}

fn ingest_error(error: &ApplicationError) -> GatewayError {
    match error {
        ApplicationError::InvalidRequest(_) | ApplicationError::NotFound(_) => {
            GatewayError::InvalidPayload
        }
        ApplicationError::Forbidden(_) => GatewayError::Unauthorized,
        ApplicationError::Conflict(_) => GatewayError::Conflict,
        ApplicationError::Repository(_)
        | ApplicationError::Unavailable(_)
        | ApplicationError::TargetTransport(_)
        | ApplicationError::TargetTimeout(_)
        | ApplicationError::TargetContract(_) => GatewayError::Unavailable(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    #[test]
    fn json_and_protobuf_decode_to_the_same_spans() {
        let json = include_bytes!("../../../fixtures/otlp/correlated-run/02-execution.json");
        let request: ExportTraceServiceRequest =
            serde_json::from_slice(json).expect("OTLP JSON fixture should decode");
        let mut protobuf = Vec::new();
        request
            .encode(&mut protobuf)
            .expect("OTLP request should encode");
        let decoded = ExportTraceServiceRequest::decode(protobuf.as_slice())
            .expect("OTLP protobuf should decode");

        let from_json = convert_request(request);
        let from_protobuf = convert_request(decoded);

        assert_eq!(from_json.rejected, 0);
        assert_eq!(from_json.spans.len(), 2);
        assert_eq!(from_json.spans[0].trace_id, from_protobuf.spans[0].trace_id);
        assert_eq!(from_json.spans[0].span_id, from_protobuf.spans[0].span_id);
    }

    #[test]
    fn gzip_decoding_honors_the_expanded_limit() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"expanded payload").expect("gzip write");
        let compressed = encoder.finish().expect("gzip finish");
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, "gzip".parse().expect("header"));

        assert_eq!(
            decode_body(&headers, &compressed, 64).expect("within limit"),
            b"expanded payload"
        );
        assert!(matches!(
            decode_body(&headers, &compressed, 4),
            Err(GatewayError::PayloadTooLarge)
        ));
    }

    #[test]
    fn cross_trace_fixture_contains_a_causal_approval_link() {
        let approval: ExportTraceServiceRequest = serde_json::from_slice(include_bytes!(
            "../../../fixtures/otlp/correlated-run/01-approval.json"
        ))
        .expect("approval fixture");
        let execution: ExportTraceServiceRequest = serde_json::from_slice(include_bytes!(
            "../../../fixtures/otlp/correlated-run/02-execution.json"
        ))
        .expect("execution fixture");
        let approval = convert_request(approval);
        let execution = convert_request(execution);

        assert_eq!(
            approval.spans[0].trace_id,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            execution.spans[0].links[0].trace_id,
            approval.spans[0].trace_id
        );
        assert_ne!(execution.spans[0].trace_id, approval.spans[0].trace_id);
    }

    #[test]
    fn passive_fixture_uses_external_session_and_terminal_without_eval_run_id() {
        let execution: ExportTraceServiceRequest = serde_json::from_slice(include_bytes!(
            "../../../fixtures/otlp/passive-session/02-execution-terminal.json"
        ))
        .expect("passive execution fixture");
        let execution = convert_request(execution);

        assert_eq!(execution.rejected, 0);
        assert_eq!(execution.spans.len(), 2);
        assert_eq!(
            execution.spans[0]
                .resource_attributes
                .get("featherlane.external_run.id")
                .and_then(serde_json::Value::as_str),
            Some("session-placeholder")
        );
        assert!(
            !execution.spans[0]
                .attributes
                .contains_key("featherlane.eval_run.id")
        );
        assert_eq!(
            execution.spans[1]
                .attributes
                .get("featherlane.run.terminal")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}
