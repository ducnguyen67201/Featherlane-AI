use std::collections::BTreeMap;

use governance_application::{
    CompleteEvaluationRun, CompletionRequest, ConfigureTargetTelemetry,
    ConfigureTargetTelemetryRequest, DurableJobRepository, EvaluateFinalizedRun,
    EvaluationRunRepository, EvidenceBundleRepository, FinalizeEvaluationRun, IngestTelemetryBatch,
    RotateTargetTelemetryIngestKey, RotateTelemetryIngestKey, SpanInsert, SpanInsertOutcome,
    TargetRepository, TelemetryIngestKeyRepository, TelemetrySpanRepository,
};
use governance_domain::{
    CompletionReason, EvalRunId, EvaluationRun, EvaluationRunState, InvocationId, OrganizationId,
    PolicyPackId, RunBoundaryKind, ScenarioId, TargetId,
};
use governance_migration::Migrator;
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository,
    SeaOrmTargetRepository,
    entities::{eval_runs, jobs, policy_rules, rule_results, targets},
};
use governance_targets::{
    CapabilityReport, DriverType, EvidenceMode, RegisteredTarget, TargetEnvironment,
    TargetManifest, TelemetryBoundaryConfig,
};
use governance_telemetry::{
    ATTR_EVAL_RUN_ID, ATTR_EVENT_TYPE, ATTR_EXTERNAL_RUN_ID, ATTR_INVOCATION_ID, ATTR_SCENARIO_ID,
    ATTR_TERMINAL_STATE, CorrelationCandidate, ObservedSpan, RedactionPolicy, SpanLink,
    TelemetryLimits,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, Database, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, Set, Statement,
};
use sea_orm_migration::MigratorTrait;
use serde_json::json;
use time::{Duration, OffsetDateTime};

static MIGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn apply_migrations(database: &DatabaseConnection) {
    let _guard = MIGRATION_LOCK.lock().await;
    Migrator::up(database, None)
        .await
        .expect("migrations should apply");
}

#[tokio::test(flavor = "multi_thread")]
async fn evaluation_list_skips_unmappable_legacy_rows() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    apply_migrations(&database).await;
    let organization_id = OrganizationId::new();
    let eval_run_id = EvalRunId::new();
    let mut cleanup = EvaluationCleanup {
        database: database.clone(),
        organization_ids: vec![organization_id],
        active: true,
    };
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO organizations(id,name,created_at) VALUES ('{organization_id}','legacy list test',now())"
            ),
        ))
        .await
        .expect("test organization should insert");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO eval_runs(id,organization_id,target_id,policy_pack_key,verdict,summary,created_at,completed_at,\
                    target_version,policy_pack_version,policy_content_sha256,scenario_id,primary_invocation_id,hard_deadline_at) \
                 VALUES ('{eval_run_id}','{organization_id}','legacy-target','removed-policy','pass','{{}}'::jsonb,now(),now(),\
                    'legacy',0,'legacy','{eval_run_id}','{eval_run_id}',now())"
            ),
        ))
        .await
        .expect("legacy evaluation should insert");

    let runs = SeaOrmEvaluationRunRepository::new(database.clone())
        .list_runs(organization_id)
        .await
        .expect("legacy rows should not break the evaluation list");
    assert!(runs.is_empty());

    cleanup.active = false;
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id = '{organization_id}'"),
        ))
        .await
        .expect("test records should clean up");
}

struct EvaluationCleanup {
    database: DatabaseConnection,
    organization_ids: Vec<OrganizationId>,
    active: bool,
}

impl Drop for EvaluationCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let database = self.database.clone();
        let ids = self
            .organization_ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                if let Err(error) = database
                    .execute_raw(Statement::from_string(
                        database.get_database_backend(),
                        format!("DELETE FROM organizations WHERE id IN ({ids})"),
                    ))
                    .await
                {
                    eprintln!("evaluation test cleanup failed: {error}");
                }
            });
        });
    }
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)]
async fn telemetry_boundary_update_preserves_target_connection_and_capability() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    apply_migrations(&database).await;
    let organization_id = OrganizationId::new();
    let mut cleanup = EvaluationCleanup {
        database: database.clone(),
        organization_ids: vec![organization_id],
        active: true,
    };
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO organizations(id,name,created_at) VALUES ('{organization_id}','target config test',now())"
            ),
        ))
        .await
        .expect("test organization should insert");
    let target_id = TargetId::new();
    let policy_pack_id = PolicyPackId::new();
    let checked_at = OffsetDateTime::now_utc();
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO policy_packs(id,organization_id,key,version,title,status,content_sha256,published_at,created_at) \
                 VALUES ('{policy_pack_id}','{organization_id}','target-policy',1,'Target policy','approved','target-policy-sha',now(),now())"
            ),
        ))
        .await
        .expect("target policy should insert");
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
        created_at: Set(checked_at),
    }
    .insert(&database)
    .await
    .expect("target policy rule should insert");
    let target = RegisteredTarget {
        id: target_id,
        organization_id,
        name: "Trace Agent".to_owned(),
        environment: TargetEnvironment::Staging,
        manifest: TargetManifest {
            schema_version: "1.0".to_owned(),
            target_id: "trace-agent".to_owned(),
            target_version: "git:before".to_owned(),
            driver_type: DriverType::Webhook,
            endpoint: "http://127.0.0.1:8099/webhook".to_owned(),
            reset_endpoint: Some("http://127.0.0.1:8099/reset".to_owned()),
            status_endpoint: None,
            terminal_response_key: None,
            auth_secret_ref: Some("TRACE_AGENT_TOKEN".to_owned()),
            timeout_seconds: 45,
            evidence_mode: EvidenceMode::Inline,
            otlp_required: false,
            production_credentials_allowed: false,
            telemetry_boundary: TelemetryBoundaryConfig::default(),
        },
        capability: CapabilityReport {
            target_id: "trace-agent".to_owned(),
            reachable: true,
            reset_supported: true,
            trace_context_supported: true,
            issues: vec!["fixture capability".to_owned()],
            checked_at,
        },
        created_at: checked_at,
    };
    let targets = SeaOrmTargetRepository::new(database.clone());
    targets
        .create(&target)
        .await
        .expect("target should persist");
    let configured = TelemetryBoundaryConfig {
        boundary_kind: RunBoundaryKind::AgentTask,
        external_id_attributes: vec!["agent.session.id".to_owned()],
        terminal_attribute: Some("agent.session.finished".to_owned()),
        default_policy_pack_id: Some(policy_pack_id),
        settle_seconds: 4,
        idle_timeout_seconds: Some(90),
        max_duration_seconds: Some(600),
        conversation_id_is_task_boundary: false,
    };
    let updated = ConfigureTargetTelemetry::new(
        targets.clone(),
        SeaOrmPolicyPackRepository::new(database.clone()),
    )
    .execute(ConfigureTargetTelemetryRequest {
        organization_id,
        target_id,
        config: configured.clone(),
    })
    .await
    .expect("telemetry boundary should update");
    assert_eq!(updated.manifest.telemetry_boundary, configured);
    assert_eq!(updated.name, target.name);
    assert_eq!(updated.manifest.target_id, target.manifest.target_id);
    assert_eq!(
        updated.manifest.target_version,
        target.manifest.target_version
    );
    assert_eq!(updated.manifest.endpoint, target.manifest.endpoint);
    assert_eq!(
        updated.manifest.auth_secret_ref,
        target.manifest.auth_secret_ref
    );
    assert_eq!(updated.capability, target.capability);
    let mut invalid_config = configured.clone();
    invalid_config.default_policy_pack_id = Some(PolicyPackId::new());
    assert!(matches!(
        ConfigureTargetTelemetry::new(
            targets.clone(),
            SeaOrmPolicyPackRepository::new(database.clone()),
        )
        .execute(ConfigureTargetTelemetryRequest {
            organization_id,
            target_id,
            config: invalid_config,
        })
        .await,
        Err(governance_application::ApplicationError::NotFound(_))
    ));
    assert_eq!(
        targets
            .get(organization_id, target_id)
            .await
            .expect("target should reload")
            .expect("target should still exist")
            .manifest
            .telemetry_boundary,
        configured
    );
    let keys = SeaOrmEvaluationRunRepository::new(database.clone());
    let rotated = RotateTargetTelemetryIngestKey::new(targets, keys.clone())
        .execute(organization_id, target_id, None)
        .await
        .expect("target-scoped ingest key should rotate");
    assert_eq!(rotated.key.target_id, target.manifest.target_id);
    assert_eq!(
        keys.resolve_key(
            &rotated.key.token_prefix,
            &rotated.key.token_sha256,
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("target-scoped key should resolve")
        .expect("target-scoped key should be active")
        .target_id,
        "trace-agent"
    );

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id = '{organization_id}'"),
        ))
        .await
        .expect("test records should clean up");
    cleanup.active = false;
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)]
async fn correlated_run_is_tenant_safe_idempotent_and_immutable() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    apply_migrations(&database).await;
    let repository = SeaOrmEvaluationRunRepository::new(database.clone());
    let organization_id = OrganizationId::new();
    let other_organization_id = OrganizationId::new();
    let policy_pack_id = PolicyPackId::new();
    let mut cleanup = EvaluationCleanup {
        database: database.clone(),
        organization_ids: vec![organization_id, other_organization_id],
        active: true,
    };
    let now = OffsetDateTime::now_utc();
    let now = now
        .replace_nanosecond((now.nanosecond() / 1_000) * 1_000)
        .expect("microsecond precision should be valid");
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
    assert_eq!(
        jobs::Entity::find()
            .filter(jobs::Column::OrganizationId.eq(organization_id.0))
            .filter(jobs::Column::DedupeKey.eq(format!("timeout:{}", passive_run.id)))
            .count(&database)
            .await
            .expect("timeout job count should load"),
        1
    );
    let finalize_job = governance_application::DurableJob {
        organization_id,
        eval_run_id: passive_run.id,
        kind: "finalize_evidence".to_owned(),
        dedupe_key: format!("finalize:{}", passive_run.id),
        available_at: now + Duration::seconds(60),
    };
    repository
        .enqueue(&finalize_job)
        .await
        .expect("finalization job should enqueue");
    repository
        .enqueue(&governance_application::DurableJob {
            available_at: now + Duration::seconds(30),
            ..finalize_job.clone()
        })
        .await
        .expect("earlier duplicate job should coalesce");
    assert_eq!(
        jobs::Entity::find()
            .filter(jobs::Column::OrganizationId.eq(organization_id.0))
            .filter(jobs::Column::DedupeKey.eq(&finalize_job.dedupe_key))
            .one(&database)
            .await
            .expect("coalesced finalization job should load")
            .expect("coalesced finalization job should exist")
            .available_at,
        finalize_job.available_at
    );

    let initial_key = RotateTelemetryIngestKey::new(repository.clone())
        .execute(organization_id, "passive-workflow".to_owned(), None)
        .await
        .expect("initial ingest key should rotate");
    let left_rotation_service = RotateTelemetryIngestKey::new(repository.clone());
    let right_rotation_service = RotateTelemetryIngestKey::new(repository.clone());
    let (left_rotation, right_rotation) = tokio::join!(
        left_rotation_service.execute(organization_id, "passive-workflow".to_owned(), None,),
        right_rotation_service.execute(organization_id, "passive-workflow".to_owned(), None,)
    );
    left_rotation.expect("left concurrent key rotation should succeed");
    right_rotation.expect("right concurrent key rotation should succeed");
    assert_eq!(
        governance_persistence::entities::telemetry_ingest_keys::Entity::find()
            .filter(
                governance_persistence::entities::telemetry_ingest_keys::Column::OrganizationId
                    .eq(organization_id.0),
            )
            .filter(
                governance_persistence::entities::telemetry_ingest_keys::Column::TargetId
                    .eq("passive-workflow"),
            )
            .filter(
                governance_persistence::entities::telemetry_ingest_keys::Column::Status
                    .eq("active"),
            )
            .count(&database)
            .await
            .expect("active ingest keys should count"),
        1
    );
    assert!(
        repository
            .resolve_key(
                &initial_key.key.token_prefix,
                &initial_key.key.token_sha256,
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("revoked initial key should resolve safely")
            .is_none()
    );

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

    let mut stale_collector = run.clone();
    stale_collector.id = EvalRunId::new();
    stale_collector.primary_invocation_id = InvocationId::new();
    stale_collector.external_run_id = None;
    stale_collector.state = EvaluationRunState::Created;
    repository
        .create_run(&stale_collector)
        .await
        .expect("race fixture should insert");
    let mut terminal_transition = stale_collector.clone();
    terminal_transition
        .begin_settling(
            CompletionReason::TerminalEvent,
            Some("completed".to_owned()),
            now + Duration::seconds(10),
            now + Duration::milliseconds(1),
        )
        .expect("terminal transition should be valid");
    assert!(
        repository
            .update_run(
                &terminal_transition,
                stale_collector.state,
                stale_collector.updated_at,
            )
            .await
            .expect("terminal compare-and-set should succeed")
    );
    stale_collector
        .transition_to(
            EvaluationRunState::Collecting,
            now + Duration::milliseconds(2),
        )
        .expect("collector transition should be valid in isolation");
    assert!(
        !repository
            .update_run(&stale_collector, EvaluationRunState::Created, now,)
            .await
            .expect("stale compare-and-set should be rejected")
    );
    assert_eq!(
        repository
            .get_run(organization_id, stale_collector.id)
            .await
            .expect("race fixture should load")
            .expect("race fixture should exist")
            .state,
        EvaluationRunState::Settling
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

    let mut interrupted_finalization = repository
        .get_run(organization_id, run.id)
        .await
        .expect("settling run should load")
        .expect("settling run should exist");
    let expected_updated_at = interrupted_finalization.updated_at;
    interrupted_finalization
        .transition_to(EvaluationRunState::Finalizing, OffsetDateTime::now_utc())
        .expect("finalizing transition should be valid");
    assert!(
        repository
            .update_run(
                &interrupted_finalization,
                EvaluationRunState::Settling,
                expected_updated_at,
            )
            .await
            .expect("interrupted finalization fixture should persist")
    );

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
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "UPDATE eval_runs SET state = 'finalizing', trace_count = 0, event_count = 0, \
                 trace_quality = NULL, evidence_sha256 = NULL, finalized_at = NULL, updated_at = now() \
                 WHERE organization_id = '{organization_id}' AND id = '{}'",
                run.id
            ),
        ))
        .await
        .expect("post-bundle interruption fixture should persist");
    let replay = finalizer
        .execute(organization_id, run.id)
        .await
        .expect("finalization should replay");
    assert_eq!(first.evidence_sha256, replay.evidence_sha256);
    assert_eq!(first.trace_ids.len(), 2);
    assert_eq!(first.events.len(), 2);
    let recovered_run = repository
        .get_run(organization_id, run.id)
        .await
        .expect("recovered run should load")
        .expect("recovered run should exist");
    assert_eq!(recovered_run.state, EvaluationRunState::Evaluating);
    assert_eq!(recovered_run.trace_count, 2);
    assert_eq!(recovered_run.event_count, 2);
    assert_eq!(recovered_run.trace_quality, Some(first.trace_quality));
    assert_eq!(
        recovered_run.evidence_sha256.as_deref(),
        Some(first.evidence_sha256.as_str())
    );
    assert_eq!(
        recovered_run
            .finalized_at
            .map(|timestamp| timestamp.unix_timestamp_nanos() / 1_000),
        first
            .finalized_at
            .map(|timestamp| timestamp.unix_timestamp_nanos() / 1_000)
    );
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
    let claimed_job = repository
        .claim_due(OffsetDateTime::now_utc(), 30)
        .await
        .expect("due job should claim")
        .expect("a due job should exist");
    assert_eq!(claimed_job.eval_run_id, run.id);
    assert_eq!(claimed_job.kind, "finalize_evaluation_run");
    assert!(
        repository
            .reschedule_job(
                claimed_job.id,
                claimed_job.attempts,
                OffsetDateTime::UNIX_EPOCH,
            )
            .await
            .expect("current lease reschedule should persist")
    );
    let claimed_job = repository
        .claim_due(OffsetDateTime::now_utc(), 30)
        .await
        .expect("rescheduled job should claim")
        .expect("rescheduled job should be due");
    assert_eq!(claimed_job.eval_run_id, run.id);
    assert_eq!(claimed_job.kind, "finalize_evaluation_run");
    assert_eq!(claimed_job.attempts, 1);
    assert!(
        !repository
            .complete_job(
                claimed_job.id,
                claimed_job.attempts.saturating_add(1),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("stale lease completion should be rejected safely")
    );
    assert!(
        repository
            .complete_job(
                claimed_job.id,
                claimed_job.attempts,
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("current lease completion should persist")
    );

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id IN ('{organization_id}','{other_organization_id}')"),
        ))
        .await
        .expect("test records should clean up");
    cleanup.active = false;
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)]
async fn passive_terminal_session_evaluates_two_traces_once() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    apply_migrations(&database).await;
    let organization_id = OrganizationId::new();
    let policy_pack_id = PolicyPackId::new();
    let mut cleanup = EvaluationCleanup {
        database: database.clone(),
        organization_ids: vec![organization_id],
        active: true,
    };
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO organizations(id,name,created_at) VALUES ('{organization_id}','passive terminal test',now())"
            ),
        ))
        .await
        .expect("test organization should insert");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO policy_packs(id,organization_id,key,version,title,status,content_sha256,published_at,created_at) \
                 VALUES ('{policy_pack_id}','{organization_id}','passive-pack',1,'Passive','approved','passive-policy-sha',now(),now())"
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
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(&database)
    .await
    .expect("test policy rule should insert");
    targets::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        organization_id: Set(organization_id.0),
        key: Set("passive-terminal-agent".to_owned()),
        version: Set("git:passive-terminal".to_owned()),
        driver_type: Set("http_text".to_owned()),
        endpoint: Set("http://127.0.0.1:1".to_owned()),
        capabilities: Set(json!({
            "telemetry_boundary": {
                "boundary_kind": "workflow_execution",
                "external_id_attributes": [ATTR_EXTERNAL_RUN_ID],
                "terminal_attribute": "workflow.completed",
                "default_policy_pack_id": policy_pack_id,
                "settle_seconds": 0,
                "idle_timeout_seconds": 60,
                "max_duration_seconds": 300,
                "conversation_id_is_task_boundary": false
            }
        })),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(&database)
    .await
    .expect("passive target should insert");

    let mut approval = observed_span_for_target(
        "11111111111111111111111111111111",
        "1111111111111111",
        "human_approval_decision",
    );
    approval
        .resource_attributes
        .insert(ATTR_EXTERNAL_RUN_ID.to_owned(), json!("session-42"));
    approval
        .attributes
        .insert("decision".to_owned(), json!("approved"));
    approval
        .attributes
        .insert("governance.actor.id".to_owned(), json!("fixture-approver"));
    approval
        .attributes
        .insert("governance.actor.type".to_owned(), json!("human"));
    let mut tool = observed_span_for_target(
        "22222222222222222222222222222222",
        "2222222222222221",
        "tool_call",
    );
    tool.resource_attributes
        .insert(ATTR_EXTERNAL_RUN_ID.to_owned(), json!("session-42"));
    tool.attributes.insert(
        "governance.input".to_owned(),
        json!(r#"{"amount":700,"currency":"USD"}"#),
    );
    tool.links.push(SpanLink {
        trace_id: approval.trace_id.clone(),
        span_id: approval.span_id.clone(),
    });
    let mut terminal = observed_span_for_target(
        "22222222222222222222222222222222",
        "2222222222222222",
        "final_output",
    );
    terminal.parent_span_id = Some(tool.span_id.clone());
    terminal
        .resource_attributes
        .insert(ATTR_EXTERNAL_RUN_ID.to_owned(), json!("session-42"));
    terminal
        .attributes
        .insert("workflow.completed".to_owned(), json!(true));
    terminal
        .attributes
        .insert(ATTR_TERMINAL_STATE.to_owned(), json!("completed"));

    let repository = SeaOrmEvaluationRunRepository::new(database.clone());
    let ingest = IngestTelemetryBatch::new(
        SeaOrmPolicyPackRepository::new(database.clone()),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
        RedactionPolicy::default(),
        TelemetryLimits::default(),
        10,
        300,
        3_600,
    );
    let identity = governance_application::TelemetryIngestIdentity {
        organization_id,
        target_id: "passive-terminal-agent".to_owned(),
    };
    ingest
        .execute(&identity, vec![approval])
        .await
        .expect("approval trace should ingest");
    let terminal_outcome = ingest
        .execute(&identity, vec![tool, terminal.clone()])
        .await
        .expect("execution and terminal trace should ingest");
    assert_eq!(terminal_outcome.accepted, 2);

    let run = repository
        .get_run_by_external_id(
            organization_id,
            "passive-terminal-agent",
            RunBoundaryKind::WorkflowExecution,
            "session-42",
        )
        .await
        .expect("automatic run lookup should succeed")
        .expect("automatic run should exist");
    assert_eq!(run.state, EvaluationRunState::Settling);
    assert_eq!(run.completion_reason, Some(CompletionReason::TerminalEvent));
    assert_eq!(run.policy_pack_id, policy_pack_id);
    assert_eq!(run.target_version, "git:passive-terminal");
    assert_eq!(
        eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::TargetId.eq("passive-terminal-agent"))
            .filter(eval_runs::Column::ExternalRunId.eq("session-42"))
            .count(&database)
            .await
            .expect("automatic run count should load"),
        1
    );
    assert_eq!(
        jobs::Entity::find()
            .filter(jobs::Column::OrganizationId.eq(organization_id.0))
            .filter(jobs::Column::DedupeKey.eq(format!("finalize:{}", run.id)))
            .count(&database)
            .await
            .expect("finalize job count should load"),
        1
    );

    let finalizer = FinalizeEvaluationRun::new(
        repository.clone(),
        repository.clone(),
        repository.clone(),
        repository.clone(),
    );
    let evidence = finalizer
        .execute(organization_id, run.id)
        .await
        .expect("automatic run should finalize");
    assert_eq!(evidence.trace_ids.len(), 2);
    assert_eq!(evidence.events.len(), 3);
    let evaluator = EvaluateFinalizedRun::new(
        SeaOrmPolicyPackRepository::new(database.clone()),
        repository.clone(),
        repository.clone(),
        SeaOrmEvaluationRepository::new(database.clone()),
    );
    let summary = evaluator
        .execute(organization_id, run.id)
        .await
        .expect("automatic run should evaluate");
    assert_eq!(summary.results.len(), 1);
    assert_eq!(
        repository
            .get_run(organization_id, run.id)
            .await
            .expect("completed run should load")
            .expect("completed run should exist")
            .state,
        EvaluationRunState::Completed
    );

    let retry = ingest
        .execute(&identity, vec![terminal])
        .await
        .expect("terminal retry should remain idempotent");
    assert_eq!(retry.duplicates, 1);
    assert_eq!(
        eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::TargetId.eq("passive-terminal-agent"))
            .count(&database)
            .await
            .expect("run count after retry should load"),
        1
    );
    assert_eq!(
        evaluator
            .execute(organization_id, run.id)
            .await
            .expect("evaluation replay should succeed"),
        summary
    );

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id = '{organization_id}'"),
        ))
        .await
        .expect("test records should clean up");
    cleanup.active = false;
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
        max_spans_per_run: 100_000,
    }
}
