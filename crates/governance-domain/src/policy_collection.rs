use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    OrganizationId, PolicyCandidate, PolicyCandidateStatus, PolicyCollectionId, PolicyImport,
    PolicyImportId, PolicyImportReadiness, PolicyPackId, PolicySourceId, SourceConnectionId,
    SourceIngestionBatchId, SourceIngestionItemId, SourceSubscriptionId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCollectionStatus {
    Draft,
    Compiled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyCollection {
    pub id: PolicyCollectionId,
    pub organization_id: OrganizationId,
    pub key: String,
    pub version: u32,
    pub title: String,
    pub status: PolicyCollectionStatus,
    pub compiled_policy_pack_id: Option<PolicyPackId>,
    pub created_by: String,
    pub idempotency_key: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyCollectionImport {
    pub policy_collection_id: PolicyCollectionId,
    pub policy_import_id: PolicyImportId,
    pub policy_source_id: PolicySourceId,
    pub position: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub added_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceIngestionBatchKind {
    Upload,
    Paste,
    Url,
    GoogleDrive,
    MicrosoftGraph,
    Notion,
    Sync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceIngestionBatchStatus {
    Pending,
    Running,
    Partial,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceIngestionItemStatus {
    Pending,
    Acquiring,
    Queued,
    Processing,
    ReviewRequired,
    Unchanged,
    Blocked,
    Failed,
}

impl SourceIngestionItemStatus {
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use SourceIngestionItemStatus::{
            Acquiring, Blocked, Failed, Pending, Processing, Queued, ReviewRequired, Unchanged,
        };
        matches!(
            (self, next),
            (Pending, Acquiring)
                | (Acquiring, Queued | Unchanged | Blocked | Failed)
                | (Queued, Processing | Failed)
                | (Processing, ReviewRequired | Blocked | Failed)
                | (Failed, Pending)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceIngestionBatch {
    pub id: SourceIngestionBatchId,
    pub organization_id: OrganizationId,
    pub policy_collection_id: Option<PolicyCollectionId>,
    pub kind: SourceIngestionBatchKind,
    pub status: SourceIngestionBatchStatus,
    pub requested_by: String,
    pub total_count: u32,
    pub succeeded_count: u32,
    pub failed_count: u32,
    pub unchanged_count: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceIngestionItem {
    pub id: SourceIngestionItemId,
    pub organization_id: OrganizationId,
    pub batch_id: SourceIngestionBatchId,
    pub ordinal: u32,
    pub client_item_key: String,
    pub connection_id: Option<SourceConnectionId>,
    pub subscription_id: Option<SourceSubscriptionId>,
    pub external_item_id: Option<String>,
    pub status: SourceIngestionItemStatus,
    pub policy_import_id: Option<PolicyImportId>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub attempt_count: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyCollectionSourceBlocker {
    pub policy_import_id: PolicyImportId,
    pub title: String,
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyCollectionReadiness {
    pub source_count: u32,
    pub review_complete_count: u32,
    pub approved_rule_count: u32,
    pub blockers: Vec<PolicyCollectionSourceBlocker>,
    pub collection_blockers: Vec<String>,
}

impl PolicyCollectionReadiness {
    #[must_use]
    pub fn calculate(members: &[(PolicyImport, Vec<PolicyCandidate>)]) -> Self {
        let mut result = Self {
            source_count: u32::try_from(members.len()).unwrap_or(u32::MAX),
            ..Self::default()
        };
        if members.is_empty() {
            result
                .collection_blockers
                .push("collection must contain at least one source".to_owned());
            return result;
        }
        for (import, candidates) in members {
            let readiness = PolicyImportReadiness::calculate(import, candidates);
            if readiness.review_complete() {
                result.review_complete_count += 1;
            } else {
                result.blockers.push(PolicyCollectionSourceBlocker {
                    policy_import_id: import.id,
                    title: import.title.clone(),
                    blockers: readiness.review_blockers(),
                });
            }
            result.approved_rule_count += u32::try_from(
                candidates
                    .iter()
                    .filter(|candidate| candidate.status == PolicyCandidateStatus::Approved)
                    .count(),
            )
            .unwrap_or(u32::MAX);
        }
        if result.approved_rule_count == 0 {
            result
                .collection_blockers
                .push("collection must contain at least one approved rule".to_owned());
        }
        result
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.source_count > 0 && self.blockers.is_empty() && self.collection_blockers.is_empty()
    }
}
