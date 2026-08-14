use std::fs;

use axum::{body::Body, http::Request};
use governance_domain::ReviewStatus;
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn health_route_is_available() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unknown_evaluation_returns_problem_response() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/evaluations/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn empty_overview_does_not_fabricate_metrics() {
    let Json(snapshot) = overview().await;

    assert_eq!(snapshot.active_agents, 0);
    assert_eq!(snapshot.evaluations_30d, 0);
    assert!(snapshot.pass_rate.abs() < f64::EPSILON);
    assert_eq!(snapshot.open_findings, 0);
    assert!(snapshot.trace_coverage.abs() < f64::EPSILON);
    assert!(snapshot.recent_runs.is_empty());
    assert!(snapshot.daily_activity.is_empty());
}

#[tokio::test]
async fn corpus_is_resolved_by_set_name() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/corpora/open-us-law")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unknown_corpus_set_returns_not_found() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/v1/corpora/not-imported")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn import_fixture_builds_one_draft_database_aggregate() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/policies/refund-governance.import.json");
    let input = fs::read_to_string(path).expect("policy fixture should be readable");
    let request: PolicyImportRequest =
        serde_json::from_str(&input).expect("policy fixture should match the import contract");
    let bundle = build_policy_bundle(default_organization_id(), &request)
        .expect("policy aggregate should compile");

    assert_eq!(bundle.pack.status, ReviewStatus::Draft);
    assert_eq!(bundle.pack.rules.len(), 1);
    assert_eq!(bundle.sources.len(), 1);
    assert_eq!(bundle.obligations.len(), 1);
    assert_eq!(bundle.pack.content_sha256.len(), 64);
}

#[test]
fn import_rejects_a_rule_without_a_persisted_obligation() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/policies/refund-governance.import.json");
    let input = fs::read_to_string(path).expect("policy fixture should be readable");
    let mut request: PolicyImportRequest =
        serde_json::from_str(&input).expect("policy fixture should match the import contract");
    request.rules[0].obligation_key = "MISSING".to_owned();

    assert!(build_policy_bundle(default_organization_id(), &request).is_err());
}

#[test]
fn target_versions_are_trimmed_before_persistence() {
    let request = CreateTargetRequest {
        name: "Test agent".to_owned(),
        key: "test-agent".to_owned(),
        version: "  git:abc123  ".to_owned(),
        environment: TargetEnvironment::Staging,
        driver_type: DriverType::HttpText,
        endpoint: "http://127.0.0.1:8091/messages".to_owned(),
        reset_endpoint: None,
        status_endpoint: None,
        terminal_response_key: None,
        auth_secret_ref: None,
        timeout_seconds: 30,
        otlp_required: false,
        telemetry_boundary: governance_targets::TelemetryBoundaryConfig::default(),
    };
    let capability = CapabilityReport {
        target_id: request.key.clone(),
        reachable: true,
        reset_supported: false,
        trace_context_supported: true,
        issues: vec![],
        checked_at: OffsetDateTime::now_utc(),
    };

    let target = build_registered_target(default_organization_id(), request, capability)
        .expect("target should be valid");

    assert_eq!(target.manifest.target_version, "git:abc123");
    let view = target_view(&target, None);
    assert!(!view.auto_evaluation_enabled);
    assert_eq!(view.automatic_boundary_kind, None);
    assert_eq!(view.default_policy_pack_id, None);
}

#[test]
fn target_view_exposes_valid_automatic_evaluation_binding() {
    let policy_pack_id = PolicyPackId::new();
    let request = CreateTargetRequest {
        name: "Observed agent".to_owned(),
        key: "observed-agent".to_owned(),
        version: "git:trace".to_owned(),
        environment: TargetEnvironment::Staging,
        driver_type: DriverType::HttpText,
        endpoint: "http://127.0.0.1:8091/messages".to_owned(),
        reset_endpoint: None,
        status_endpoint: None,
        terminal_response_key: None,
        auth_secret_ref: None,
        timeout_seconds: 30,
        otlp_required: true,
        telemetry_boundary: governance_targets::TelemetryBoundaryConfig {
            boundary_kind: RunBoundaryKind::WorkflowExecution,
            default_policy_pack_id: Some(policy_pack_id),
            idle_timeout_seconds: Some(300),
            max_duration_seconds: Some(3_600),
            ..governance_targets::TelemetryBoundaryConfig::default()
        },
    };
    let capability = CapabilityReport {
        target_id: request.key.clone(),
        reachable: true,
        reset_supported: false,
        trace_context_supported: true,
        issues: vec![],
        checked_at: OffsetDateTime::now_utc(),
    };
    let target = build_registered_target(default_organization_id(), request, capability)
        .expect("automatic target should be structurally valid");

    let view = target_view(&target, None);
    assert!(view.auto_evaluation_enabled);
    assert_eq!(
        view.automatic_boundary_kind,
        Some(RunBoundaryKind::WorkflowExecution)
    );
    assert_eq!(view.default_policy_pack_id, Some(policy_pack_id));
}
