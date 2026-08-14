use async_trait::async_trait;
use governance_domain::{
    OrganizationId, PolicyCollectionId, SourceConnection, SourceConnectionId, SourceIngestionBatch,
    SourceIngestionBatchId, SourceIngestionItem, SourceIngestionItemId, SourceSubscription,
    SourceSubscriptionId,
};

use crate::ApplicationError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionRevisionDecision {
    Unchanged,
    CreateInitial,
    CreateRevision,
    BlockedPendingReview,
}

#[async_trait]
pub trait SourceConnectionRepository: Send + Sync {
    async fn list_connections(
        &self,
        organization_id: OrganizationId,
        actor_id: &str,
    ) -> Result<Vec<SourceConnection>, ApplicationError>;
    async fn get_connection(
        &self,
        organization_id: OrganizationId,
        id: SourceConnectionId,
    ) -> Result<Option<SourceConnection>, ApplicationError>;
    async fn list_subscriptions(
        &self,
        organization_id: OrganizationId,
        connection_id: SourceConnectionId,
    ) -> Result<Vec<SourceSubscription>, ApplicationError>;
    async fn reserve_revision(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
        external_revision: &str,
        raw_sha256: &str,
    ) -> Result<SubscriptionRevisionDecision, ApplicationError>;
}

#[async_trait]
pub trait SourceIngestionRepository: Send + Sync {
    async fn create_batch(
        &self,
        batch: &SourceIngestionBatch,
        items: &[SourceIngestionItem],
    ) -> Result<SourceIngestionBatch, ApplicationError>;
    async fn get_batch(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionBatchId,
    ) -> Result<Option<(SourceIngestionBatch, Vec<SourceIngestionItem>)>, ApplicationError>;
    async fn claim_item(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionItemId,
    ) -> Result<SourceIngestionItem, ApplicationError>;
    async fn update_item(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionItemId,
        status: governance_domain::SourceIngestionItemStatus,
        policy_import_id: Option<governance_domain::PolicyImportId>,
        failure: Option<(&str, &str)>,
    ) -> Result<SourceIngestionItem, ApplicationError>;
    async fn recompute_batch(
        &self,
        organization_id: OrganizationId,
        id: SourceIngestionBatchId,
    ) -> Result<SourceIngestionBatch, ApplicationError>;
}

#[derive(Clone, Debug)]
pub struct NewSourceBatch {
    pub organization_id: OrganizationId,
    pub policy_collection_id: Option<PolicyCollectionId>,
    pub requested_by: String,
}
