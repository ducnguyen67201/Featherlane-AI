use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    EventMatcher, OrganizationId, PolicyCandidateId, PolicyCandidateReviewId, PolicyImportId,
    PolicyImportTransformationId, PolicyPackId, PolicySourceId, RuleAssertion, Severity, SourceId,
    SourceIngestionItemId, SourceLocator, SourceSubscriptionId, SourceType,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyImportStatus {
    Uploading,
    Queued,
    Parsing,
    Extracting,
    ReviewRequired,
    ReadyToCompile,
    Compiled,
    NeedsOcr,
    FailedRetryable,
    FailedTerminal,
}

impl PolicyImportStatus {
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use PolicyImportStatus::{
            Compiled, Extracting, FailedRetryable, FailedTerminal, NeedsOcr, Parsing, Queued,
            ReadyToCompile, ReviewRequired, Uploading,
        };

        matches!(
            (self, next),
            (Uploading | FailedRetryable | NeedsOcr, Queued)
                | (Uploading, FailedRetryable | FailedTerminal)
                | (Queued, Parsing | FailedRetryable | FailedTerminal)
                | (
                    Parsing,
                    Extracting | NeedsOcr | FailedRetryable | FailedTerminal
                )
                | (
                    Extracting,
                    ReviewRequired | FailedRetryable | FailedTerminal
                )
                | (ReviewRequired, ReadyToCompile)
                | (ReadyToCompile, ReviewRequired | Compiled)
        )
    }

    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Uploading | Self::Queued | Self::Parsing | Self::Extracting
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyInputKind {
    File,
    PastedText,
    Url,
    GoogleDrive,
    MicrosoftGraph,
    Notion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    PlainText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCandidateOrigin {
    Model,
    Human,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCandidateStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleMappingStatus {
    Ready,
    ManualRequired,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceVerificationStatus {
    Pending,
    Verified,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceSegment {
    pub ordinal: u32,
    pub page: Option<u32>,
    pub section: Option<String>,
    pub paragraph_start: Option<u32>,
    pub paragraph_end: Option<u32>,
    pub text: String,
    pub text_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub format: DocumentFormat,
    pub parser_version: String,
    pub title: Option<String>,
    pub segments: Vec<SourceSegment>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyImportCoverage {
    pub total_chunks: u32,
    pub processed_chunks: u32,
    #[serde(default)]
    pub failed_chunks: Vec<String>,
    pub duplicate_candidates: u32,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PolicyImportCoverage {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.total_chunks > 0
            && self.total_chunks == self.processed_chunks
            && self.failed_chunks.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyImport {
    pub id: PolicyImportId,
    pub organization_id: OrganizationId,
    pub policy_source_id: PolicySourceId,
    pub revision: u32,
    pub supersedes_import_id: Option<PolicyImportId>,
    pub status: PolicyImportStatus,
    pub input_kind: PolicyInputKind,
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub effective_from: Option<OffsetDateTime>,
    pub source_url: Option<String>,
    pub original_filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub detected_mime_type: String,
    pub byte_length: u64,
    pub content_sha256: String,
    pub raw_object_key: String,
    pub processing_object_key: String,
    pub processing_content_sha256: String,
    pub processing_mime_type: String,
    pub active_transformation_id: Option<PolicyImportTransformationId>,
    pub ingestion_item_id: Option<SourceIngestionItemId>,
    pub source_subscription_id: Option<SourceSubscriptionId>,
    pub external_revision: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub external_modified_at: Option<OffsetDateTime>,
    pub normalized_object_key: Option<String>,
    pub parser_kind: Option<String>,
    pub parser_version: Option<String>,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    pub page_count: Option<u32>,
    pub coverage: PolicyImportCoverage,
    pub candidate_count: u32,
    pub verification_status: SourceVerificationStatus,
    pub verified_by: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub verified_at: Option<OffsetDateTime>,
    pub verification_notes: Option<String>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub idempotency_key: Option<String>,
    pub compiled_source_id: Option<SourceId>,
    pub compiled_policy_pack_id: Option<PolicyPackId>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleSuggestion {
    pub trigger: EventMatcher,
    pub assertions: Vec<RuleAssertion>,
    #[serde(default)]
    pub evidence_required: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractedCandidate {
    pub source_segment_ordinal: u32,
    pub exact_excerpt: String,
    pub statement: String,
    #[serde(default)]
    pub applicability: Value,
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    pub suggested_severity: Severity,
    pub suggested_rule: Option<RuleSuggestion>,
    pub mapping_status: RuleMappingStatus,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionResponse {
    pub candidates: Vec<ExtractedCandidate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractionBatch {
    pub candidates: Vec<ExtractedCandidate>,
    pub coverage: PolicyImportCoverage,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateReview {
    pub reviewer_id: String,
    pub notes: String,
    #[serde(with = "time::serde::rfc3339")]
    pub reviewed_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyCandidate {
    pub id: PolicyCandidateId,
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub position: u32,
    pub origin: PolicyCandidateOrigin,
    pub fingerprint: String,
    pub key: String,
    pub statement: String,
    pub locator: SourceLocator,
    pub source_excerpt: String,
    pub applicability: Value,
    pub exceptions: Vec<String>,
    pub required_evidence: Vec<String>,
    pub suggested_severity: Severity,
    pub suggested_rule: Option<RuleSuggestion>,
    pub mapping_status: RuleMappingStatus,
    pub model_confidence: Option<f32>,
    pub model_payload_sha256: Option<String>,
    pub status: PolicyCandidateStatus,
    pub review: Option<CandidateReview>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyCandidateReviewRecord {
    pub id: PolicyCandidateReviewId,
    pub organization_id: OrganizationId,
    pub candidate_id: PolicyCandidateId,
    pub decision: PolicyCandidateStatus,
    pub reviewer_id: String,
    pub notes: String,
    pub before_payload: Value,
    pub after_payload: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub reviewed_at: OffsetDateTime,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PolicyImportReadiness {
    pub source_verified: bool,
    pub coverage_complete: bool,
    pub all_candidates_disposed: bool,
    pub has_approved_candidate: bool,
    pub approved_mappings_ready: bool,
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl PolicyImportReadiness {
    #[must_use]
    pub fn calculate(import: &PolicyImport, candidates: &[PolicyCandidate]) -> Self {
        let source_verified = import.verification_status == SourceVerificationStatus::Verified;
        let coverage_complete = import.coverage.is_complete();
        let all_candidates_disposed = !candidates.is_empty()
            && candidates
                .iter()
                .all(|candidate| candidate.status != PolicyCandidateStatus::Pending);
        let approved: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.status == PolicyCandidateStatus::Approved)
            .collect();
        let has_approved_candidate = !approved.is_empty();
        let approved_mappings_ready = approved.iter().all(|candidate| {
            candidate.mapping_status == RuleMappingStatus::Ready
                && candidate
                    .suggested_rule
                    .as_ref()
                    .is_some_and(|rule| !rule.assertions.is_empty())
                && !candidate.locator.excerpt_sha256.is_empty()
        });
        let mut blockers = Vec::new();
        if !source_verified {
            blockers.push("source verification is required".to_owned());
        }
        if !coverage_complete {
            blockers.push("all source chunks must be processed".to_owned());
        }
        if !all_candidates_disposed {
            blockers.push("every candidate must be approved or rejected".to_owned());
        }
        if !has_approved_candidate {
            blockers.push("at least one candidate must be approved".to_owned());
        }
        if !approved_mappings_ready {
            blockers
                .push("every approved candidate needs a supported deterministic rule".to_owned());
        }
        Self {
            source_verified,
            coverage_complete,
            all_candidates_disposed,
            has_approved_candidate,
            approved_mappings_ready,
            blockers,
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }

    #[must_use]
    pub fn review_complete(&self) -> bool {
        self.source_verified
            && self.coverage_complete
            && self.all_candidates_disposed
            && self.approved_mappings_ready
    }

    #[must_use]
    pub fn review_blockers(&self) -> Vec<String> {
        self.blockers
            .iter()
            .filter(|blocker| blocker.as_str() != "at least one candidate must be approved")
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{EventType, SourceConfidence};

    fn import() -> PolicyImport {
        let now = OffsetDateTime::now_utc();
        PolicyImport {
            id: PolicyImportId::new(),
            organization_id: OrganizationId::new(),
            policy_source_id: PolicySourceId::new(),
            revision: 1,
            supersedes_import_id: None,
            status: PolicyImportStatus::ReviewRequired,
            input_kind: PolicyInputKind::PastedText,
            source_type: SourceType::CompanyPolicy,
            title: "Refund policy".to_owned(),
            jurisdiction: "internal".to_owned(),
            effective_from: None,
            source_url: None,
            original_filename: None,
            declared_mime_type: None,
            detected_mime_type: "text/plain".to_owned(),
            byte_length: 10,
            content_sha256: "a".repeat(64),
            raw_object_key: "raw".to_owned(),
            processing_object_key: "raw".to_owned(),
            processing_content_sha256: "a".repeat(64),
            processing_mime_type: "text/plain".to_owned(),
            active_transformation_id: None,
            ingestion_item_id: None,
            source_subscription_id: None,
            external_revision: None,
            external_modified_at: None,
            normalized_object_key: Some("normalized".to_owned()),
            parser_kind: Some("plain_text".to_owned()),
            parser_version: Some("1".to_owned()),
            model_provider: Some("test".to_owned()),
            model_name: Some("test".to_owned()),
            prompt_version: Some("v1".to_owned()),
            page_count: None,
            coverage: PolicyImportCoverage {
                total_chunks: 1,
                processed_chunks: 1,
                ..PolicyImportCoverage::default()
            },
            candidate_count: 1,
            verification_status: SourceVerificationStatus::Verified,
            verified_by: Some("reviewer".to_owned()),
            verified_at: Some(now),
            verification_notes: None,
            failure_code: None,
            failure_detail: None,
            idempotency_key: None,
            compiled_source_id: None,
            compiled_policy_pack_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    fn candidate(import: &PolicyImport) -> PolicyCandidate {
        let now = OffsetDateTime::now_utc();
        PolicyCandidate {
            id: PolicyCandidateId::new(),
            organization_id: import.organization_id,
            policy_import_id: import.id,
            position: 0,
            origin: PolicyCandidateOrigin::Human,
            fingerprint: "fingerprint".to_owned(),
            key: "REFUND-001".to_owned(),
            statement: "Approval is required".to_owned(),
            locator: SourceLocator {
                page: None,
                page_end: None,
                section: None,
                paragraph_start: Some(1),
                paragraph_end: Some(1),
                source_url: None,
                excerpt_sha256: "b".repeat(64),
            },
            source_excerpt: "Approval is required".to_owned(),
            applicability: json!({}),
            exceptions: Vec::new(),
            required_evidence: vec!["human_approval_decision".to_owned()],
            suggested_severity: Severity::Critical,
            suggested_rule: Some(RuleSuggestion {
                trigger: EventMatcher {
                    event_type: EventType::ToolCall,
                    name: Some("issue_refund".to_owned()),
                    ..EventMatcher::default()
                },
                assertions: vec![RuleAssertion::ExistsBefore {
                    matcher: EventMatcher {
                        event_type: EventType::HumanApprovalDecision,
                        ..EventMatcher::default()
                    },
                }],
                evidence_required: vec!["human_approval_decision".to_owned()],
            }),
            mapping_status: RuleMappingStatus::Ready,
            model_confidence: None,
            model_payload_sha256: None,
            status: PolicyCandidateStatus::Approved,
            review: Some(CandidateReview {
                reviewer_id: "reviewer".to_owned(),
                notes: String::new(),
                reviewed_at: now,
            }),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn state_machine_has_only_the_documented_transitions() {
        use PolicyImportStatus::{
            Compiled, Extracting, FailedRetryable, FailedTerminal, NeedsOcr, Parsing, Queued,
            ReadyToCompile, ReviewRequired, Uploading,
        };
        let statuses = [
            Uploading,
            Queued,
            Parsing,
            Extracting,
            ReviewRequired,
            ReadyToCompile,
            Compiled,
            NeedsOcr,
            FailedRetryable,
            FailedTerminal,
        ];
        let allowed = [
            (Uploading, Queued),
            (Uploading, FailedRetryable),
            (Uploading, FailedTerminal),
            (Queued, Parsing),
            (Queued, FailedRetryable),
            (Queued, FailedTerminal),
            (Parsing, Extracting),
            (Parsing, NeedsOcr),
            (Parsing, FailedRetryable),
            (Parsing, FailedTerminal),
            (Extracting, ReviewRequired),
            (Extracting, FailedRetryable),
            (Extracting, FailedTerminal),
            (ReviewRequired, ReadyToCompile),
            (ReadyToCompile, ReviewRequired),
            (ReadyToCompile, Compiled),
            (NeedsOcr, Queued),
            (FailedRetryable, Queued),
        ];
        for from in statuses {
            for to in statuses {
                assert_eq!(
                    from.can_transition_to(to),
                    allowed.contains(&(from, to)),
                    "unexpected transition {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn readiness_requires_all_gates() {
        let import = import();
        let readiness = PolicyImportReadiness::calculate(&import, &[candidate(&import)]);
        assert!(readiness.is_ready());
    }

    #[test]
    fn unsupported_approved_candidate_blocks_compilation() {
        let import = import();
        let mut candidate = candidate(&import);
        candidate.mapping_status = RuleMappingStatus::Unsupported;
        candidate.suggested_rule = None;
        let readiness = PolicyImportReadiness::calculate(&import, &[candidate]);
        assert!(!readiness.is_ready());
    }

    #[test]
    fn all_rejected_candidates_complete_review_without_enabling_single_source_compile() {
        let import = import();
        let mut candidate = candidate(&import);
        candidate.status = PolicyCandidateStatus::Rejected;
        candidate.suggested_rule = None;
        candidate.review = Some(CandidateReview {
            reviewer_id: "reviewer".to_owned(),
            notes: "not an enforceable obligation".to_owned(),
            reviewed_at: OffsetDateTime::now_utc(),
        });

        let readiness = PolicyImportReadiness::calculate(&import, &[candidate]);

        assert!(readiness.review_complete());
        assert!(!readiness.is_ready());
        assert!(readiness.review_blockers().is_empty());
    }

    #[test]
    fn pending_candidates_and_failed_coverage_block_compilation() {
        let mut import = import();
        import.coverage.failed_chunks.push("chunk-0002".to_owned());
        let mut candidate = candidate(&import);
        candidate.status = PolicyCandidateStatus::Pending;
        let readiness = PolicyImportReadiness::calculate(&import, &[candidate]);
        assert!(!readiness.coverage_complete);
        assert!(!readiness.all_candidates_disposed);
        assert!(!readiness.is_ready());
    }

    #[test]
    fn source_confidence_remains_separate_from_review_readiness() {
        assert_ne!(
            format!("{:?}", SourceConfidence::OfficialVerified),
            format!("{:?}", SourceVerificationStatus::Verified)
        );
    }
}
