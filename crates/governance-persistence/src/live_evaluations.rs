use std::collections::BTreeMap;

use async_trait::async_trait;
use governance_application::{
    ApplicationError, EvaluationRepository, LiveEvaluationRepository, StoredEvaluationRun,
};
use governance_domain::{
    EvalRunId, EvaluationSummary, EvidenceBundle, OrganizationId, TargetId, TraceDefect,
    TraceQualityStatus,
};
use governance_targets::RegisteredTarget;
use governance_telemetry::{NormalizationContext, finalize_evidence};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::entities::{eval_runs, normalized_events, rule_results, targets};
use crate::{
    ensure_organization, enum_string, repository_error, serialization_error, target_from_model,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StoredEvidenceMetadata {
    terminal_state: Option<String>,
    trace_quality: TraceQualityStatus,
    trace_defects: Vec<TraceDefect>,
    side_effects: Vec<serde_json::Value>,
    evidence_sha256: String,
}

impl From<&EvidenceBundle> for StoredEvidenceMetadata {
    fn from(evidence: &EvidenceBundle) -> Self {
        Self {
            terminal_state: evidence.terminal_state.clone(),
            trace_quality: evidence.trace_quality,
            trace_defects: evidence.trace_defects.clone(),
            side_effects: evidence.side_effects.clone(),
            evidence_sha256: evidence.evidence_sha256.clone(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct StoredLiveRunPayload {
    #[serde(flatten)]
    summary: EvaluationSummary,
    #[serde(default)]
    evidence: Option<StoredEvidenceMetadata>,
}

#[derive(Clone, Debug)]
pub struct SeaOrmEvaluationRepository {
    database: DatabaseConnection,
}

impl SeaOrmEvaluationRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl EvaluationRepository for SeaOrmEvaluationRepository {
    async fn save_summary(
        &self,
        organization_id: OrganizationId,
        summary: &EvaluationSummary,
    ) -> Result<(), ApplicationError> {
        let payload = serde_json::to_value(summary).map_err(serialization_error)?;
        eval_runs::ActiveModel {
            id: Set(summary.eval_run_id.0),
            organization_id: Set(organization_id.0),
            target_id: Set("unknown".to_owned()),
            policy_pack_key: Set("unknown".to_owned()),
            verdict: Set(enum_string(summary.verdict)?),
            summary: Set(payload),
            created_at: Set(OffsetDateTime::now_utc()),
            completed_at: Set(Some(OffsetDateTime::now_utc())),
        }
        .insert(&self.database)
        .await
        .map_err(repository_error)?;
        Ok(())
    }

    async fn get_summary(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<EvaluationSummary>, ApplicationError> {
        let model = eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::Id.eq(eval_run_id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?;
        model
            .map(|model| serde_json::from_value(model.summary).map_err(serialization_error))
            .transpose()
    }
}

#[async_trait]
impl LiveEvaluationRepository for SeaOrmEvaluationRepository {
    async fn save_run(&self, run: &StoredEvaluationRun) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        ensure_organization(&transaction, run.organization_id).await?;
        eval_runs::ActiveModel {
            id: Set(run.summary.eval_run_id.0),
            organization_id: Set(run.organization_id.0),
            target_id: Set(run.target_id.to_string()),
            policy_pack_key: Set(run.policy_pack_key.clone()),
            verdict: Set(enum_string(run.summary.verdict)?),
            summary: Set(serde_json::to_value(StoredLiveRunPayload {
                summary: run.summary.clone(),
                evidence: Some(StoredEvidenceMetadata::from(&run.evidence)),
            })
            .map_err(serialization_error)?),
            created_at: Set(run.created_at),
            completed_at: Set(Some(run.completed_at)),
        }
        .insert(&transaction)
        .await
        .map_err(repository_error)?;
        for event in &run.evidence.events {
            normalized_events::ActiveModel {
                id: Set(event.id.0),
                organization_id: Set(run.organization_id.0),
                eval_run_id: Set(run.summary.eval_run_id.0),
                invocation_id: Set(event.invocation_id.0),
                trace_id: Set(event.trace_id.clone()),
                span_id: Set(event.source_span_id.clone()),
                sequence: Set(i64::try_from(event.sequence).unwrap_or(i64::MAX)),
                event_type: Set(enum_string(event.event_type)?),
                name: Set(event.name.clone()),
                payload: Set(serde_json::to_value(event).map_err(serialization_error)?),
                started_at: Set(event.started_at),
            }
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        }
        for result in &run.summary.results {
            rule_results::ActiveModel {
                id: Set(result.id.0),
                organization_id: Set(run.organization_id.0),
                eval_run_id: Set(run.summary.eval_run_id.0),
                rule_id: Set(result.rule_id.clone()),
                severity: Set(enum_string(result.severity)?),
                status: Set(enum_string(result.status)?),
                payload: Set(serde_json::to_value(result).map_err(serialization_error)?),
                created_at: Set(run.completed_at),
            }
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)
    }

    async fn get_run(
        &self,
        organization_id: OrganizationId,
        eval_run_id: EvalRunId,
    ) -> Result<Option<StoredEvaluationRun>, ApplicationError> {
        let Some(model) = eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::Id.eq(eval_run_id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
        else {
            return Ok(None);
        };
        let mut runs = load_runs_from_models(&self.database, organization_id, vec![model]).await?;
        Ok(runs.pop())
    }

    async fn list_runs(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<StoredEvaluationRun>, ApplicationError> {
        let models = eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::TargetId.ne("unknown"))
            .order_by_desc(eval_runs::Column::CreatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        load_runs_from_models(&self.database, organization_id, models).await
    }

    async fn latest_by_target(
        &self,
        organization_id: OrganizationId,
    ) -> Result<BTreeMap<TargetId, StoredEvaluationRun>, ApplicationError> {
        let models = eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::TargetId.ne("unknown"))
            .order_by_desc(eval_runs::Column::CreatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        let mut latest_models = BTreeMap::new();
        for model in models {
            latest_models
                .entry(target_id_from_model(&model)?)
                .or_insert(model);
        }
        Ok(load_runs_from_models(
            &self.database,
            organization_id,
            latest_models.into_values().collect(),
        )
        .await?
        .into_iter()
        .map(|run| (run.target_id, run))
        .collect())
    }
}

async fn load_runs_from_models<C: ConnectionTrait>(
    connection: &C,
    organization_id: OrganizationId,
    models: Vec<eval_runs::Model>,
) -> Result<Vec<StoredEvaluationRun>, ApplicationError> {
    if models.is_empty() {
        return Ok(Vec::new());
    }
    let target_uuids = models
        .iter()
        .map(target_id_from_model)
        .map(|result| result.map(|target_id| target_id.0))
        .collect::<Result<Vec<_>, _>>()?;
    let targets_by_id = load_targets_by_id(connection, organization_id, target_uuids).await?;
    let run_ids = models.iter().map(|model| model.id).collect::<Vec<_>>();
    let mut events_by_run = load_events_by_run(connection, organization_id, run_ids).await?;
    let mut runs = Vec::with_capacity(models.len());
    for model in models {
        let target_id = target_id_from_model(&model)?;
        let target = targets_by_id.get(&target_id).cloned().ok_or_else(|| {
            ApplicationError::Repository("evaluation target no longer exists".to_owned())
        })?;
        let events = events_by_run.remove(&model.id).unwrap_or_default();
        runs.push(stored_run_from_model_with_events(
            organization_id,
            model,
            target,
            events,
        )?);
    }
    Ok(runs)
}

async fn load_targets_by_id<C: ConnectionTrait>(
    connection: &C,
    organization_id: OrganizationId,
    target_uuids: Vec<Uuid>,
) -> Result<BTreeMap<TargetId, RegisteredTarget>, ApplicationError> {
    let mut targets_by_id = BTreeMap::new();
    for model in targets::Entity::find()
        .filter(targets::Column::OrganizationId.eq(organization_id.0))
        .filter(targets::Column::Id.is_in(target_uuids))
        .all(connection)
        .await
        .map_err(repository_error)?
    {
        let target = target_from_model(model)?;
        targets_by_id.insert(target.id, target);
    }
    Ok(targets_by_id)
}

async fn load_events_by_run<C: ConnectionTrait>(
    connection: &C,
    organization_id: OrganizationId,
    run_ids: Vec<Uuid>,
) -> Result<BTreeMap<Uuid, Vec<governance_domain::NormalizedEvent>>, ApplicationError> {
    let event_models = normalized_events::Entity::find()
        .filter(normalized_events::Column::OrganizationId.eq(organization_id.0))
        .filter(normalized_events::Column::EvalRunId.is_in(run_ids))
        .order_by_asc(normalized_events::Column::EvalRunId)
        .order_by_asc(normalized_events::Column::Sequence)
        .all(connection)
        .await
        .map_err(repository_error)?;
    let mut events_by_run = BTreeMap::new();
    for event in event_models {
        events_by_run
            .entry(event.eval_run_id)
            .or_insert_with(Vec::new)
            .push(serde_json::from_value(event.payload).map_err(serialization_error)?);
    }
    Ok(events_by_run)
}

fn target_id_from_model(model: &eval_runs::Model) -> Result<TargetId, ApplicationError> {
    Uuid::parse_str(&model.target_id)
        .map(TargetId)
        .map_err(|_| {
            ApplicationError::Repository("evaluation target identifier is invalid".to_owned())
        })
}

fn stored_run_from_model_with_events(
    organization_id: OrganizationId,
    model: eval_runs::Model,
    target: RegisteredTarget,
    events: Vec<governance_domain::NormalizedEvent>,
) -> Result<StoredEvaluationRun, ApplicationError> {
    let stored_payload: StoredLiveRunPayload =
        serde_json::from_value(model.summary).map_err(serialization_error)?;
    let summary = stored_payload.summary;
    let context = events.first().map_or_else(
        || NormalizationContext {
            organization_id,
            eval_run_id: EvalRunId(model.id),
            invocation_id: governance_domain::InvocationId::new(),
            scenario_id: governance_domain::ScenarioId::new(),
        },
        |event| NormalizationContext {
            organization_id,
            eval_run_id: EvalRunId(model.id),
            invocation_id: event.invocation_id,
            scenario_id: event.scenario_id,
        },
    );
    let evidence = match stored_payload.evidence {
        None => {
            let terminal_state = events.iter().rev().find_map(|event| {
                event
                    .attributes
                    .get("terminal_state")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            });
            finalize_evidence(
                context,
                target.manifest.target_version.clone(),
                terminal_state,
                events,
                vec![],
            )
        }
        Some(metadata) => EvidenceBundle {
            organization_id: context.organization_id,
            eval_run_id: context.eval_run_id,
            invocation_id: context.invocation_id,
            scenario_id: context.scenario_id,
            target_version: target.manifest.target_version.clone(),
            terminal_state: metadata.terminal_state,
            events,
            side_effects: metadata.side_effects,
            trace_quality: metadata.trace_quality,
            trace_defects: metadata.trace_defects,
            evidence_sha256: metadata.evidence_sha256,
        },
    };
    Ok(StoredEvaluationRun {
        organization_id,
        target_id: target.id,
        target_name: target.name,
        target_version: target.manifest.target_version,
        policy_pack_key: model.policy_pack_key,
        created_at: model.created_at,
        completed_at: model.completed_at.unwrap_or(model.created_at),
        evidence,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use governance_domain::{RunVerdict, TraceQualityStatus};

    use super::*;

    #[test]
    fn live_run_payload_preserves_evidence_integrity_metadata() {
        let summary = EvaluationSummary {
            eval_run_id: EvalRunId::new(),
            verdict: RunVerdict::Pass,
            results: vec![],
            passed: 1,
            failed: 0,
            inconclusive: 0,
        };
        let payload = StoredLiveRunPayload {
            summary: summary.clone(),
            evidence: Some(StoredEvidenceMetadata {
                terminal_state: Some("completed".to_owned()),
                trace_quality: TraceQualityStatus::Complete,
                trace_defects: vec![],
                side_effects: vec![serde_json::json!({"kind": "refund"})],
                evidence_sha256: "abc123".to_owned(),
            }),
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        let legacy_summary: EvaluationSummary =
            serde_json::from_value(value.clone()).expect("summary readers remain compatible");
        let decoded: StoredLiveRunPayload =
            serde_json::from_value(value).expect("payload should deserialize");
        let metadata = decoded.evidence.expect("metadata should be present");

        assert_eq!(legacy_summary, summary);
        assert_eq!(metadata.terminal_state.as_deref(), Some("completed"));
        assert_eq!(
            metadata.side_effects,
            vec![serde_json::json!({"kind": "refund"})]
        );
        assert_eq!(metadata.evidence_sha256, "abc123");
    }
}
