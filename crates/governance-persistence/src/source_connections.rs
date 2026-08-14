use async_trait::async_trait;
use governance_application::{
    ApplicationError, SourceConnectionRepository, SubscriptionRevisionDecision,
};
use governance_domain::{
    OrganizationId, PolicyImportStatus, SourceConnection, SourceConnectionId,
    SourceConnectionStatus, SourceProvider, SourceSubscription, SourceSubscriptionId,
    SourceSubscriptionStatus,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use time::OffsetDateTime;

use crate::{
    entities::{
        policy_imports, source_connection_oauth_states, source_connections, source_subscriptions,
    },
    enum_from_string, repository_error,
};

#[derive(Clone, Debug)]
pub struct SeaOrmSourceConnectionRepository {
    database: DatabaseConnection,
}

impl SeaOrmSourceConnectionRepository {
    #[must_use]
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }

    /// Persists a one-time OAuth state record.
    ///
    /// # Errors
    ///
    /// Returns an application error when organization validation, serialization, or storage fails.
    pub async fn store_oauth_state(&self, state: NewOAuthState) -> Result<(), ApplicationError> {
        super::ensure_organization(&self.database, state.organization_id).await?;
        source_connection_oauth_states::ActiveModel {
            state_hash: Set(state.state_hash),
            organization_id: Set(state.organization_id.0),
            provider: Set(crate::enum_string(state.provider)?),
            actor_id: Set(state.actor_id),
            originating_collection_id: Set(state.originating_collection_id.map(|id| id.0)),
            pkce_ciphertext: Set(state.pkce_ciphertext),
            pkce_nonce: Set(state.pkce_nonce),
            key_version: Set(state
                .key_version
                .map(|value| i32::try_from(value).unwrap_or(i32::MAX))),
            redirect_uri: Set(state.redirect_uri),
            expires_at: Set(state.expires_at),
            consumed_at: Set(None),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(&self.database)
        .await
        .map_err(repository_error)?;
        Ok(())
    }

    /// Atomically consumes an actor- and provider-bound OAuth state record.
    ///
    /// # Errors
    ///
    /// Returns an error when the state is missing, expired, consumed, mismatched, or storage fails.
    pub async fn consume_oauth_state(
        &self,
        organization_id: OrganizationId,
        state_hash: &str,
        provider: SourceProvider,
        actor_id: &str,
    ) -> Result<ConsumedOAuthState, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = source_connection_oauth_states::Entity::find()
            .filter(source_connection_oauth_states::Column::StateHash.eq(state_hash))
            .filter(source_connection_oauth_states::Column::OrganizationId.eq(organization_id.0))
            .filter(
                source_connection_oauth_states::Column::Provider.eq(crate::enum_string(provider)?),
            )
            .filter(source_connection_oauth_states::Column::ActorId.eq(actor_id))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound("OAuth state".to_owned()))?;
        if model.consumed_at.is_some() || model.expires_at <= OffsetDateTime::now_utc() {
            return Err(ApplicationError::Conflict(
                "OAuth state is expired or was already consumed".to_owned(),
            ));
        }
        let mut active: source_connection_oauth_states::ActiveModel = model.clone().into();
        active.consumed_at = Set(Some(OffsetDateTime::now_utc()));
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(ConsumedOAuthState {
            originating_collection_id: model
                .originating_collection_id
                .map(governance_domain::PolicyCollectionId),
            pkce_ciphertext: model.pkce_ciphertext,
            pkce_nonce: model.pkce_nonce,
            key_version: model
                .key_version
                .and_then(|value| u32::try_from(value).ok()),
            redirect_uri: model.redirect_uri,
        })
    }

    /// Creates or refreshes a provider connection without changing its stable identity.
    ///
    /// # Errors
    ///
    /// Returns an error when identity validation, serialization, or storage fails.
    pub async fn save_connection(
        &self,
        input: NewStoredConnection,
    ) -> Result<SourceConnection, ApplicationError> {
        let now = OffsetDateTime::now_utc();
        let existing = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(input.organization_id.0))
            .filter(source_connections::Column::Provider.eq(crate::enum_string(input.provider)?))
            .filter(source_connections::Column::ProviderAccountId.eq(&input.provider_account_id))
            .filter(source_connections::Column::ConnectedBy.eq(&input.connected_by))
            .one(&self.database)
            .await
            .map_err(repository_error)?;
        let model = if let Some(existing) = existing {
            if existing.id != input.id.0 {
                return Err(ApplicationError::Conflict(
                    "connection identity changed during credential update".to_owned(),
                ));
            }
            let mut active: source_connections::ActiveModel = existing.into();
            active.display_label = Set(input.display_label);
            active.status = Set(crate::enum_string(SourceConnectionStatus::Active)?);
            active.granted_scopes =
                Set(serde_json::to_value(input.granted_scopes)
                    .map_err(crate::serialization_error)?);
            active.credential_ciphertext = Set(Some(input.credential_ciphertext));
            active.credential_nonce = Set(Some(input.credential_nonce));
            active.credential_key_version = Set(Some(
                i32::try_from(input.credential_key_version).unwrap_or(i32::MAX),
            ));
            active.access_expires_at = Set(input.access_expires_at);
            active.last_failure_code = Set(None);
            active.updated_at = Set(now);
            active
                .update(&self.database)
                .await
                .map_err(repository_error)?
        } else {
            source_connections::ActiveModel {
                id: Set(input.id.0),
                organization_id: Set(input.organization_id.0),
                provider: Set(crate::enum_string(input.provider)?),
                connected_by: Set(input.connected_by),
                provider_account_id: Set(input.provider_account_id),
                display_label: Set(input.display_label),
                status: Set(crate::enum_string(SourceConnectionStatus::Active)?),
                granted_scopes: Set(serde_json::to_value(input.granted_scopes)
                    .map_err(crate::serialization_error)?),
                credential_ciphertext: Set(Some(input.credential_ciphertext)),
                credential_nonce: Set(Some(input.credential_nonce)),
                credential_key_version: Set(Some(
                    i32::try_from(input.credential_key_version).unwrap_or(i32::MAX),
                )),
                access_expires_at: Set(input.access_expires_at),
                last_sync_at: Set(None),
                last_failure_code: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.database)
            .await
            .map_err(repository_error)?
        };
        connection_from_model(model)
    }

    /// Loads the encrypted credential owned by an actor-bound connection.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection or credential is unavailable or invalid.
    pub async fn encrypted_credential(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
        actor_id: &str,
    ) -> Result<StoredEncryptedCredential, ApplicationError> {
        let model = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::Id.eq(id.0))
            .filter(source_connections::Column::ConnectedBy.eq(actor_id))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        Ok(StoredEncryptedCredential {
            provider: enum_from_string(&model.provider)?,
            ciphertext: model.credential_ciphertext.ok_or_else(|| {
                ApplicationError::Conflict("connection is disconnected".to_owned())
            })?,
            nonce: model.credential_nonce.ok_or_else(|| {
                ApplicationError::Conflict("connection is disconnected".to_owned())
            })?,
            key_version: model
                .credential_key_version
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    ApplicationError::Conflict("connection is disconnected".to_owned())
                })?,
        })
    }

    /// Creates or reuses a subscription for one external provider item.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection is invalid, inactive, mismatched, or storage fails.
    pub async fn upsert_subscription(
        &self,
        input: NewSourceSubscription,
    ) -> Result<SourceSubscription, ApplicationError> {
        let connection = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(input.organization_id.0))
            .filter(source_connections::Column::Id.eq(input.connection_id.0))
            .filter(source_connections::Column::ConnectedBy.eq(&input.actor_id))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(input.connection_id.to_string()))?;
        if enum_from_string::<SourceConnectionStatus>(&connection.status)?
            != SourceConnectionStatus::Active
        {
            return Err(ApplicationError::Conflict(
                "source connection must be active".to_owned(),
            ));
        }
        if enum_from_string::<SourceProvider>(&connection.provider)? != input.provider {
            return Err(ApplicationError::InvalidRequest(
                "source provider does not match the connection".to_owned(),
            ));
        }
        if let Some(existing) = source_subscriptions::Entity::find()
            .filter(source_subscriptions::Column::OrganizationId.eq(input.organization_id.0))
            .filter(source_subscriptions::Column::ConnectionId.eq(input.connection_id.0))
            .filter(source_subscriptions::Column::ExternalItemId.eq(&input.external_item_id))
            .one(&self.database)
            .await
            .map_err(repository_error)?
        {
            return subscription_from_model(existing);
        }
        let now = OffsetDateTime::now_utc();
        let model = source_subscriptions::ActiveModel {
            id: Set(SourceSubscriptionId::new().0),
            organization_id: Set(input.organization_id.0),
            connection_id: Set(Some(input.connection_id.0)),
            provider: Set(crate::enum_string(input.provider)?),
            external_item_id: Set(input.external_item_id),
            canonical_url: Set(None),
            title: Set(input
                .title_hint
                .unwrap_or_else(|| "Remote policy source".to_owned())),
            mime_type: Set(None),
            policy_source_id: Set(governance_domain::PolicySourceId::new().0),
            last_external_revision: Set(None),
            last_import_id: Set(None),
            last_observed_modified_at: Set(None),
            status: Set(crate::enum_string(SourceSubscriptionStatus::Active)?),
            failure_code: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&self.database)
        .await
        .map_err(repository_error)?;
        subscription_from_model(model)
    }

    /// Records the latest successfully imported revision for a subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when the subscription is missing or the transaction fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_subscription_import(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
        import_id: governance_domain::PolicyImportId,
        policy_source_id: governance_domain::PolicySourceId,
        external_revision: String,
        external_modified_at: Option<OffsetDateTime>,
        canonical_url: Option<String>,
        title: String,
        mime_type: Option<String>,
    ) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = source_subscriptions::Entity::find()
            .filter(source_subscriptions::Column::OrganizationId.eq(organization_id.0))
            .filter(source_subscriptions::Column::Id.eq(subscription_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(subscription_id.to_string()))?;
        let mut active: source_subscriptions::ActiveModel = model.into();
        active.last_import_id = Set(Some(import_id.0));
        active.policy_source_id = Set(policy_source_id.0);
        active.last_external_revision = Set(Some(external_revision));
        active.last_observed_modified_at = Set(external_modified_at);
        active.canonical_url = Set(canonical_url);
        active.title = Set(title);
        active.mime_type = Set(mime_type);
        active.status = Set(crate::enum_string(SourceSubscriptionStatus::Active)?);
        active.failure_code = Set(None);
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)
    }

    /// Removes stored credentials and disconnects every dependent subscription atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when the actor-owned connection is missing or the transaction fails.
    pub async fn disconnect_connection(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
        actor_id: &str,
    ) -> Result<(), ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::Id.eq(id.0))
            .filter(source_connections::Column::ConnectedBy.eq(actor_id))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let mut active: source_connections::ActiveModel = model.into();
        active.status = Set(crate::enum_string(SourceConnectionStatus::Disconnected)?);
        active.credential_ciphertext = Set(None);
        active.credential_nonce = Set(None);
        active.credential_key_version = Set(None);
        active.access_expires_at = Set(None);
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        let subscriptions = source_subscriptions::Entity::find()
            .filter(source_subscriptions::Column::OrganizationId.eq(organization_id.0))
            .filter(source_subscriptions::Column::ConnectionId.eq(id.0))
            .all(&transaction)
            .await
            .map_err(repository_error)?;
        for subscription in subscriptions {
            let mut active: source_subscriptions::ActiveModel = subscription.into();
            active.status = Set(crate::enum_string(SourceSubscriptionStatus::Disconnected)?);
            active.updated_at = Set(OffsetDateTime::now_utc());
            active
                .update(&transaction)
                .await
                .map_err(repository_error)?;
        }
        transaction.commit().await.map_err(repository_error)
    }

    /// Replaces an actor-owned connection credential after refresh or key rotation.
    ///
    /// # Errors
    ///
    /// Returns an error when ownership or provider binding fails, or storage is unavailable.
    pub async fn update_encrypted_credential(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
        actor_id: &str,
        credential: StoredEncryptedCredential,
        access_expires_at: Option<OffsetDateTime>,
    ) -> Result<(), ApplicationError> {
        let model = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::Id.eq(id.0))
            .filter(source_connections::Column::ConnectedBy.eq(actor_id))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        if enum_from_string::<SourceProvider>(&model.provider)? != credential.provider {
            return Err(ApplicationError::Conflict(
                "credential provider binding changed".to_owned(),
            ));
        }
        let mut active: source_connections::ActiveModel = model.into();
        active.credential_ciphertext = Set(Some(credential.ciphertext));
        active.credential_nonce = Set(Some(credential.nonce));
        active.credential_key_version = Set(Some(
            i32::try_from(credential.key_version).unwrap_or(i32::MAX),
        ));
        active.access_expires_at = Set(access_expires_at);
        active.status = Set(crate::enum_string(SourceConnectionStatus::Active)?);
        active.last_failure_code = Set(None);
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(())
    }

    /// Records a classified connection failure and status transition.
    ///
    /// # Errors
    ///
    /// Returns an error when the actor-owned connection is missing or storage fails.
    pub async fn mark_connection_failure(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
        actor_id: &str,
        status: SourceConnectionStatus,
        code: &str,
    ) -> Result<(), ApplicationError> {
        let model = source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::Id.eq(id.0))
            .filter(source_connections::Column::ConnectedBy.eq(actor_id))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        let mut active: source_connections::ActiveModel = model.into();
        active.status = Set(crate::enum_string(status)?);
        active.last_failure_code = Set(Some(code.to_owned()));
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&self.database)
            .await
            .map_err(repository_error)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NewOAuthState {
    pub state_hash: String,
    pub organization_id: OrganizationId,
    pub provider: SourceProvider,
    pub actor_id: String,
    pub originating_collection_id: Option<governance_domain::PolicyCollectionId>,
    pub pkce_ciphertext: Option<Vec<u8>>,
    pub pkce_nonce: Option<Vec<u8>>,
    pub key_version: Option<u32>,
    pub redirect_uri: String,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct ConsumedOAuthState {
    pub originating_collection_id: Option<governance_domain::PolicyCollectionId>,
    pub pkce_ciphertext: Option<Vec<u8>>,
    pub pkce_nonce: Option<Vec<u8>>,
    pub key_version: Option<u32>,
    pub redirect_uri: String,
}

#[derive(Clone, Debug)]
pub struct NewStoredConnection {
    pub id: SourceConnectionId,
    pub organization_id: OrganizationId,
    pub provider: SourceProvider,
    pub connected_by: String,
    pub provider_account_id: String,
    pub display_label: String,
    pub granted_scopes: Vec<String>,
    pub credential_ciphertext: Vec<u8>,
    pub credential_nonce: Vec<u8>,
    pub credential_key_version: u32,
    pub access_expires_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug)]
pub struct StoredEncryptedCredential {
    pub provider: SourceProvider,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_version: u32,
}

#[derive(Clone, Debug)]
pub struct NewSourceSubscription {
    pub organization_id: OrganizationId,
    pub connection_id: SourceConnectionId,
    pub provider: SourceProvider,
    pub external_item_id: String,
    pub title_hint: Option<String>,
    pub actor_id: String,
}

#[async_trait]
impl SourceConnectionRepository for SeaOrmSourceConnectionRepository {
    async fn list_connections(
        &self,
        organization_id: OrganizationId,
        actor_id: &str,
    ) -> Result<Vec<SourceConnection>, ApplicationError> {
        source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::ConnectedBy.eq(actor_id))
            .order_by_desc(source_connections::Column::UpdatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(connection_from_model)
            .collect()
    }

    async fn get_connection(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
    ) -> Result<Option<SourceConnection>, ApplicationError> {
        source_connections::Entity::find()
            .filter(source_connections::Column::OrganizationId.eq(organization_id.0))
            .filter(source_connections::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(connection_from_model)
            .transpose()
    }

    async fn list_subscriptions(
        &self,
        organization_id: OrganizationId,
        connection_id: SourceConnectionId,
    ) -> Result<Vec<SourceSubscription>, ApplicationError> {
        source_subscriptions::Entity::find()
            .filter(source_subscriptions::Column::OrganizationId.eq(organization_id.0))
            .filter(source_subscriptions::Column::ConnectionId.eq(connection_id.0))
            .order_by_desc(source_subscriptions::Column::UpdatedAt)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(subscription_from_model)
            .collect()
    }

    async fn reserve_revision(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
        external_revision: &str,
        raw_sha256: &str,
    ) -> Result<SubscriptionRevisionDecision, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let subscription = source_subscriptions::Entity::find()
            .filter(source_subscriptions::Column::OrganizationId.eq(organization_id.0))
            .filter(source_subscriptions::Column::Id.eq(subscription_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(subscription_id.to_string()))?;
        let Some(last_import_id) = subscription.last_import_id else {
            return Ok(SubscriptionRevisionDecision::CreateInitial);
        };
        let import = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(last_import_id))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(last_import_id.to_string()))?;
        if subscription.last_external_revision.as_deref() == Some(external_revision)
            || import.content_sha256 == raw_sha256
        {
            return Ok(SubscriptionRevisionDecision::Unchanged);
        }
        if enum_from_string::<PolicyImportStatus>(&import.status)? != PolicyImportStatus::Compiled {
            return Ok(SubscriptionRevisionDecision::BlockedPendingReview);
        }
        Ok(SubscriptionRevisionDecision::CreateRevision)
    }
}

fn connection_from_model(
    model: source_connections::Model,
) -> Result<SourceConnection, ApplicationError> {
    Ok(SourceConnection {
        id: SourceConnectionId(model.id),
        organization_id: OrganizationId(model.organization_id),
        provider: enum_from_string::<SourceProvider>(&model.provider)?,
        connected_by: model.connected_by,
        provider_account_id: model.provider_account_id,
        display_label: model.display_label,
        status: enum_from_string::<SourceConnectionStatus>(&model.status)?,
        granted_scopes: serde_json::from_value(model.granted_scopes)
            .map_err(crate::serialization_error)?,
        access_expires_at: model.access_expires_at,
        last_sync_at: model.last_sync_at,
        last_failure_code: model.last_failure_code,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn subscription_from_model(
    model: source_subscriptions::Model,
) -> Result<SourceSubscription, ApplicationError> {
    Ok(SourceSubscription {
        id: SourceSubscriptionId(model.id),
        organization_id: OrganizationId(model.organization_id),
        connection_id: model.connection_id.map(SourceConnectionId),
        provider: enum_from_string(&model.provider)?,
        external_item_id: model.external_item_id,
        canonical_url: model.canonical_url,
        title: model.title,
        mime_type: model.mime_type,
        policy_source_id: governance_domain::PolicySourceId(model.policy_source_id),
        last_external_revision: model.last_external_revision,
        last_import_id: model.last_import_id.map(governance_domain::PolicyImportId),
        status: enum_from_string::<SourceSubscriptionStatus>(&model.status)?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
