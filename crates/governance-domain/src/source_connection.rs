use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    OrganizationId, PolicyImportId, PolicyImportTransformationId, PolicySourceId,
    SourceConnectionId, SourceSubscriptionId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvider {
    GoogleDrive,
    MicrosoftGraph,
    Notion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnectionStatus {
    Active,
    ReauthorizationRequired,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSubscriptionStatus {
    Active,
    UpdateWaitingForReview,
    PermissionDenied,
    RemoteDeleted,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyImportTransformationKind {
    HtmlToText,
    NotionMarkdown,
    ManualOcr,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceConnection {
    pub id: SourceConnectionId,
    pub organization_id: OrganizationId,
    pub provider: SourceProvider,
    pub connected_by: String,
    pub provider_account_id: String,
    pub display_label: String,
    pub status: SourceConnectionStatus,
    pub granted_scopes: Vec<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub access_expires_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_sync_at: Option<OffsetDateTime>,
    pub last_failure_code: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceSubscription {
    pub id: SourceSubscriptionId,
    pub organization_id: OrganizationId,
    pub connection_id: Option<SourceConnectionId>,
    pub provider: SourceProvider,
    pub external_item_id: String,
    pub canonical_url: Option<String>,
    pub title: String,
    pub mime_type: Option<String>,
    pub policy_source_id: PolicySourceId,
    pub last_external_revision: Option<String>,
    pub last_import_id: Option<PolicyImportId>,
    pub status: SourceSubscriptionStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyImportTransformation {
    pub id: PolicyImportTransformationId,
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub kind: PolicyImportTransformationKind,
    pub input_sha256: String,
    pub output_sha256: String,
    pub output_mime_type: String,
    pub processor: String,
    pub processor_version: String,
    pub created_by: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
