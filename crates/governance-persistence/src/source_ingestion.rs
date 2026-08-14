use async_trait::async_trait;
use governance_application::{ApplicationError, SourceIngestionRepository};
use governance_domain::{
    OrganizationId, PolicyCollectionId, PolicyImportId, SourceConnectionId, SourceIngestionBatch,
    SourceIngestionBatchId, SourceIngestionBatchStatus, SourceIngestionItem, SourceIngestionItemId,
    SourceIngestionItemStatus, SourceSubscriptionId,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use time::OffsetDateTime;

use crate::{
    ensure_organization,
    entities::{source_ingestion_batches, source_ingestion_items},
    enum_from_string, enum_string, repository_error,
};

#[derive(Clone, Debug)]
pub struct SeaOrmSourceIngestionRepository {
    database: DatabaseConnection,
}

impl SeaOrmSourceIngestionRepository {
    #[must_use]
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }

    /// Lists the most recent ingestion batches for a collection.
    ///
    /// # Errors
    ///
    /// Returns an application error when storage fails or persisted values are invalid.
    pub async fn list_batches(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        limit: u64,
    ) -> Result<Vec<SourceIngestionBatch>, ApplicationError> {
        source_ingestion_batches::Entity::find()
            .filter(source_ingestion_batches::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_batches::Column::PolicyCollectionId.eq(collection_id.0))
            .order_by_desc(source_ingestion_batches::Column::CreatedAt)
            .limit(limit.min(50))
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(batch_from_model)
            .collect()
    }
}

#[async_trait]
impl SourceIngestionRepository for SeaOrmSourceIngestionRepository {
    async fn create_batch(
        &self,
        batch: &SourceIngestionBatch,
        items: &[SourceIngestionItem],
    ) -> Result<SourceIngestionBatch, ApplicationError> {
        if items.len() != usize::try_from(batch.total_count).unwrap_or(usize::MAX)
            || items.iter().any(|item| {
                item.organization_id != batch.organization_id || item.batch_id != batch.id
            })
        {
            return Err(ApplicationError::InvalidRequest(
                "batch item counts and ownership must match".to_owned(),
            ));
        }
        ensure_organization(&self.database, batch.organization_id).await?;
        let transaction = self.database.begin().await.map_err(repository_error)?;
        batch_active_model(batch)?
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        for item in items {
            item_active_model(item)?
                .insert(&transaction)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)?;
        Ok(batch.clone())
    }

    async fn get_batch(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionBatchId,
    ) -> Result<Option<(SourceIngestionBatch, Vec<SourceIngestionItem>)>, ApplicationError> {
        let Some(batch) = source_ingestion_batches::Entity::find()
            .filter(source_ingestion_batches::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_batches::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
        else {
            return Ok(None);
        };
        let items = source_ingestion_items::Entity::find()
            .filter(source_ingestion_items::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_items::Column::BatchId.eq(id.0))
            .order_by_asc(source_ingestion_items::Column::Ordinal)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(item_from_model)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some((batch_from_model(batch)?, items)))
    }

    async fn claim_item(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionItemId,
    ) -> Result<SourceIngestionItem, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = source_ingestion_items::Entity::find()
            .filter(source_ingestion_items::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_items::Column::Id.eq(id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let status = enum_from_string::<SourceIngestionItemStatus>(&model.status)?;
        if !matches!(
            status,
            SourceIngestionItemStatus::Pending | SourceIngestionItemStatus::Failed
        ) {
            return Err(ApplicationError::Conflict(
                "source ingestion item is already claimed or terminal".to_owned(),
            ));
        }
        let attempt_count = model.attempt_count;
        let mut active: source_ingestion_items::ActiveModel = model.into();
        active.status = Set(enum_string(SourceIngestionItemStatus::Acquiring)?);
        active.attempt_count = Set(attempt_count.saturating_add(1));
        active.failure_code = Set(None);
        active.failure_detail = Set(None);
        active.updated_at = Set(OffsetDateTime::now_utc());
        let model = active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        item_from_model(model)
    }

    async fn recompute_batch(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionBatchId,
    ) -> Result<SourceIngestionBatch, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = source_ingestion_batches::Entity::find()
            .filter(source_ingestion_batches::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_batches::Column::Id.eq(id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let items = source_ingestion_items::Entity::find()
            .filter(source_ingestion_items::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_items::Column::BatchId.eq(id.0))
            .all(&transaction)
            .await
            .map_err(repository_error)?;
        let succeeded = items
            .iter()
            .filter(|item| item.status == "review_required")
            .count();
        let failed = items.iter().filter(|item| item.status == "failed").count();
        let unchanged = items
            .iter()
            .filter(|item| item.status == "unchanged")
            .count();
        let blocked = items.iter().filter(|item| item.status == "blocked").count();
        let terminal = succeeded + failed + unchanged + blocked;
        let status = if terminal < items.len() {
            SourceIngestionBatchStatus::Running
        } else if failed + blocked == items.len() {
            SourceIngestionBatchStatus::Failed
        } else if failed + blocked > 0 {
            SourceIngestionBatchStatus::Partial
        } else {
            SourceIngestionBatchStatus::Complete
        };
        let mut active: source_ingestion_batches::ActiveModel = model.into();
        active.status = Set(enum_string(status)?);
        active.succeeded_count = Set(i32::try_from(succeeded).unwrap_or(i32::MAX));
        active.failed_count = Set(i32::try_from(failed).unwrap_or(i32::MAX));
        active.unchanged_count = Set(i32::try_from(unchanged).unwrap_or(i32::MAX));
        active.updated_at = Set(OffsetDateTime::now_utc());
        let model = active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        batch_from_model(model)
    }

    async fn update_item(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionItemId,
        status: SourceIngestionItemStatus,
        policy_import_id: Option<PolicyImportId>,
        failure: Option<(&str, &str)>,
    ) -> Result<SourceIngestionItem, ApplicationError> {
        let model = source_ingestion_items::Entity::find()
            .filter(source_ingestion_items::Column::OrganizationId.eq(organization_id.0))
            .filter(source_ingestion_items::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let current = enum_from_string::<SourceIngestionItemStatus>(&model.status)?;
        if current != status && !current.can_transition_to(status) {
            return Err(ApplicationError::Conflict(format!(
                "invalid ingestion item transition from {current:?} to {status:?}"
            )));
        }
        let mut active: source_ingestion_items::ActiveModel = model.into();
        active.status = Set(enum_string(status)?);
        if let Some(import_id) = policy_import_id {
            active.policy_import_id = Set(Some(import_id.0));
        }
        if let Some((code, detail)) = failure {
            active.failure_code = Set(Some(code.to_owned()));
            active.failure_detail = Set(Some(detail.to_owned()));
        } else {
            active.failure_code = Set(None);
            active.failure_detail = Set(None);
        }
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&self.database)
            .await
            .map_err(repository_error)
            .and_then(item_from_model)
    }
}

fn batch_active_model(
    batch: &SourceIngestionBatch,
) -> Result<source_ingestion_batches::ActiveModel, ApplicationError> {
    Ok(source_ingestion_batches::ActiveModel {
        id: Set(batch.id.0),
        organization_id: Set(batch.organization_id.0),
        policy_collection_id: Set(batch.policy_collection_id.map(|id| id.0)),
        kind: Set(enum_string(batch.kind)?),
        status: Set(enum_string(batch.status)?),
        requested_by: Set(batch.requested_by.clone()),
        total_count: Set(i32::try_from(batch.total_count).unwrap_or(i32::MAX)),
        succeeded_count: Set(i32::try_from(batch.succeeded_count).unwrap_or(i32::MAX)),
        failed_count: Set(i32::try_from(batch.failed_count).unwrap_or(i32::MAX)),
        unchanged_count: Set(i32::try_from(batch.unchanged_count).unwrap_or(i32::MAX)),
        idempotency_key: Set(None),
        created_at: Set(batch.created_at),
        updated_at: Set(batch.updated_at),
    })
}

fn item_active_model(
    item: &SourceIngestionItem,
) -> Result<source_ingestion_items::ActiveModel, ApplicationError> {
    Ok(source_ingestion_items::ActiveModel {
        id: Set(item.id.0),
        organization_id: Set(item.organization_id.0),
        batch_id: Set(item.batch_id.0),
        ordinal: Set(i32::try_from(item.ordinal).unwrap_or(i32::MAX)),
        client_item_key: Set(item.client_item_key.clone()),
        connection_id: Set(item.connection_id.map(|id| id.0)),
        subscription_id: Set(item.subscription_id.map(|id| id.0)),
        external_item_id: Set(item.external_item_id.clone()),
        status: Set(enum_string(item.status)?),
        policy_import_id: Set(item.policy_import_id.map(|id| id.0)),
        failure_code: Set(item.failure_code.clone()),
        failure_detail: Set(item.failure_detail.clone()),
        attempt_count: Set(i32::try_from(item.attempt_count).unwrap_or(i32::MAX)),
        created_at: Set(item.created_at),
        updated_at: Set(item.updated_at),
    })
}

fn batch_from_model(
    model: source_ingestion_batches::Model,
) -> Result<SourceIngestionBatch, ApplicationError> {
    Ok(SourceIngestionBatch {
        id: SourceIngestionBatchId(model.id),
        organization_id: OrganizationId(model.organization_id),
        policy_collection_id: model.policy_collection_id.map(PolicyCollectionId),
        kind: enum_from_string(&model.kind)?,
        status: enum_from_string(&model.status)?,
        requested_by: model.requested_by,
        total_count: u32::try_from(model.total_count).unwrap_or_default(),
        succeeded_count: u32::try_from(model.succeeded_count).unwrap_or_default(),
        failed_count: u32::try_from(model.failed_count).unwrap_or_default(),
        unchanged_count: u32::try_from(model.unchanged_count).unwrap_or_default(),
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn item_from_model(
    model: source_ingestion_items::Model,
) -> Result<SourceIngestionItem, ApplicationError> {
    Ok(SourceIngestionItem {
        id: SourceIngestionItemId(model.id),
        organization_id: OrganizationId(model.organization_id),
        batch_id: SourceIngestionBatchId(model.batch_id),
        ordinal: u32::try_from(model.ordinal).unwrap_or_default(),
        client_item_key: model.client_item_key,
        connection_id: model.connection_id.map(SourceConnectionId),
        subscription_id: model.subscription_id.map(SourceSubscriptionId),
        external_item_id: model.external_item_id,
        status: enum_from_string(&model.status)?,
        policy_import_id: model.policy_import_id.map(PolicyImportId),
        failure_code: model.failure_code,
        failure_detail: model.failure_detail,
        attempt_count: u32::try_from(model.attempt_count).unwrap_or_default(),
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
