use async_trait::async_trait;
use governance_application::{
    ApplicationError, CollectionCompilationSnapshot, CompiledCollectionSource,
    PolicyCollectionRepository, PolicyPackRepository,
};
use governance_domain::{
    OrganizationId, PolicyCollection, PolicyCollectionId, PolicyCollectionImport,
    PolicyCollectionStatus, PolicyImport, PolicyImportId, PolicyImportStatus, PolicyPack,
    PolicyPackId, PolicySourceId,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, SqlErr, TransactionTrait,
};
use time::OffsetDateTime;

use crate::{
    SeaOrmPolicyPackRepository, ensure_organization,
    entities::{policy_candidates, policy_collection_imports, policy_collections, policy_imports},
    enum_from_string, enum_string, persist_bundle, repository_error,
};

#[derive(Clone, Debug)]
pub struct SeaOrmPolicyCollectionRepository {
    database: DatabaseConnection,
}

impl SeaOrmPolicyCollectionRepository {
    #[must_use]
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait]
impl PolicyCollectionRepository for SeaOrmPolicyCollectionRepository {
    async fn create(
        &self,
        collection: &PolicyCollection,
    ) -> Result<PolicyCollection, ApplicationError> {
        ensure_organization(&self.database, collection.organization_id).await?;
        let result = collection_active_model(collection)?
            .insert(&self.database)
            .await;
        match result {
            Ok(_) => Ok(collection.clone()),
            Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                Err(ApplicationError::Conflict(
                    "collection key/version or idempotency key already exists".to_owned(),
                ))
            }
            Err(error) => Err(repository_error(error)),
        }
    }

    async fn get(
        &self,
        organization_id: OrganizationId,
        id: PolicyCollectionId,
    ) -> Result<Option<PolicyCollection>, ApplicationError> {
        policy_collections::Entity::find()
            .filter(policy_collections::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collections::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(collection_from_model)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PolicyCollection>, ApplicationError> {
        policy_collections::Entity::find()
            .filter(policy_collections::Column::OrganizationId.eq(organization_id.0))
            .order_by_desc(policy_collections::Column::UpdatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(collection_from_model)
            .collect()
    }

    async fn members(
        &self,
        organization_id: OrganizationId,
        id: PolicyCollectionId,
    ) -> Result<Vec<PolicyCollectionImport>, ApplicationError> {
        Ok(policy_collection_imports::Entity::find()
            .filter(policy_collection_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collection_imports::Column::PolicyCollectionId.eq(id.0))
            .order_by_asc(policy_collection_imports::Column::Position)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(|model| member_from_model(&model))
            .collect())
    }

    async fn add_import(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        import: &PolicyImport,
    ) -> Result<PolicyCollectionImport, ApplicationError> {
        if import.organization_id != organization_id {
            return Err(ApplicationError::NotFound(import.id.to_string()));
        }
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let collection = policy_collections::Entity::find()
            .filter(policy_collections::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collections::Column::Id.eq(collection_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(collection_id.to_string()))?;
        if collection.status != enum_string(PolicyCollectionStatus::Draft)? {
            return Err(ApplicationError::Conflict(
                "compiled collection membership is immutable".to_owned(),
            ));
        }
        let stored_import = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(import.id.0))
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(import.id.to_string()))?;
        if stored_import.policy_source_id != import.policy_source_id.0 {
            return Err(ApplicationError::Conflict(
                "policy source lineage changed while adding the member".to_owned(),
            ));
        }
        let position = policy_collection_imports::Entity::find()
            .filter(policy_collection_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collection_imports::Column::PolicyCollectionId.eq(collection_id.0))
            .all(&transaction)
            .await
            .map_err(repository_error)?
            .len();
        let member = PolicyCollectionImport {
            policy_collection_id: collection_id,
            policy_import_id: import.id,
            policy_source_id: import.policy_source_id,
            position: u32::try_from(position).unwrap_or(u32::MAX),
            added_at: OffsetDateTime::now_utc(),
        };
        let result = policy_collection_imports::ActiveModel {
            organization_id: Set(organization_id.0),
            policy_collection_id: Set(collection_id.0),
            policy_import_id: Set(import.id.0),
            policy_source_id: Set(import.policy_source_id.0),
            position: Set(i32::try_from(member.position).unwrap_or(i32::MAX)),
            added_at: Set(member.added_at),
        }
        .insert(&transaction)
        .await;
        if let Err(error) = result {
            if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                return Err(ApplicationError::Conflict(
                    "this source revision or another revision of the source is already in the collection"
                        .to_owned(),
                ));
            }
            return Err(repository_error(error));
        }
        transaction.commit().await.map_err(repository_error)?;
        Ok(member)
    }

    async fn remove_import(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        import_id: PolicyImportId,
    ) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let collection = policy_collections::Entity::find()
            .filter(policy_collections::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collections::Column::Id.eq(collection_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(collection_id.to_string()))?;
        if collection.status != enum_string(PolicyCollectionStatus::Draft)? {
            return Err(ApplicationError::Conflict(
                "compiled collection membership is immutable".to_owned(),
            ));
        }
        let result = policy_collection_imports::Entity::delete_many()
            .filter(policy_collection_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collection_imports::Column::PolicyCollectionId.eq(collection_id.0))
            .filter(policy_collection_imports::Column::PolicyImportId.eq(import_id.0))
            .exec(&transaction)
            .await
            .map_err(repository_error)?;
        if result.rows_affected != 1 {
            return Err(ApplicationError::NotFound(import_id.to_string()));
        }
        transaction.commit().await.map_err(repository_error)
    }

    async fn save_compiled_bundle(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        bundle: &governance_domain::PolicyBundle,
        snapshot: &CollectionCompilationSnapshot,
        sources: &[CompiledCollectionSource],
    ) -> Result<PolicyPack, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let collection = policy_collections::Entity::find()
            .filter(policy_collections::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collections::Column::Id.eq(collection_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(collection_id.to_string()))?;
        if let Some(pack_id) = collection.compiled_policy_pack_id {
            transaction.commit().await.map_err(repository_error)?;
            return SeaOrmPolicyPackRepository::new(self.database.clone())
                .get(organization_id, PolicyPackId(pack_id))
                .await?
                .ok_or_else(|| ApplicationError::NotFound(pack_id.to_string()));
        }
        if collection.status != enum_string(PolicyCollectionStatus::Draft)? {
            return Err(ApplicationError::Conflict(
                "policy collection is not a draft".to_owned(),
            ));
        }
        let members = policy_collection_imports::Entity::find()
            .filter(policy_collection_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_collection_imports::Column::PolicyCollectionId.eq(collection_id.0))
            .order_by_asc(policy_collection_imports::Column::PolicyImportId)
            .lock_exclusive()
            .all(&transaction)
            .await
            .map_err(repository_error)?;
        let member_ids: Vec<_> = members
            .iter()
            .map(|member| member.policy_import_id)
            .collect();
        let snapshot_ids: Vec<_> = snapshot.imports.iter().map(|(id, _)| id.0).collect();
        if member_ids != snapshot_ids {
            return Err(ApplicationError::Conflict(
                "collection membership changed during compilation".to_owned(),
            ));
        }
        for (import_id, expected_updated_at) in &snapshot.imports {
            let model = policy_imports::Entity::find()
                .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
                .filter(policy_imports::Column::Id.eq(import_id.0))
                .lock_exclusive()
                .one(&transaction)
                .await
                .map_err(repository_error)?
                .ok_or_else(|| ApplicationError::NotFound(import_id.to_string()))?;
            if model.updated_at != *expected_updated_at
                || !matches!(
                    enum_from_string(&model.status)?,
                    PolicyImportStatus::ReadyToCompile | PolicyImportStatus::Compiled
                )
            {
                return Err(ApplicationError::Conflict(
                    "source review changed during compilation".to_owned(),
                ));
            }
        }
        for (candidate_id, expected_updated_at) in &snapshot.candidates {
            let model = policy_candidates::Entity::find()
                .filter(policy_candidates::Column::OrganizationId.eq(organization_id.0))
                .filter(policy_candidates::Column::Id.eq(candidate_id.0))
                .lock_exclusive()
                .one(&transaction)
                .await
                .map_err(repository_error)?
                .ok_or_else(|| ApplicationError::NotFound(candidate_id.to_string()))?;
            if model.updated_at != *expected_updated_at
                || !member_ids.contains(&model.policy_import_id)
            {
                return Err(ApplicationError::Conflict(
                    "candidate review changed during compilation".to_owned(),
                ));
            }
        }
        persist_bundle(&transaction, bundle).await?;
        for assignment in sources {
            let model = policy_imports::Entity::find()
                .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
                .filter(policy_imports::Column::Id.eq(assignment.policy_import_id.0))
                .one(&transaction)
                .await
                .map_err(repository_error)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(assignment.policy_import_id.to_string())
                })?;
            if model
                .compiled_source_id
                .is_some_and(|id| id != assignment.source_id.0)
            {
                return Err(ApplicationError::Conflict(
                    "compiled source identity cannot be changed".to_owned(),
                ));
            }
            let compiled_policy_pack_id = model.compiled_policy_pack_id;
            let mut active: policy_imports::ActiveModel = model.into();
            active.status = Set(enum_string(PolicyImportStatus::Compiled)?);
            active.compiled_source_id = Set(Some(assignment.source_id.0));
            if compiled_policy_pack_id.is_none() {
                active.compiled_policy_pack_id = Set(Some(bundle.pack.id.0));
            }
            active.completed_at = Set(Some(OffsetDateTime::now_utc()));
            active.updated_at = Set(OffsetDateTime::now_utc());
            active
                .update(&transaction)
                .await
                .map_err(repository_error)?;
        }
        let mut active: policy_collections::ActiveModel = collection.into();
        active.status = Set(enum_string(PolicyCollectionStatus::Compiled)?);
        active.compiled_policy_pack_id = Set(Some(bundle.pack.id.0));
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(bundle.pack.clone())
    }

    async fn compiled_pack(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
    ) -> Result<Option<PolicyPack>, ApplicationError> {
        let Some(collection) = self.get(organization_id, collection_id).await? else {
            return Ok(None);
        };
        let Some(pack_id) = collection.compiled_policy_pack_id else {
            return Ok(None);
        };
        SeaOrmPolicyPackRepository::new(self.database.clone())
            .get(organization_id, pack_id)
            .await
    }
}

fn collection_active_model(
    collection: &PolicyCollection,
) -> Result<policy_collections::ActiveModel, ApplicationError> {
    Ok(policy_collections::ActiveModel {
        id: Set(collection.id.0),
        organization_id: Set(collection.organization_id.0),
        key: Set(collection.key.clone()),
        version: Set(i32::try_from(collection.version).unwrap_or(i32::MAX)),
        title: Set(collection.title.clone()),
        status: Set(enum_string(collection.status)?),
        compiled_policy_pack_id: Set(collection.compiled_policy_pack_id.map(|id| id.0)),
        created_by: Set(collection.created_by.clone()),
        idempotency_key: Set(collection.idempotency_key.clone()),
        created_at: Set(collection.created_at),
        updated_at: Set(collection.updated_at),
    })
}

fn collection_from_model(
    model: policy_collections::Model,
) -> Result<PolicyCollection, ApplicationError> {
    Ok(PolicyCollection {
        id: PolicyCollectionId(model.id),
        organization_id: OrganizationId(model.organization_id),
        key: model.key,
        version: u32::try_from(model.version).unwrap_or_default(),
        title: model.title,
        status: enum_from_string(&model.status)?,
        compiled_policy_pack_id: model.compiled_policy_pack_id.map(PolicyPackId),
        created_by: model.created_by,
        idempotency_key: model.idempotency_key,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn member_from_model(model: &policy_collection_imports::Model) -> PolicyCollectionImport {
    PolicyCollectionImport {
        policy_collection_id: PolicyCollectionId(model.policy_collection_id),
        policy_import_id: PolicyImportId(model.policy_import_id),
        policy_source_id: PolicySourceId(model.policy_source_id),
        position: u32::try_from(model.position).unwrap_or_default(),
        added_at: model.added_at,
    }
}
