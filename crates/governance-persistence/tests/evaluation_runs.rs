use std::collections::BTreeMap;

use governance_application::{
    CompleteEvaluationRun, CompletionRequest, DurableJobRepository, EvaluateFinalizedRun,
    EvaluationRunRepository, EvidenceBundleRepository, FinalizeEvaluationRun, IngestTelemetryBatch,
    SpanInsert, SpanInsertOutcome, TelemetrySpanRepository,
};
use governance_domain::{
    CompletionReason, EvalRunId, EvaluationRun, EvaluationRunState, InvocationId, OrganizationId,
    PolicyPackId, RunBoundaryKind, ScenarioId,
};
use governance_migration::Migrator;
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository,
    entities::{policy_rules, rule_results, targets},
};
use governance_telemetry::{
    ATTR_EVAL_RUN_ID, ATTR_EVENT_TYPE, ATTR_INVOCATION_ID, ATTR_SCENARIO_ID, CorrelationCandidate,
    ObservedSpan, RedactionPolicy, SpanLink, TelemetryLimits,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, EntityTrait, PaginatorTrait,
    QueryFilter, Set, Statement,
};
use sea_orm_migration::MigratorTrait;
use serde_json::json;
use time::{Duration, OffsetDateTime};

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn correlated_run_is_tenant_safe_idempotent_and_immutable() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    Migrator::up(&database, None)
        .await
        .expect("migrations should apply");
    let repository = SeaOrmEvaluationRunRepository::new(database.clone());
    let organization_id = OrganizationId::new();
    let other_organization_id = OrganizationId::new();
    let policy_pack_id = PolicyPackId::new();
    let now = OffsetDateTime::now_utc();
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO organizations(id,name,created_at) VALUES ('{organization_id}','eval test',now()),('{other_organization_id}','other tenant',now())"
            ),
        ))
        .await
        .expect("test organizations should insert");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO policy_packs(id,organization_id,key,version,title,status,content_sha256,published_at,created_at) \
                 VALUES ('{policy_pack_id}','{organization_id}','test-pack',1,'Test','approved','policy-sha',now(),now())"
            ),
        ))
        .await
        .expect("test policy pack should insert");
    let rule_payload = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../fixtures/policies/refund-governance.import.json"
    ))
    .expect("policy fixture should parse")["rules"][0]
        .clone();
    policy_rules::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        organization_id: Set(organization_id.0),
        policy_pack_id: Set(policy_pack_id.0),
        rule_id: Set("refund_requires_prior_approval".to_owned()),
        rule_version: Set(1),
        position: Set(0),
        obligation_key: Set("INTERNAL-REFUND-004".to_owned()),
        severity: Set("critical".to_owned()),
        rule_payload: Set(rule_payload),
        created_at: Set(now),
    }
    .insert(&database)
    .await
    .expect("test policy rule should insert");
    targets::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        organization_id: Set(organization_id.0),
        key: Set("passive-workflow".to_owned()),
        version: Set("git:passive".to_owned()),
        driver_type: Set("http_text".to_owned()),
        endpoint: Set("http://127.0.0.1:1".to_owned()),
        capabilities: Set(json!({
            "telemetry_boundary": {
                "boundary_kind": "workflow_execution",
                "external_id_attributes": ["workflow.run.id"],
                "terminal_attribute": "workflow.completed",
                "default_policy_pack_id": policy_pack_id,
                "settle_seconds": 1,
                "idle_timeout_seconds": 60,
                "max_duration_seconds": 300,
                "conversation_id_is_task_boundary": false
            }
        })),
        created_at: Set(now),
    }
    .insert(&database)
    .await
    .expect("passive target should insert");

    let mut passive_span = observed_span_for_target(
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "eeeeeeeeeeeeeeee",
        "agent_start",
    );
    passive_span
        .attributes
        .insert("workflow.run.id".to_owned(), json!("workflow-passive-42"));
    let passive = IngestTelemetryBatch::new(
        SeaOrmPolicyPackRepository::new(database.clone()),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        RedactionPolicy::default(),
        TelemetryLimits::default(),
        1,
        60,
        300,
    )
    .execute(
        &governance_application::TelemetryIngestIdentity {
            organization_id,
            target_id: "passive-workflow".to_owned(),
        },
        vec![passive_span],
    )
    .await
    .expect("configured passive span should ingest");
    assert_eq!(passive.accepted, 1);
    let passive_run = repository
        .get_run_by_external_id(
            organization_id,
            "passive-workflow",
            RunBoundaryKind::WorkflowExecution,
            "workflow-passive-42",
        )
        .await
        .expect("passive run lookup should succeed")
        .expect("configured external ID should auto-create a run");
    assert_eq!(passive_run.state, EvaluationRunState::Collecting);

    let run = EvaluationRun {
        id: EvalRunId::new(),
        organization_id,
        target_id: "target-a".to_owned(),
        target_version: "git:test".to_owned(),
        policy_pack_id,
        policy_pack_key: "test-pack".to_owned(),
        policy_pack_version: 1,
        policy_content_sha256: "policy-sha".to_owned(),
        scenario_id: ScenarioId::new(),
        rule_ids: vec![],
        boundary_kind: RunBoundaryKind::WorkflowExecution,
        external_run_id: Some("workflow-42".to_owned()),
        primary_invocation_id: InvocationId::new(),
        state: EvaluationRunState::Collecting,
        completion_reason: None,
        terminal_state: None,
        verdict: None,
        trace_quality: None,
        evidence_sha256: None,
        span_count: 0,
        trace_count: 0,
        event_count: 0,
        created_at: now,
        updated_at: now,
        last_seen_at: None,
        settle_until: None,
        hard_deadline_at: now + Duration::minutes(5),
        finalized_at: None,
        completed_at: None,
    };
    repository
        .create_run(&run)
        .await
        .expect("run should insert");
    assert!(
        repository
            .get_run(other_organization_id, run.id)
            .await
            .expect("cross-tenant query should succeed")
            .is_none()
    );

    let approval = observed_span(
        &run,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaa",
        "human_approval_decision",
        vec![],
    );
    let approval_insert = span_insert(&run, approval.clone(), "approval-sha");
    assert_eq!(
        repository
            .insert_span(&approval_insert)
            .await
            .expect("approval should insert"),
        SpanInsertOutcome::Inserted
    );
    assert_eq!(
        repository
            .insert_span(&approval_insert)
            .await
            .expect("retry should be idempotent"),
        SpanInsertOutcome::Duplicate
    );
    let mut conflict = approval_insert.clone();
    conflict.sanitized_payload_sha256 = "different-sha".to_owned();
    assert_eq!(
        repository
            .insert_span(&conflict)
            .await
            .expect("conflict should classify"),
        SpanInsertOutcome::Conflict
    );
    let mut other_run = run.clone();
    other_run.id = EvalRunId::new();
    other_run.primary_invocation_id = InvocationId::new();
    other_run.external_run_id = None;
    repository
        .create_run(&other_run)
        .await
        .expect("second run should insert");
    let migrated_trace = observed_span(
        &other_run,
        &approval.trace_id,
        "dddddddddddddddd",
        "tool_call",
        vec![],
    );
    assert_eq!(
        repository
            .insert_span(&span_insert(
                &other_run,
                migrated_trace,
                "migrated-trace-sha"
            ))
            .await
            .expect("trace membership conflict should classify"),
        SpanInsertOutcome::Conflict
    );

    let terminal = observed_span(
        &run,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "bbbbbbbbbbbbbbbb",
        "final_output",
        vec![SpanLink {
            trace_id: approval.trace_id.clone(),
            span_id: approval.span_id.clone(),
        }],
    );
    repository
        .insert_span(&span_insert(&run, terminal, "terminal-sha"))
        .await
        .expect("terminal should insert");
    CompleteEvaluationRun::new(repository.clone(), repository.clone())
        .execute(CompletionRequest {
            organization_id,
            eval_run_id: run.id,
            reason: CompletionReason::Explicit,
            terminal_state: Some("completed".to_owned()),
            settle_seconds: 0,
        })
        .await
        .expect("run should complete collection");

    let finalizer = FinalizeEvaluationRun::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
    );
    let first = finalizer
        .execute(organization_id, run.id)
        .await
        .expect("run should finalize");
    let replay = finalizer
        .execute(organization_id, run.id)
        .await
        .expect("finalization should replay");
    assert_eq!(first.evidence_sha256, replay.evidence_sha256);
    assert_eq!(first.trace_ids.len(), 2);
    assert_eq!(first.events.len(), 2);
    assert!(
        repository
            .get_bundle(other_organization_id, run.id)
            .await
            .expect("cross-tenant evidence query should succeed")
            .is_none()
    );
    let evaluator = EvaluateFinalizedRun::new(
        SeaOrmPolicyPackRepository::new(database.clone()),
        repository.clone(),
        repository.clone(),
        SeaOrmEvaluationRepository::new(database.clone()),
    );
    let summary = evaluator
        .execute(organization_id, run.id)
        .await
        .expect("finalized evidence should evaluate");
    let summary_replay = evaluator
        .execute(organization_id, run.id)
        .await
        .expect("evaluation should replay");
    assert_eq!(summary, summary_replay);
    assert_eq!(
        rule_results::Entity::find()
            .filter(rule_results::Column::OrganizationId.eq(organization_id.0))
            .filter(rule_results::Column::EvalRunId.eq(run.id.0))
            .count(&database)
            .await
            .expect("rule result count should load"),
        1
    );
    assert_eq!(
        repository
            .get_run(organization_id, run.id)
            .await
            .expect("completed run should load")
            .expect("completed run should exist")
            .state,
        EvaluationRunState::Completed
    );

    let late = observed_span(
        &run,
        "cccccccccccccccccccccccccccccccc",
        "cccccccccccccccc",
        "tool_call",
        vec![],
    );
    assert_eq!(
        repository
            .insert_span(&span_insert(&run, late, "late-sha"))
            .await
            .expect("late span should persist as diagnostics"),
        SpanInsertOutcome::LateAfterFinalize
    );
    assert!(
        repository
            .claim_due(OffsetDateTime::now_utc(), 30)
            .await
            .expect("due job should claim")
            .is_some()
    );

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id IN ('{organization_id}','{other_organization_id}')"),
        ))
        .await
        .expect("test records should clean up");
}

fn observed_span(
    run: &EvaluationRun,
    trace_id: &str,
    span_id: &str,
    event_type: &str,
    links: Vec<SpanLink>,
) -> ObservedSpan {
    let now = OffsetDateTime::now_utc();
    ObservedSpan {
        trace_id: trace_id.to_owned(),
        span_id: span_id.to_owned(),
        parent_span_id: None,
        links,
        name: event_type.to_owned(),
        started_at: now,
        ended_at: Some(now + Duration::milliseconds(1)),
        attributes: BTreeMap::from([
            (ATTR_EVAL_RUN_ID.to_owned(), json!(run.id.to_string())),
            (
                ATTR_INVOCATION_ID.to_owned(),
                json!(run.primary_invocation_id.to_string()),
            ),
            (
                ATTR_SCENARIO_ID.to_owned(),
                json!(run.scenario_id.to_string()),
            ),
            (ATTR_EVENT_TYPE.to_owned(), json!(event_type)),
        ]),
        resource_attributes: BTreeMap::new(),
        instrumentation_scope: Some("integration-test".to_owned()),
        status: Some("ok".to_owned()),
    }
}

fn observed_span_for_target(trace_id: &str, span_id: &str, event_type: &str) -> ObservedSpan {
    let now = OffsetDateTime::now_utc();
    ObservedSpan {
        trace_id: trace_id.to_owned(),
        span_id: span_id.to_owned(),
        parent_span_id: None,
        links: Vec::new(),
        name: event_type.to_owned(),
        started_at: now,
        ended_at: Some(now + Duration::milliseconds(1)),
        attributes: BTreeMap::from([(ATTR_EVENT_TYPE.to_owned(), json!(event_type))]),
        resource_attributes: BTreeMap::new(),
        instrumentation_scope: Some("passive-integration-test".to_owned()),
        status: Some("ok".to_owned()),
    }
}

fn span_insert(run: &EvaluationRun, span: ObservedSpan, hash: &str) -> SpanInsert {
    SpanInsert {
        organization_id: run.organization_id,
        target_id: run.target_id.clone(),
        correlation: CorrelationCandidate {
            eval_run_id: Some(run.id),
            invocation_id: Some(run.primary_invocation_id),
            scenario_id: Some(run.scenario_id),
            external_run_id: run.external_run_id.clone(),
            terminal: false,
            terminal_state: None,
        },
        span,
        sanitized_payload_sha256: hash.to_owned(),
        received_at: OffsetDateTime::now_utc(),
    }
}
