#![allow(clippy::missing_errors_doc, clippy::too_many_lines)]

use async_trait::async_trait;
use governance_domain::{
    CandidateReview, CompiledRule, DocumentFormat, ExtractedCandidate, ExtractionBatch,
    MissingEvidencePolicy, Obligation, ObligationId, OrganizationId, ParsedDocument, PolicyBundle,
    PolicyCandidate, PolicyCandidateId, PolicyCandidateOrigin, PolicyCandidateReviewId,
    PolicyCandidateReviewRecord, PolicyCandidateStatus, PolicyImport, PolicyImportCoverage,
    PolicyImportId, PolicyImportReadiness, PolicyImportStatus, PolicyPack, PolicySourceId,
    ReviewStatus, ReviewerApproval, RuleMappingStatus, RuleSuggestion, Severity, Source,
    SourceConfidence, SourceId, SourceLocator, SourceType, SourceVerificationStatus,
};
use governance_policy::{PolicyDocument, compile_policy_document};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::ApplicationError;

#[derive(Clone, Debug)]
pub struct NewPolicyImport {
    pub organization_id: OrganizationId,
    pub input_kind: governance_domain::PolicyInputKind,
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    pub effective_from: Option<OffsetDateTime>,
    pub source_url: Option<String>,
    pub original_filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub detected_mime_type: String,
    pub content: Vec<u8>,
    pub idempotency_key: Option<String>,
    pub supersedes_import_id: Option<PolicyImportId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateEdit {
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
}

#[derive(Clone, Debug)]
pub struct ReviewCandidateCommand {
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub candidate_id: PolicyCandidateId,
    pub decision: PolicyCandidateStatus,
    pub reviewer_id: String,
    pub notes: String,
    pub expected_updated_at: Option<OffsetDateTime>,
    pub edit: Option<CandidateEdit>,
}

#[derive(Clone, Debug)]
pub struct ManualCandidateCommand {
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub reviewer_id: String,
    pub statement: String,
    pub source_excerpt: String,
    pub locator: SourceLocator,
    pub applicability: Value,
    pub exceptions: Vec<String>,
    pub required_evidence: Vec<String>,
    pub suggested_severity: Severity,
    pub suggested_rule: RuleSuggestion,
}

#[derive(Clone, Debug)]
pub struct VerifySourceCommand {
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub verified: bool,
    pub reviewer_id: String,
    pub notes: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CompilePolicyImportCommand {
    pub organization_id: OrganizationId,
    pub policy_import_id: PolicyImportId,
    pub key: String,
    pub version: u32,
    pub title: String,
}

#[async_trait]
pub trait PolicyImportRepository: Send + Sync {
    async fn create(&self, import: &PolicyImport) -> Result<PolicyImport, ApplicationError>;
    async fn get(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
    ) -> Result<Option<PolicyImport>, ApplicationError>;
    async fn list(
        &self,
        organization_id: OrganizationId,
        limit: u64,
        status: Option<PolicyImportStatus>,
        cursor: Option<PolicyImportId>,
    ) -> Result<Vec<PolicyImport>, ApplicationError>;
    async fn transition(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
        expected: PolicyImportStatus,
        next: PolicyImportStatus,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn mark_parsed(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
        normalized_object_key: &str,
        document: &ParsedDocument,
        coverage: &PolicyImportCoverage,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn replace_unreviewed_candidates(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
        candidates: &[PolicyCandidate],
        batch: &ExtractionBatch,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn list_candidates(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
    ) -> Result<Vec<PolicyCandidate>, ApplicationError>;
    async fn get_candidate(
        &self,
        organization_id: OrganizationId,
        import_id: PolicyImportId,
        candidate_id: PolicyCandidateId,
    ) -> Result<Option<PolicyCandidate>, ApplicationError>;
    async fn save_candidate_review(
        &self,
        candidate: &PolicyCandidate,
        review: &PolicyCandidateReviewRecord,
        expected_updated_at: Option<OffsetDateTime>,
    ) -> Result<PolicyCandidate, ApplicationError>;
    async fn add_manual_candidate(
        &self,
        candidate: &PolicyCandidate,
        review: &PolicyCandidateReviewRecord,
    ) -> Result<PolicyCandidate, ApplicationError>;
    async fn verify_source(
        &self,
        command: &VerifySourceCommand,
        reviewed_at: OffsetDateTime,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn set_review_state(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
        status: PolicyImportStatus,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn mark_failure(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
        status: PolicyImportStatus,
        code: &str,
        detail: &str,
    ) -> Result<PolicyImport, ApplicationError>;
    async fn save_compiled_bundle(
        &self,
        import_id: PolicyImportId,
        bundle: &PolicyBundle,
    ) -> Result<PolicyPack, ApplicationError>;
    async fn get_compiled_pack(
        &self,
        organization_id: OrganizationId,
        import_id: PolicyImportId,
    ) -> Result<Option<PolicyPack>, ApplicationError>;
}

#[async_trait]
pub trait SourceArtifactStore: Send + Sync {
    async fn put(&self, key: &str, content: Vec<u8>) -> Result<(), ApplicationError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, ApplicationError>;
}

#[async_trait]
pub trait PolicyDocumentParser: Send + Sync {
    async fn parse(
        &self,
        detected_mime_type: &str,
        content: Vec<u8>,
    ) -> Result<ParsedDocument, ApplicationError>;
}

#[async_trait]
pub trait PolicyExtractionModel: Send + Sync {
    async fn extract(&self, document: &ParsedDocument)
    -> Result<ExtractionBatch, ApplicationError>;
}

#[derive(Debug)]
pub struct CreatePolicyImport<R, S> {
    repository: R,
    artifacts: S,
}

impl<R, S> CreatePolicyImport<R, S>
where
    R: PolicyImportRepository,
    S: SourceArtifactStore,
{
    pub fn new(repository: R, artifacts: S) -> Self {
        Self {
            repository,
            artifacts,
        }
    }

    pub async fn execute(&self, input: NewPolicyImport) -> Result<PolicyImport, ApplicationError> {
        validate_new_import(&input)?;
        let id = PolicyImportId::new();
        let content_sha256 = sha256_hex(&input.content);
        let parent = match input.supersedes_import_id {
            Some(parent_id) => Some(
                self.repository
                    .get(input.organization_id, parent_id)
                    .await?
                    .ok_or_else(|| ApplicationError::NotFound(parent_id.to_string()))?,
            ),
            None => None,
        };
        let parent_lineage = parent.as_ref().map(|parent| {
            (
                parent.policy_source_id,
                parent.revision,
                parent.id,
                parent.status,
                parent.content_sha256.as_str(),
            )
        });
        let (policy_source_id, revision, supersedes_import_id) =
            resolve_import_lineage(parent_lineage, &content_sha256)?;
        let raw_object_key = format!(
            "organizations/{}/policy-imports/{}/raw/{}",
            input.organization_id, id, content_sha256
        );
        let now = OffsetDateTime::now_utc();
        let import = PolicyImport {
            id,
            organization_id: input.organization_id,
            policy_source_id,
            revision,
            supersedes_import_id,
            status: PolicyImportStatus::Uploading,
            input_kind: input.input_kind,
            source_type: input.source_type,
            title: input.title.trim().to_owned(),
            jurisdiction: input.jurisdiction.trim().to_owned(),
            effective_from: input.effective_from,
            source_url: input.source_url,
            original_filename: input.original_filename,
            declared_mime_type: input.declared_mime_type,
            detected_mime_type: input.detected_mime_type,
            byte_length: u64::try_from(input.content.len()).unwrap_or(u64::MAX),
            content_sha256,
            raw_object_key: raw_object_key.clone(),
            normalized_object_key: None,
            parser_kind: None,
            parser_version: None,
            model_provider: None,
            model_name: None,
            prompt_version: None,
            page_count: None,
            coverage: PolicyImportCoverage::default(),
            candidate_count: 0,
            verification_status: SourceVerificationStatus::Pending,
            verified_by: None,
            verified_at: None,
            verification_notes: None,
            failure_code: None,
            failure_detail: None,
            idempotency_key: input.idempotency_key,
            compiled_source_id: None,
            compiled_policy_pack_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        self.repository.create(&import).await?;
        if let Err(error) = self.artifacts.put(&raw_object_key, input.content).await {
            let _ = self
                .repository
                .mark_failure(
                    input.organization_id,
                    id,
                    PolicyImportStatus::FailedRetryable,
                    "artifact_write_failed",
                    "source storage is temporarily unavailable",
                )
                .await;
            return Err(error);
        }
        self.repository
            .transition(
                input.organization_id,
                id,
                PolicyImportStatus::Uploading,
                PolicyImportStatus::Queued,
            )
            .await
    }
}

fn resolve_import_lineage(
    parent: Option<(
        PolicySourceId,
        u32,
        PolicyImportId,
        PolicyImportStatus,
        &str,
    )>,
    content_sha256: &str,
) -> Result<(PolicySourceId, u32, Option<PolicyImportId>), ApplicationError> {
    let Some((policy_source_id, parent_revision, parent_id, parent_status, parent_hash)) = parent
    else {
        return Ok((PolicySourceId::new(), 1, None));
    };
    if parent_status != PolicyImportStatus::Compiled {
        return Err(ApplicationError::Conflict(
            "only a compiled import can be updated with a new source version".to_owned(),
        ));
    }
    if parent_hash == content_sha256 {
        return Err(ApplicationError::Conflict(
            "the uploaded source is unchanged".to_owned(),
        ));
    }
    let revision = parent_revision.checked_add(1).ok_or_else(|| {
        ApplicationError::Conflict("the policy source revision limit was reached".to_owned())
    })?;
    Ok((policy_source_id, revision, Some(parent_id)))
}

#[derive(Debug)]
pub struct ProcessPolicyImport<R, S, P, M> {
    repository: R,
    artifacts: S,
    parser: P,
    model: M,
}

impl<R, S, P, M> ProcessPolicyImport<R, S, P, M>
where
    R: PolicyImportRepository,
    S: SourceArtifactStore,
    P: PolicyDocumentParser,
    M: PolicyExtractionModel,
{
    pub fn new(repository: R, artifacts: S, parser: P, model: M) -> Self {
        Self {
            repository,
            artifacts,
            parser,
            model,
        }
    }

    pub async fn execute(
        &self,
        organization_id: OrganizationId,
        id: PolicyImportId,
    ) -> Result<PolicyImport, ApplicationError> {
        let import = self
            .repository
            .transition(
                organization_id,
                id,
                PolicyImportStatus::Queued,
                PolicyImportStatus::Parsing,
            )
            .await?;
        let raw = match self.artifacts.get(&import.raw_object_key).await {
            Ok(raw) => raw,
            Err(error) => {
                let (status, code, detail) = if matches!(error, ApplicationError::NotFound(_)) {
                    (
                        PolicyImportStatus::FailedTerminal,
                        "artifact_missing",
                        "the stored source artifact no longer exists",
                    )
                } else {
                    (
                        PolicyImportStatus::FailedRetryable,
                        "artifact_read_failed",
                        "source storage is temporarily unavailable",
                    )
                };
                let _ = self
                    .repository
                    .mark_failure(organization_id, id, status, code, detail)
                    .await;
                return Err(error);
            }
        };
        if sha256_hex(&raw) != import.content_sha256 {
            return self
                .repository
                .mark_failure(
                    organization_id,
                    id,
                    PolicyImportStatus::FailedTerminal,
                    "artifact_hash_mismatch",
                    "stored source artifact failed integrity verification",
                )
                .await
                .and_then(|_| {
                    Err(ApplicationError::InvalidRequest(
                        "stored source artifact failed integrity verification".to_owned(),
                    ))
                });
        }
        let document = match self.parser.parse(&import.detected_mime_type, raw).await {
            Ok(document) => document,
            Err(ApplicationError::InvalidRequest(detail)) if detail.starts_with("needs_ocr") => {
                return self
                    .repository
                    .mark_failure(
                        organization_id,
                        id,
                        PolicyImportStatus::NeedsOcr,
                        "needs_ocr",
                        "the PDF has no usable embedded text",
                    )
                    .await;
            }
            Err(error) => {
                let _ = self
                    .repository
                    .mark_failure(
                        organization_id,
                        id,
                        PolicyImportStatus::FailedTerminal,
                        "parse_failed",
                        "the source could not be parsed safely",
                    )
                    .await;
                return Err(error);
            }
        };
        let normalized = serde_json::to_vec(&document)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let normalized_sha256 = sha256_hex(&normalized);
        let normalized_object_key = format!(
            "organizations/{organization_id}/policy-imports/{id}/normalized/{normalized_sha256}.json"
        );
        if let Err(error) = self.artifacts.put(&normalized_object_key, normalized).await {
            let _ = self
                .repository
                .mark_failure(
                    organization_id,
                    id,
                    PolicyImportStatus::FailedRetryable,
                    "normalized_artifact_write_failed",
                    "normalized source storage is temporarily unavailable",
                )
                .await;
            return Err(error);
        }
        let initial_coverage = PolicyImportCoverage {
            total_chunks: u32::try_from(document.segments.len()).unwrap_or(u32::MAX),
            ..PolicyImportCoverage::default()
        };
        if let Err(error) = self
            .repository
            .mark_parsed(
                organization_id,
                id,
                &normalized_object_key,
                &document,
                &initial_coverage,
            )
            .await
        {
            let _ = self
                .repository
                .mark_failure(
                    organization_id,
                    id,
                    PolicyImportStatus::FailedRetryable,
                    "parse_persistence_failed",
                    "parsed source metadata could not be persisted",
                )
                .await;
            return Err(error);
        }
        let batch = match self.model.extract(&document).await {
            Ok(batch) => batch,
            Err(error) => {
                let _ = self
                    .repository
                    .mark_failure(
                        organization_id,
                        id,
                        PolicyImportStatus::FailedRetryable,
                        "extraction_failed",
                        "policy candidate extraction is temporarily unavailable",
                    )
                    .await;
                return Err(error);
            }
        };
        let candidates = extracted_candidates(&import, &document, &batch.candidates)?;
        match self
            .repository
            .replace_unreviewed_candidates(organization_id, id, &candidates, &batch)
            .await
        {
            Ok(import) => Ok(import),
            Err(error) => {
                let _ = self
                    .repository
                    .mark_failure(
                        organization_id,
                        id,
                        PolicyImportStatus::FailedRetryable,
                        "candidate_persistence_failed",
                        "extracted candidates could not be persisted",
                    )
                    .await;
                Err(error)
            }
        }
    }
}

pub async fn refresh_import_readiness<R: PolicyImportRepository>(
    repository: &R,
    organization_id: OrganizationId,
    import_id: PolicyImportId,
) -> Result<PolicyImport, ApplicationError> {
    let import = repository
        .get(organization_id, import_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(import_id.to_string()))?;
    let candidates = repository
        .list_candidates(organization_id, import_id)
        .await?;
    let readiness = PolicyImportReadiness::calculate(&import, &candidates);
    let status = if readiness.is_ready() {
        PolicyImportStatus::ReadyToCompile
    } else {
        PolicyImportStatus::ReviewRequired
    };
    repository
        .set_review_state(organization_id, import_id, status)
        .await
}

pub async fn review_policy_candidate<R: PolicyImportRepository>(
    repository: &R,
    command: ReviewCandidateCommand,
) -> Result<PolicyCandidate, ApplicationError> {
    if command.reviewer_id.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "reviewer_id is required".to_owned(),
        ));
    }
    if command.reviewer_id.chars().count() > 320 || command.notes.chars().count() > 4_000 {
        return Err(ApplicationError::InvalidRequest(
            "reviewer identity or notes exceed the configured limit".to_owned(),
        ));
    }
    if command.decision == PolicyCandidateStatus::Pending {
        return Err(ApplicationError::InvalidRequest(
            "review decision must be approved or rejected".to_owned(),
        ));
    }
    let import = repository
        .get(command.organization_id, command.policy_import_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.policy_import_id.to_string()))?;
    if import.status == PolicyImportStatus::Compiled {
        return Err(ApplicationError::Conflict(
            "compiled imports are immutable".to_owned(),
        ));
    }
    if !matches!(
        import.status,
        PolicyImportStatus::ReviewRequired | PolicyImportStatus::ReadyToCompile
    ) {
        return Err(ApplicationError::Conflict(
            "candidates can only be reviewed after extraction completes".to_owned(),
        ));
    }
    let before = repository
        .get_candidate(
            command.organization_id,
            command.policy_import_id,
            command.candidate_id,
        )
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.candidate_id.to_string()))?;
    let mut candidate = before.clone();
    if let Some(edit) = command.edit {
        validate_candidate_edit(&edit)?;
        candidate.statement = edit.statement;
        candidate.applicability = edit.applicability;
        candidate.exceptions = edit.exceptions;
        candidate.required_evidence = edit.required_evidence;
        candidate.suggested_severity = edit.suggested_severity;
        candidate.suggested_rule = edit.suggested_rule;
        candidate.mapping_status = edit.mapping_status;
    }
    let reviewed_at = OffsetDateTime::now_utc();
    candidate.status = command.decision;
    candidate.review = Some(CandidateReview {
        reviewer_id: command.reviewer_id.clone(),
        notes: command.notes.clone(),
        reviewed_at,
    });
    candidate.updated_at = reviewed_at;
    let review = PolicyCandidateReviewRecord {
        id: PolicyCandidateReviewId::new(),
        organization_id: command.organization_id,
        candidate_id: command.candidate_id,
        decision: command.decision,
        reviewer_id: command.reviewer_id,
        notes: command.notes,
        before_payload: serde_json::to_value(&before)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?,
        after_payload: serde_json::to_value(&candidate)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?,
        reviewed_at,
    };
    let saved = repository
        .save_candidate_review(&candidate, &review, command.expected_updated_at)
        .await?;
    refresh_import_readiness(
        repository,
        command.organization_id,
        command.policy_import_id,
    )
    .await?;
    Ok(saved)
}

pub async fn add_manual_policy_candidate<R: PolicyImportRepository>(
    repository: &R,
    command: ManualCandidateCommand,
) -> Result<PolicyCandidate, ApplicationError> {
    if command.statement.trim().is_empty() || command.source_excerpt.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "statement and exact source excerpt are required".to_owned(),
        ));
    }
    if command.reviewer_id.trim().is_empty()
        || command.source_excerpt.trim().is_empty()
        || command.reviewer_id.chars().count() > 320
        || command.statement.chars().count() > 2_000
        || command.source_excerpt.chars().count() > 4_000
    {
        return Err(ApplicationError::InvalidRequest(
            "manual candidate fields exceed the configured limit".to_owned(),
        ));
    }
    if command.suggested_rule.assertions.is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "manual candidate rule requires at least one deterministic assertion".to_owned(),
        ));
    }
    if sha256_hex(command.source_excerpt.as_bytes()) != command.locator.excerpt_sha256 {
        return Err(ApplicationError::InvalidRequest(
            "source excerpt hash does not match the locator".to_owned(),
        ));
    }
    let import = repository
        .get(command.organization_id, command.policy_import_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.policy_import_id.to_string()))?;
    if import.status == PolicyImportStatus::Compiled {
        return Err(ApplicationError::Conflict(
            "compiled imports are immutable".to_owned(),
        ));
    }
    if !matches!(
        import.status,
        PolicyImportStatus::ReviewRequired | PolicyImportStatus::ReadyToCompile
    ) {
        return Err(ApplicationError::Conflict(
            "manual candidates can only be added during review".to_owned(),
        ));
    }
    let existing = repository
        .list_candidates(command.organization_id, command.policy_import_id)
        .await?;
    let now = OffsetDateTime::now_utc();
    let candidate_id = PolicyCandidateId::new();
    let key = format!("MAN-{}", candidate_id.0.simple());
    let fingerprint =
        candidate_fingerprint(&import.content_sha256, &command.statement, &command.locator)?;
    let candidate = PolicyCandidate {
        id: candidate_id,
        organization_id: command.organization_id,
        policy_import_id: command.policy_import_id,
        position: u32::try_from(existing.len()).unwrap_or(u32::MAX),
        origin: PolicyCandidateOrigin::Human,
        fingerprint,
        key,
        statement: command.statement,
        locator: command.locator,
        source_excerpt: command.source_excerpt,
        applicability: command.applicability,
        exceptions: command.exceptions,
        required_evidence: command.required_evidence,
        suggested_severity: command.suggested_severity,
        suggested_rule: Some(command.suggested_rule),
        mapping_status: RuleMappingStatus::Ready,
        model_confidence: None,
        model_payload_sha256: None,
        status: PolicyCandidateStatus::Approved,
        review: Some(CandidateReview {
            reviewer_id: command.reviewer_id.clone(),
            notes: "Manually added from source".to_owned(),
            reviewed_at: now,
        }),
        created_at: now,
        updated_at: now,
    };
    let review = PolicyCandidateReviewRecord {
        id: PolicyCandidateReviewId::new(),
        organization_id: command.organization_id,
        candidate_id: candidate.id,
        decision: PolicyCandidateStatus::Approved,
        reviewer_id: command.reviewer_id,
        notes: "Manually added from source".to_owned(),
        before_payload: Value::Null,
        after_payload: serde_json::to_value(&candidate)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?,
        reviewed_at: now,
    };
    let saved = repository.add_manual_candidate(&candidate, &review).await?;
    refresh_import_readiness(
        repository,
        command.organization_id,
        command.policy_import_id,
    )
    .await?;
    Ok(saved)
}

pub async fn compile_policy_import<R: PolicyImportRepository>(
    repository: &R,
    command: CompilePolicyImportCommand,
) -> Result<PolicyPack, ApplicationError> {
    if let Some(pack) = repository
        .get_compiled_pack(command.organization_id, command.policy_import_id)
        .await?
    {
        return Ok(pack);
    }
    if command.key.trim().is_empty() || command.title.trim().is_empty() || command.version == 0 {
        return Err(ApplicationError::InvalidRequest(
            "pack key, title, and a positive version are required".to_owned(),
        ));
    }
    let import = repository
        .get(command.organization_id, command.policy_import_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.policy_import_id.to_string()))?;
    let candidates = repository
        .list_candidates(command.organization_id, command.policy_import_id)
        .await?;
    let readiness = PolicyImportReadiness::calculate(&import, &candidates);
    if !readiness.is_ready() || import.status != PolicyImportStatus::ReadyToCompile {
        return Err(ApplicationError::Conflict(format!(
            "import is not ready to compile: {}",
            readiness.blockers.join("; ")
        )));
    }
    let source_id = SourceId::new();
    let source = Source {
        id: source_id,
        organization_id: command.organization_id,
        source_type: import.source_type,
        title: import.title.clone(),
        jurisdiction: import.jurisdiction.clone(),
        effective_from: import.effective_from,
        content_sha256: import.content_sha256.clone(),
        confidence: match import.source_type {
            SourceType::PrimaryLaw | SourceType::OfficialGuidance => {
                SourceConfidence::OfficialVerified
            }
            SourceType::Standard | SourceType::CompanyPolicy => {
                SourceConfidence::SnapshotOfficialProvenance
            }
            SourceType::ExpertInterpretation => SourceConfidence::SnapshotUnverifiedProvenance,
        },
    };
    let approved: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.status == PolicyCandidateStatus::Approved)
        .collect();
    let mut obligations = Vec::with_capacity(approved.len());
    let mut rules = Vec::with_capacity(approved.len());
    for candidate in approved {
        let review = candidate.review.as_ref().ok_or_else(|| {
            ApplicationError::Conflict(format!(
                "approved candidate {} is missing review metadata",
                candidate.id
            ))
        })?;
        let suggestion = candidate.suggested_rule.as_ref().ok_or_else(|| {
            ApplicationError::Conflict(format!(
                "approved candidate {} has no deterministic rule",
                candidate.id
            ))
        })?;
        obligations.push(Obligation {
            id: ObligationId::new(),
            organization_id: command.organization_id,
            source_id,
            key: candidate.key.clone(),
            statement: candidate.statement.clone(),
            locator: candidate.locator.clone(),
            applicability: candidate.applicability.clone(),
            exceptions: candidate.exceptions.clone(),
            required_evidence: candidate.required_evidence.clone(),
            review: Some(ReviewerApproval {
                status: ReviewStatus::Approved,
                reviewer_id: review.reviewer_id.clone(),
                reviewed_at: review.reviewed_at,
            }),
        });
        rules.push(compiled_rule(candidate, suggestion));
    }
    let canonical = serde_json::to_vec(&(&source, &obligations, &rules))
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let pack = compile_policy_document(
        command.organization_id,
        PolicyDocument {
            key: command.key,
            version: command.version,
            title: command.title,
            status: ReviewStatus::Draft,
            rules,
        },
        sha256_hex(&canonical),
    )
    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    repository
        .save_compiled_bundle(
            command.policy_import_id,
            &PolicyBundle {
                pack,
                sources: vec![source],
                obligations,
            },
        )
        .await
}

fn compiled_rule(candidate: &PolicyCandidate, suggestion: &RuleSuggestion) -> CompiledRule {
    CompiledRule {
        id: format!(
            "{}_v1",
            candidate.key.to_ascii_lowercase().replace('-', "_")
        ),
        version: 1,
        obligation_key: candidate.key.clone(),
        severity: candidate.suggested_severity,
        trigger: suggestion.trigger.clone(),
        assertions: suggestion.assertions.clone(),
        evidence_required: suggestion.evidence_required.clone(),
        on_missing_evidence: MissingEvidencePolicy::NotObservable,
    }
}

fn validate_new_import(input: &NewPolicyImport) -> Result<(), ApplicationError> {
    if input.title.trim().is_empty() || input.jurisdiction.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "title and jurisdiction are required".to_owned(),
        ));
    }
    if input.content.is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "policy source cannot be empty".to_owned(),
        ));
    }
    if input.title.chars().count() > 240
        || input.jurisdiction.chars().count() > 120
        || input
            .source_url
            .as_ref()
            .is_some_and(|url| url.chars().count() > 2_048)
    {
        return Err(ApplicationError::InvalidRequest(
            "policy source metadata exceeds the configured limit".to_owned(),
        ));
    }
    if input
        .source_url
        .as_ref()
        .is_some_and(|url| !url.starts_with("https://") && !url.starts_with("http://"))
    {
        return Err(ApplicationError::InvalidRequest(
            "source_url must use http or https".to_owned(),
        ));
    }
    if input.idempotency_key.as_ref().is_some_and(|key| {
        key.len() > 128 || key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_graphic())
    }) {
        return Err(ApplicationError::InvalidRequest(
            "Idempotency-Key must contain 1-128 printable non-space ASCII characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_candidate_edit(edit: &CandidateEdit) -> Result<(), ApplicationError> {
    if edit.statement.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "candidate statement cannot be empty".to_owned(),
        ));
    }
    if edit.mapping_status == RuleMappingStatus::Ready
        && edit
            .suggested_rule
            .as_ref()
            .is_none_or(|rule| rule.assertions.is_empty())
    {
        return Err(ApplicationError::InvalidRequest(
            "a ready mapping requires a deterministic rule".to_owned(),
        ));
    }
    if edit.statement.chars().count() > 2_000
        || edit.exceptions.len() > 100
        || edit.required_evidence.len() > 100
        || edit
            .exceptions
            .iter()
            .chain(&edit.required_evidence)
            .any(|item| item.chars().count() > 256)
    {
        return Err(ApplicationError::InvalidRequest(
            "candidate fields exceed the configured limit".to_owned(),
        ));
    }
    Ok(())
}

fn extracted_candidates(
    import: &PolicyImport,
    document: &ParsedDocument,
    extracted: &[ExtractedCandidate],
) -> Result<Vec<PolicyCandidate>, ApplicationError> {
    let now = OffsetDateTime::now_utc();
    extracted
        .iter()
        .enumerate()
        .map(|(position, item)| {
            if !(0.0..=1.0).contains(&item.confidence) {
                return Err(ApplicationError::InvalidRequest(
                    "model confidence must be between zero and one".to_owned(),
                ));
            }
            let segment = document
                .segments
                .iter()
                .find(|segment| segment.ordinal == item.source_segment_ordinal)
                .ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "candidate references an unknown source segment".to_owned(),
                    )
                })?;
            if item.exact_excerpt.trim().is_empty() || !segment.text.contains(&item.exact_excerpt) {
                return Err(ApplicationError::InvalidRequest(
                    "candidate excerpt is not an exact substring of its source segment".to_owned(),
                ));
            }
            let excerpt_sha256 = sha256_hex(item.exact_excerpt.as_bytes());
            let locator = SourceLocator {
                page: segment.page,
                page_end: segment.page,
                section: segment.section.clone(),
                paragraph_start: segment.paragraph_start,
                paragraph_end: segment.paragraph_end,
                source_url: import.source_url.clone(),
                excerpt_sha256,
            };
            let fingerprint =
                candidate_fingerprint(&import.content_sha256, &item.statement, &locator)?;
            let model_payload_sha256 = sha256_hex(
                &serde_json::to_vec(item)
                    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?,
            );
            let candidate_id = PolicyCandidateId::new();
            Ok(PolicyCandidate {
                id: candidate_id,
                organization_id: import.organization_id,
                policy_import_id: import.id,
                position: u32::try_from(position).unwrap_or(u32::MAX),
                origin: PolicyCandidateOrigin::Model,
                fingerprint,
                key: format!("EXT-{}", candidate_id.0.simple()),
                statement: item.statement.trim().to_owned(),
                locator,
                source_excerpt: item.exact_excerpt.clone(),
                applicability: item.applicability.clone(),
                exceptions: item.exceptions.clone(),
                required_evidence: item.required_evidence.clone(),
                suggested_severity: item.suggested_severity,
                suggested_rule: item.suggested_rule.clone(),
                mapping_status: item.mapping_status,
                model_confidence: Some(item.confidence),
                model_payload_sha256: Some(model_payload_sha256),
                status: PolicyCandidateStatus::Pending,
                review: None,
                created_at: now,
                updated_at: now,
            })
        })
        .collect()
}

fn candidate_fingerprint(
    content_hash: &str,
    statement: &str,
    locator: &SourceLocator,
) -> Result<String, ApplicationError> {
    let canonical =
        serde_json::to_vec(&(content_hash, statement.trim().to_ascii_lowercase(), locator))
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(sha256_hex(&canonical))
}

pub fn detect_document_format(
    content: &[u8],
) -> Result<(DocumentFormat, &'static str), ApplicationError> {
    if content.starts_with(b"%PDF-") {
        Ok((DocumentFormat::Pdf, "application/pdf"))
    } else if content.starts_with(b"PK\x03\x04") {
        Ok((
            DocumentFormat::Docx,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ))
    } else if std::str::from_utf8(content).is_ok() && !content.contains(&0) {
        Ok((DocumentFormat::PlainText, "text/plain"))
    } else {
        Err(ApplicationError::InvalidRequest(
            "unsupported policy source format".to_owned(),
        ))
    }
}

#[must_use]
pub fn sha256_hex(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_allowed_document_magic() {
        assert_eq!(
            detect_document_format(b"%PDF-1.7\n")
                .expect("PDF should be detected")
                .0,
            DocumentFormat::Pdf
        );
        assert_eq!(
            detect_document_format(b"plain policy text")
                .expect("text should be detected")
                .0,
            DocumentFormat::PlainText
        );
    }

    #[test]
    fn rejects_binary_unknown_format() {
        assert!(detect_document_format(&[0, 1, 2, 3]).is_err());
    }

    #[test]
    fn validates_idempotency_key() {
        let input = NewPolicyImport {
            organization_id: OrganizationId::new(),
            input_kind: governance_domain::PolicyInputKind::PastedText,
            source_type: SourceType::CompanyPolicy,
            title: "Policy".to_owned(),
            jurisdiction: "internal".to_owned(),
            effective_from: None,
            source_url: None,
            original_filename: None,
            declared_mime_type: None,
            detected_mime_type: "text/plain".to_owned(),
            content: b"policy".to_vec(),
            idempotency_key: Some("contains space".to_owned()),
            supersedes_import_id: None,
        };
        assert!(validate_new_import(&input).is_err());
    }

    #[test]
    fn stable_sha_has_expected_length() {
        assert_eq!(sha256_hex(b"policy").len(), 64);
    }

    #[test]
    fn changed_content_advances_the_existing_source_lineage() {
        let expected_source_id = PolicySourceId::new();
        let parent_id = PolicyImportId::new();
        let old_hash = sha256_hex(b"old policy");

        let (source_id, revision, supersedes) = resolve_import_lineage(
            Some((
                expected_source_id,
                4,
                parent_id,
                PolicyImportStatus::Compiled,
                &old_hash,
            )),
            &sha256_hex(b"new policy"),
        )
        .expect("changed content should create a revision");

        assert_eq!(source_id, expected_source_id);
        assert_eq!(revision, 5);
        assert_eq!(supersedes, Some(parent_id));
    }

    #[test]
    fn unchanged_content_does_not_create_a_revision() {
        let source_id = PolicySourceId::new();
        let parent_id = PolicyImportId::new();
        let same_hash = sha256_hex(b"same policy");

        assert!(
            resolve_import_lineage(
                Some((
                    source_id,
                    1,
                    parent_id,
                    PolicyImportStatus::Compiled,
                    &same_hash
                )),
                &same_hash,
            )
            .is_err()
        );
    }
}
