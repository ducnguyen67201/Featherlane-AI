//! `SeaORM` persistence adapters. ORM models never cross this crate boundary.

pub mod entities;
mod evaluation_runs;
mod policy_imports;

pub use evaluation_runs::SeaOrmEvaluationRunRepository;
pub use policy_imports::SeaOrmPolicyImportRepository;

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use governance_application::{ApplicationError, EvaluationRepository, PolicyPackRepository};
use governance_domain::{
    CompiledRule, EvalRunId, EvaluationSummary, OrganizationId, PolicyBundle, PolicyPack,
    PolicyPackApproval, PolicyPackId, PolicyPackStatusChange, ReviewStatus,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait, sea_query::OnConflict,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::entities::{
    eval_runs, obligations, organizations, policy_pack_sources, policy_packs, policy_reviews,
    policy_rules, rule_results, sources,
};

#[derive(Clone, Debug)]
pub struct SeaOrmPolicyPackRepository {
    database: DatabaseConnection,
}

impl SeaOrmPolicyPackRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }

    /// Counts persisted source links for a policy pack.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub async fn source_count(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
    ) -> Result<usize, ApplicationError> {
        use sea_orm::PaginatorTrait;

        let count = policy_pack_sources::Entity::find()
            .filter(policy_pack_sources::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_pack_sources::Column::PolicyPackId.eq(id.0))
            .count(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Returns source counts for every pack in an organization with one query.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub async fn source_counts(
        &self,
        organization_id: OrganizationId,
    ) -> Result<BTreeMap<PolicyPackId, usize>, ApplicationError> {
        let links = policy_pack_sources::Entity::find()
            .filter(policy_pack_sources::Column::OrganizationId.eq(organization_id.0))
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        let mut counts = BTreeMap::new();
        for link in links {
            *counts.entry(PolicyPackId(link.policy_pack_id)).or_insert(0) += 1;
        }
        Ok(counts)
    }

    /// Returns the latest persisted policy reviewer for each pack.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub async fn latest_reviewers(
        &self,
        organization_id: OrganizationId,
    ) -> Result<BTreeMap<PolicyPackId, String>, ApplicationError> {
        let reviews = policy_reviews::Entity::find()
            .filter(policy_reviews::Column::OrganizationId.eq(organization_id.0))
            .order_by_desc(policy_reviews::Column::ReviewedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        let mut reviewers = BTreeMap::new();
        for review in reviews {
            reviewers
                .entry(PolicyPackId(review.policy_pack_id))
                .or_insert(review.reviewer_id);
        }
        Ok(reviewers)
    }
}

#[async_trait]
impl PolicyPackRepository for SeaOrmPolicyPackRepository {
    async fn get(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
    ) -> Result<Option<PolicyPack>, ApplicationError> {
        let model = policy_packs::Entity::find()
            .filter(policy_packs::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_packs::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?;
        let Some(model) = model else {
            return Ok(None);
        };
        Ok(Some(load_pack(&self.database, model).await?))
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PolicyPack>, ApplicationError> {
        let models = policy_packs::Entity::find()
            .filter(policy_packs::Column::OrganizationId.eq(organization_id.0))
            .order_by_desc(policy_packs::Column::CreatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let pack_ids: Vec<Uuid> = models.iter().map(|model| model.id).collect();
        let rule_models = policy_rules::Entity::find()
            .filter(policy_rules::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_rules::Column::PolicyPackId.is_in(pack_ids))
            .order_by_asc(policy_rules::Column::Position)
            .all(&self.database)
            .await
            .map_err(repository_error)?;
        let mut rules_by_pack: BTreeMap<Uuid, Vec<CompiledRule>> = BTreeMap::new();
        for model in rule_models {
            rules_by_pack
                .entry(model.policy_pack_id)
                .or_default()
                .push(rule_from_model(model)?);
        }

        models
            .into_iter()
            .map(|model| {
                let rules = rules_by_pack.remove(&model.id).unwrap_or_default();
                pack_from_model(model, rules)
            })
            .collect()
    }

    async fn save_bundle(&self, bundle: &PolicyBundle) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        persist_bundle(&transaction, bundle).await?;
        transaction.commit().await.map_err(repository_error)
    }

    async fn approve(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        approval: &PolicyPackApproval,
    ) -> Result<PolicyPack, ApplicationError> {
        if approval.reviewer_id.trim().is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "policy approval requires a reviewer identifier".to_owned(),
            ));
        }
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = policy_packs::Entity::find()
            .filter(policy_packs::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_packs::Column::Id.eq(id.0))
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        if model.status != "draft" {
            return Err(ApplicationError::InvalidRequest(
                "only a draft policy version can be approved".to_owned(),
            ));
        }

        let source_ids: Vec<Uuid> = policy_pack_sources::Entity::find()
            .filter(policy_pack_sources::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_pack_sources::Column::PolicyPackId.eq(id.0))
            .all(&transaction)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(|link| link.source_id)
            .collect();
        if source_ids.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "a policy pack needs at least one persisted source before approval".to_owned(),
            ));
        }
        let obligation_models = obligations::Entity::find()
            .filter(obligations::Column::OrganizationId.eq(organization_id.0))
            .filter(obligations::Column::SourceId.is_in(source_ids))
            .all(&transaction)
            .await
            .map_err(repository_error)?;
        if obligation_models.is_empty()
            || obligation_models
                .iter()
                .any(|obligation| obligation.review_status != "approved")
        {
            return Err(ApplicationError::InvalidRequest(
                "every extracted obligation must have a persisted human approval".to_owned(),
            ));
        }

        let mut active: policy_packs::ActiveModel = model.into();
        active.status = Set("approved".to_owned());
        active.published_at = Set(Some(approval.approved_at));
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        policy_reviews::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(organization_id.0),
            policy_pack_id: Set(id.0),
            status: Set("approved".to_owned()),
            reviewer_id: Set(approval.reviewer_id.clone()),
            notes: Set(approval.notes.clone()),
            reviewed_at: Set(approval.approved_at),
        }
        .insert(&transaction)
        .await
        .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn disable(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        change: &PolicyPackStatusChange,
    ) -> Result<PolicyPack, ApplicationError> {
        transition_pack_status(
            &self.database,
            organization_id,
            id,
            "approved",
            "disabled",
            None,
            change,
        )
        .await?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn enable(
        &self,
        organization_id: OrganizationId,
        id: PolicyPackId,
        change: &PolicyPackStatusChange,
    ) -> Result<PolicyPack, ApplicationError> {
        transition_pack_status(
            &self.database,
            organization_id,
            id,
            "disabled",
            "approved",
            Some(change.changed_at),
            change,
        )
        .await?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }
}

async fn transition_pack_status(
    database: &DatabaseConnection,
    organization_id: OrganizationId,
    id: PolicyPackId,
    expected: &str,
    next: &str,
    published_at: Option<OffsetDateTime>,
    change: &PolicyPackStatusChange,
) -> Result<(), ApplicationError> {
    if change.actor_id.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "policy status changes require an actor identifier".to_owned(),
        ));
    }
    let transaction = database.begin().await.map_err(repository_error)?;
    let model = policy_packs::Entity::find()
        .filter(policy_packs::Column::OrganizationId.eq(organization_id.0))
        .filter(policy_packs::Column::Id.eq(id.0))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(repository_error)?
        .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
    if model.status != expected {
        return Err(ApplicationError::Conflict(format!(
            "only a {expected} policy pack can transition to {next}"
        )));
    }
    let mut active: policy_packs::ActiveModel = model.into();
    active.status = Set(next.to_owned());
    active.published_at = Set(published_at);
    active
        .update(&transaction)
        .await
        .map_err(repository_error)?;
    policy_reviews::ActiveModel {
        id: Set(Uuid::now_v7()),
        organization_id: Set(organization_id.0),
        policy_pack_id: Set(id.0),
        status: Set(next.to_owned()),
        reviewer_id: Set(change.actor_id.clone()),
        notes: Set(change.notes.clone()),
        reviewed_at: Set(change.changed_at),
    }
    .insert(&transaction)
    .await
    .map_err(repository_error)?;
    transaction.commit().await.map_err(repository_error)
}

pub(crate) async fn persist_bundle<C: ConnectionTrait>(
    connection: &C,
    bundle: &PolicyBundle,
) -> Result<(), ApplicationError> {
    validate_policy_bundle(bundle)?;
    ensure_organization(connection, bundle.pack.organization_id).await?;
    ensure_version_available(connection, &bundle.pack).await?;
    insert_pack(connection, &bundle.pack).await?;
    insert_rules(connection, &bundle.pack).await?;
    insert_sources(connection, bundle).await?;
    insert_obligations(connection, bundle).await
}

fn validate_policy_bundle(bundle: &PolicyBundle) -> Result<(), ApplicationError> {
    if bundle.pack.rules.is_empty() || bundle.sources.is_empty() || bundle.obligations.is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "a persisted policy aggregate needs rules, sources, and obligations".to_owned(),
        ));
    }
    if bundle.pack.status != ReviewStatus::Draft || bundle.pack.published_at.is_some() {
        return Err(ApplicationError::InvalidRequest(
            "new policy aggregates must be persisted as unpublished drafts".to_owned(),
        ));
    }
    if bundle
        .sources
        .iter()
        .any(|source| source.organization_id != bundle.pack.organization_id)
        || bundle
            .obligations
            .iter()
            .any(|obligation| obligation.organization_id != bundle.pack.organization_id)
    {
        return Err(ApplicationError::InvalidRequest(
            "policy aggregate records must belong to one organization".to_owned(),
        ));
    }
    let source_ids: BTreeSet<_> = bundle.sources.iter().map(|source| source.id).collect();
    if bundle
        .obligations
        .iter()
        .any(|obligation| !source_ids.contains(&obligation.source_id))
    {
        return Err(ApplicationError::InvalidRequest(
            "every obligation must reference a source in the same policy aggregate".to_owned(),
        ));
    }
    let obligation_keys: BTreeSet<_> = bundle
        .obligations
        .iter()
        .map(|obligation| obligation.key.as_str())
        .collect();
    if obligation_keys.len() != bundle.obligations.len() {
        return Err(ApplicationError::InvalidRequest(
            "obligation keys must be unique within a policy aggregate".to_owned(),
        ));
    }
    if bundle
        .pack
        .rules
        .iter()
        .any(|rule| !obligation_keys.contains(rule.obligation_key.as_str()))
    {
        return Err(ApplicationError::InvalidRequest(
            "every rule must reference an obligation in the same policy aggregate".to_owned(),
        ));
    }
    Ok(())
}

async fn ensure_organization<C: ConnectionTrait>(
    connection: &C,
    organization_id: OrganizationId,
) -> Result<(), ApplicationError> {
    organizations::Entity::insert(organizations::ActiveModel {
        id: Set(organization_id.0),
        name: Set("Featherlane workspace".to_owned()),
        created_at: Set(OffsetDateTime::now_utc()),
    })
    .on_conflict(
        OnConflict::column(organizations::Column::Id)
            .do_nothing()
            .to_owned(),
    )
    .exec_without_returning(connection)
    .await
    .map_err(repository_error)?;
    Ok(())
}

async fn ensure_version_available<C: ConnectionTrait>(
    connection: &C,
    pack: &PolicyPack,
) -> Result<(), ApplicationError> {
    let duplicate = policy_packs::Entity::find()
        .filter(policy_packs::Column::OrganizationId.eq(pack.organization_id.0))
        .filter(policy_packs::Column::Key.eq(&pack.key))
        .filter(policy_packs::Column::Version.eq(i32_from_u32(pack.version)))
        .one(connection)
        .await
        .map_err(repository_error)?;
    if duplicate.is_some() {
        return Err(ApplicationError::InvalidRequest(format!(
            "policy pack {} version {} already exists and is immutable",
            pack.key, pack.version
        )));
    }
    Ok(())
}

async fn insert_sources<C: ConnectionTrait>(
    connection: &C,
    bundle: &PolicyBundle,
) -> Result<(), ApplicationError> {
    for source in &bundle.sources {
        let payload = serde_json::to_value(source).map_err(serialization_error)?;
        sources::ActiveModel {
            id: Set(source.id.0),
            organization_id: Set(source.organization_id.0),
            source_type: Set(enum_string(source.source_type)?),
            title: Set(source.title.clone()),
            jurisdiction: Set(source.jurisdiction.clone()),
            content_sha256: Set(source.content_sha256.clone()),
            confidence: Set(enum_string(source.confidence)?),
            payload: Set(payload),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(connection)
        .await
        .map_err(repository_error)?;
        policy_pack_sources::ActiveModel {
            policy_pack_id: Set(bundle.pack.id.0),
            source_id: Set(source.id.0),
            organization_id: Set(bundle.pack.organization_id.0),
        }
        .insert(connection)
        .await
        .map_err(repository_error)?;
    }
    Ok(())
}

async fn insert_obligations<C: ConnectionTrait>(
    connection: &C,
    bundle: &PolicyBundle,
) -> Result<(), ApplicationError> {
    for obligation in &bundle.obligations {
        let review = obligation.review.as_ref();
        let review_status = match review {
            Some(approval) => enum_string(approval.status)?,
            None => "draft".to_owned(),
        };
        obligations::ActiveModel {
            id: Set(obligation.id.0),
            organization_id: Set(obligation.organization_id.0),
            source_id: Set(obligation.source_id.0),
            key: Set(obligation.key.clone()),
            statement: Set(obligation.statement.clone()),
            obligation_payload: Set(serde_json::to_value(obligation).map_err(serialization_error)?),
            review_status: Set(review_status),
            reviewer_id: Set(review.map(|approval| approval.reviewer_id.clone())),
            reviewed_at: Set(review.map(|approval| approval.reviewed_at)),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(connection)
        .await
        .map_err(repository_error)?;
    }
    Ok(())
}

async fn insert_pack<C: ConnectionTrait>(
    connection: &C,
    pack: &PolicyPack,
) -> Result<(), ApplicationError> {
    policy_packs::ActiveModel {
        id: Set(pack.id.0),
        organization_id: Set(pack.organization_id.0),
        key: Set(pack.key.clone()),
        version: Set(i32_from_u32(pack.version)),
        title: Set(pack.title.clone()),
        status: Set(enum_string(pack.status)?),
        content_sha256: Set(pack.content_sha256.clone()),
        published_at: Set(pack.published_at),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(connection)
    .await
    .map_err(repository_error)?;
    Ok(())
}

async fn insert_rules<C: ConnectionTrait>(
    connection: &C,
    pack: &PolicyPack,
) -> Result<(), ApplicationError> {
    for (position, rule) in pack.rules.iter().enumerate() {
        policy_rules::ActiveModel {
            id: Set(Uuid::now_v7()),
            organization_id: Set(pack.organization_id.0),
            policy_pack_id: Set(pack.id.0),
            rule_id: Set(rule.id.clone()),
            rule_version: Set(i32_from_u32(rule.version)),
            position: Set(i32::try_from(position).unwrap_or(i32::MAX)),
            obligation_key: Set(rule.obligation_key.clone()),
            severity: Set(enum_string(rule.severity)?),
            rule_payload: Set(serde_json::to_value(rule).map_err(serialization_error)?),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(connection)
        .await
        .map_err(repository_error)?;
    }
    Ok(())
}

async fn load_pack<C: ConnectionTrait>(
    connection: &C,
    model: policy_packs::Model,
) -> Result<PolicyPack, ApplicationError> {
    let rules = policy_rules::Entity::find()
        .filter(policy_rules::Column::OrganizationId.eq(model.organization_id))
        .filter(policy_rules::Column::PolicyPackId.eq(model.id))
        .order_by_asc(policy_rules::Column::Position)
        .all(connection)
        .await
        .map_err(repository_error)?
        .into_iter()
        .map(rule_from_model)
        .collect::<Result<Vec<_>, _>>()?;
    pack_from_model(model, rules)
}

fn pack_from_model(
    model: policy_packs::Model,
    rules: Vec<CompiledRule>,
) -> Result<PolicyPack, ApplicationError> {
    Ok(PolicyPack {
        id: PolicyPackId(model.id),
        organization_id: OrganizationId(model.organization_id),
        key: model.key,
        version: u32::try_from(model.version).unwrap_or_default(),
        title: model.title,
        status: enum_from_string(&model.status)?,
        content_sha256: model.content_sha256,
        published_at: model.published_at,
        rules,
    })
}

fn rule_from_model(model: policy_rules::Model) -> Result<CompiledRule, ApplicationError> {
    serde_json::from_value(model.rule_payload).map_err(serialization_error)
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
        let now = OffsetDateTime::now_utc();
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let existing = eval_runs::Entity::find()
            .filter(eval_runs::Column::OrganizationId.eq(organization_id.0))
            .filter(eval_runs::Column::Id.eq(summary.eval_run_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?;
        if let Some(existing) = existing {
            if let Some(stored) = existing.summary.as_ref() {
                let stored: EvaluationSummary =
                    serde_json::from_value(stored.clone()).map_err(serialization_error)?;
                return if stored == *summary {
                    Ok(())
                } else {
                    Err(ApplicationError::Conflict(
                        "run already has a different evaluation summary".to_owned(),
                    ))
                };
            }
            let mut active: eval_runs::ActiveModel = existing.into();
            active.verdict = Set(Some(enum_string(summary.verdict)?));
            active.summary = Set(Some(payload));
            active.completed_at = Set(Some(now));
            active.updated_at = Set(now);
            active
                .update(&transaction)
                .await
                .map_err(repository_error)?;
        } else {
            eval_runs::ActiveModel {
                id: Set(summary.eval_run_id.0),
                organization_id: Set(organization_id.0),
                target_id: Set("unknown".to_owned()),
                target_version: Set(Some("legacy".to_owned())),
                policy_pack_key: Set("unknown".to_owned()),
                policy_pack_id: Set(None),
                policy_pack_version: Set(Some(0)),
                policy_content_sha256: Set(Some("legacy".to_owned())),
                scenario_id: Set(Some(summary.eval_run_id.0)),
                rule_ids: Set(serde_json::json!([])),
                boundary_kind: Set("explicit_ci".to_owned()),
                external_run_id: Set(None),
                primary_invocation_id: Set(Some(summary.eval_run_id.0)),
                state: Set("completed".to_owned()),
                completion_reason: Set(None),
                terminal_state: Set(None),
                verdict: Set(Some(enum_string(summary.verdict)?)),
                summary: Set(Some(payload)),
                settle_until: Set(None),
                hard_deadline_at: Set(Some(now)),
                last_seen_at: Set(None),
                finalized_at: Set(Some(now)),
                updated_at: Set(now),
                span_count: Set(0),
                trace_count: Set(0),
                event_count: Set(0),
                trace_quality: Set(None),
                evidence_sha256: Set(None),
                created_at: Set(now),
                completed_at: Set(Some(now)),
            }
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        }
        for result in &summary.results {
            rule_results::ActiveModel {
                id: Set(result.id.0),
                organization_id: Set(organization_id.0),
                eval_run_id: Set(summary.eval_run_id.0),
                rule_id: Set(result.rule_id.clone()),
                severity: Set(enum_string(result.severity)?),
                status: Set(enum_string(result.status)?),
                payload: Set(serde_json::to_value(result).map_err(serialization_error)?),
                created_at: Set(now),
            }
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)
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
            .and_then(|model| model.summary)
            .map(|summary| serde_json::from_value(summary).map_err(serialization_error))
            .transpose()
    }
}

fn enum_string<T: serde::Serialize>(value: T) -> Result<String, ApplicationError> {
    serde_json::to_value(value)
        .map_err(serialization_error)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApplicationError::Repository("enum did not serialize as a string".to_owned())
        })
}

fn enum_from_string<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, ApplicationError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(serialization_error)
}

fn i32_from_u32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[allow(clippy::needless_pass_by_value)] // Required as a direct `map_err` adapter.
fn serialization_error(error: serde_json::Error) -> ApplicationError {
    ApplicationError::Repository(error.to_string())
}

#[allow(clippy::needless_pass_by_value)] // Required as a direct `map_err` adapter.
fn repository_error(error: sea_orm::DbErr) -> ApplicationError {
    ApplicationError::Repository(error.to_string())
}

#[cfg(test)]
mod tests {
    use governance_domain::{PolicyPackId, ReviewStatus};

    use super::*;

    #[test]
    fn stored_status_round_trips_without_orm_types_leaking() {
        let value = enum_string(ReviewStatus::Approved).expect("status should serialize");
        assert_eq!(value, "approved");
        assert_eq!(
            enum_from_string::<ReviewStatus>(&value).expect("status should deserialize"),
            ReviewStatus::Approved
        );
        assert_ne!(PolicyPackId::new().to_string(), "");
    }
}
