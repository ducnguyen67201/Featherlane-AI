#![allow(clippy::missing_errors_doc, clippy::too_many_lines)]

use std::collections::HashSet;

use async_trait::async_trait;
use governance_domain::{
    CompiledRule, MissingEvidencePolicy, Obligation, ObligationId, OrganizationId, PolicyBundle,
    PolicyCandidateId, PolicyCandidateStatus, PolicyCollection, PolicyCollectionId,
    PolicyCollectionImport, PolicyCollectionReadiness, PolicyCollectionStatus, PolicyImport,
    PolicyImportId, PolicyImportStatus, PolicyPack, PolicySourceId, ReviewStatus, ReviewerApproval,
    Source, SourceConfidence, SourceId, SourceType,
};
use governance_policy::{PolicyDocument, compile_policy_document};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApplicationError, PolicyImportRepository};

#[derive(Clone, Debug, Deserialize)]
pub struct CreatePolicyCollectionCommand {
    pub organization_id: OrganizationId,
    pub key: String,
    pub version: u32,
    pub title: String,
    pub created_by: String,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CompilePolicyCollectionCommand {
    pub organization_id: OrganizationId,
    pub policy_collection_id: PolicyCollectionId,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClonePolicyCollectionCommand {
    pub organization_id: OrganizationId,
    pub source_collection_id: PolicyCollectionId,
    pub version: u32,
    pub title: String,
    pub created_by: String,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectionCompilationSnapshot {
    pub imports: Vec<(PolicyImportId, OffsetDateTime)>,
    pub candidates: Vec<(PolicyCandidateId, OffsetDateTime)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompiledCollectionSource {
    pub policy_import_id: PolicyImportId,
    pub source_id: SourceId,
}

#[async_trait]
pub trait PolicyCollectionRepository: Send + Sync {
    async fn create(
        &self,
        collection: &PolicyCollection,
    ) -> Result<PolicyCollection, ApplicationError>;
    async fn get(
        &self,
        organization_id: OrganizationId,
        id: PolicyCollectionId,
    ) -> Result<Option<PolicyCollection>, ApplicationError>;
    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PolicyCollection>, ApplicationError>;
    async fn members(
        &self,
        organization_id: OrganizationId,
        id: PolicyCollectionId,
    ) -> Result<Vec<PolicyCollectionImport>, ApplicationError>;
    async fn add_import(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        import: &PolicyImport,
    ) -> Result<PolicyCollectionImport, ApplicationError>;
    async fn remove_import(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        import_id: PolicyImportId,
    ) -> Result<(), ApplicationError>;
    async fn save_compiled_bundle(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
        bundle: &PolicyBundle,
        snapshot: &CollectionCompilationSnapshot,
        sources: &[CompiledCollectionSource],
    ) -> Result<PolicyPack, ApplicationError>;
    async fn compiled_pack(
        &self,
        organization_id: OrganizationId,
        collection_id: PolicyCollectionId,
    ) -> Result<Option<PolicyPack>, ApplicationError>;
}

pub async fn create_policy_collection<R: PolicyCollectionRepository>(
    repository: &R,
    command: CreatePolicyCollectionCommand,
) -> Result<PolicyCollection, ApplicationError> {
    if command.key.trim().is_empty()
        || command.title.trim().is_empty()
        || command.created_by.trim().is_empty()
        || command.version == 0
    {
        return Err(ApplicationError::InvalidRequest(
            "collection key, positive version, title, and actor are required".to_owned(),
        ));
    }
    if command.key.len() > 120 || command.title.chars().count() > 240 {
        return Err(ApplicationError::InvalidRequest(
            "collection metadata exceeds the configured limit".to_owned(),
        ));
    }
    let now = OffsetDateTime::now_utc();
    repository
        .create(&PolicyCollection {
            id: PolicyCollectionId::new(),
            organization_id: command.organization_id,
            key: command.key.trim().to_owned(),
            version: command.version,
            title: command.title.trim().to_owned(),
            status: PolicyCollectionStatus::Draft,
            compiled_policy_pack_id: None,
            created_by: command.created_by,
            idempotency_key: command.idempotency_key,
            created_at: now,
            updated_at: now,
        })
        .await
}

pub async fn policy_collection_readiness<C, I>(
    collections: &C,
    imports: &I,
    organization_id: OrganizationId,
    collection_id: PolicyCollectionId,
) -> Result<PolicyCollectionReadiness, ApplicationError>
where
    C: PolicyCollectionRepository,
    I: PolicyImportRepository,
{
    collections
        .get(organization_id, collection_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(collection_id.to_string()))?;
    let members = collections.members(organization_id, collection_id).await?;
    let mut sources = Vec::with_capacity(members.len());
    for member in members {
        let import = imports
            .get(organization_id, member.policy_import_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(member.policy_import_id.to_string()))?;
        let candidates = imports
            .list_candidates(organization_id, member.policy_import_id)
            .await?;
        sources.push((import, candidates));
    }
    Ok(PolicyCollectionReadiness::calculate(&sources))
}

pub async fn clone_policy_collection<C, I>(
    collections: &C,
    imports: &I,
    command: ClonePolicyCollectionCommand,
) -> Result<PolicyCollection, ApplicationError>
where
    C: PolicyCollectionRepository,
    I: PolicyImportRepository,
{
    let source = collections
        .get(command.organization_id, command.source_collection_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.source_collection_id.to_string()))?;
    if command.version <= source.version {
        return Err(ApplicationError::InvalidRequest(
            "the cloned collection version must be greater than the source version".to_owned(),
        ));
    }
    let members = collections
        .members(command.organization_id, command.source_collection_id)
        .await?;
    let cloned = create_policy_collection(
        collections,
        CreatePolicyCollectionCommand {
            organization_id: command.organization_id,
            key: source.key,
            version: command.version,
            title: command.title,
            created_by: command.created_by,
            idempotency_key: command.idempotency_key,
        },
    )
    .await?;
    for member in members {
        let import = imports
            .get(command.organization_id, member.policy_import_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(member.policy_import_id.to_string()))?;
        collections
            .add_import(command.organization_id, cloned.id, &import)
            .await?;
    }
    Ok(cloned)
}

pub async fn compile_policy_collection<C, I>(
    collections: &C,
    imports: &I,
    command: CompilePolicyCollectionCommand,
) -> Result<PolicyPack, ApplicationError>
where
    C: PolicyCollectionRepository,
    I: PolicyImportRepository,
{
    if let Some(pack) = collections
        .compiled_pack(command.organization_id, command.policy_collection_id)
        .await?
    {
        return Ok(pack);
    }
    let collection = collections
        .get(command.organization_id, command.policy_collection_id)
        .await?
        .ok_or_else(|| ApplicationError::NotFound(command.policy_collection_id.to_string()))?;
    if collection.status != PolicyCollectionStatus::Draft {
        return Err(ApplicationError::Conflict(
            "policy collection is immutable after compilation".to_owned(),
        ));
    }
    let members = collections
        .members(command.organization_id, command.policy_collection_id)
        .await?;
    let mut material = Vec::with_capacity(members.len());
    for member in members {
        let import = imports
            .get(command.organization_id, member.policy_import_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(member.policy_import_id.to_string()))?;
        if !matches!(
            import.status,
            PolicyImportStatus::ReadyToCompile | PolicyImportStatus::Compiled
        ) {
            return Err(ApplicationError::Conflict(format!(
                "source {} is not ready to compile",
                import.id
            )));
        }
        let candidates = imports
            .list_candidates(command.organization_id, member.policy_import_id)
            .await?;
        material.push((import, candidates));
    }
    let readiness = PolicyCollectionReadiness::calculate(&material);
    if !readiness.is_ready() {
        let mut messages = readiness.collection_blockers;
        messages.extend(readiness.blockers.into_iter().flat_map(|blocker| {
            blocker
                .blockers
                .into_iter()
                .map(move |message| format!("{}: {message}", blocker.title))
        }));
        return Err(ApplicationError::Conflict(messages.join("; ")));
    }

    let mut bundle_sources = Vec::with_capacity(material.len());
    let mut obligations = Vec::new();
    let mut rules = Vec::new();
    let mut assignments = Vec::with_capacity(material.len());
    let mut seen_rule_ids = HashSet::new();
    for (import, candidates) in &material {
        let source_id = import
            .compiled_source_id
            .unwrap_or_else(|| stable_source_id(import.policy_source_id));
        bundle_sources.push(source_for_import(import, source_id));
        assignments.push(CompiledCollectionSource {
            policy_import_id: import.id,
            source_id,
        });
        let namespace = obligation_namespace(import.policy_source_id);
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.status == PolicyCandidateStatus::Approved)
        {
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
            let obligation_key = format!("{namespace}__{}", candidate.key);
            let rule_id = format!(
                "{}_v1",
                obligation_key.to_ascii_lowercase().replace('-', "_")
            );
            if !seen_rule_ids.insert(rule_id.clone()) {
                return Err(ApplicationError::Conflict(format!(
                    "duplicate compiled rule id: {rule_id}"
                )));
            }
            obligations.push(Obligation {
                id: ObligationId::new(),
                organization_id: command.organization_id,
                source_id,
                key: obligation_key.clone(),
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
            rules.push(CompiledRule {
                id: rule_id,
                version: 1,
                obligation_key,
                severity: candidate.suggested_severity,
                trigger: suggestion.trigger.clone(),
                assertions: suggestion.assertions.clone(),
                evidence_required: suggestion.evidence_required.clone(),
                on_missing_evidence: MissingEvidencePolicy::NotObservable,
            });
        }
    }
    bundle_sources.sort_by_key(|source| source.id);
    obligations.sort_by(|left, right| left.key.cmp(&right.key));
    rules.sort_by(|left, right| left.id.cmp(&right.id));
    let canonical = serde_json::to_vec(&(&bundle_sources, &obligations, &rules))
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let pack = compile_policy_document(
        command.organization_id,
        PolicyDocument {
            key: collection.key,
            version: collection.version,
            title: collection.title,
            status: ReviewStatus::Draft,
            rules,
        },
        format!("{:x}", Sha256::digest(canonical)),
    )
    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let mut snapshot = CollectionCompilationSnapshot {
        imports: material
            .iter()
            .map(|(import, _)| (import.id, import.updated_at))
            .collect(),
        candidates: material
            .iter()
            .flat_map(|(_, candidates)| {
                candidates
                    .iter()
                    .map(|candidate| (candidate.id, candidate.updated_at))
            })
            .collect(),
    };
    snapshot.imports.sort_by_key(|(id, _)| *id);
    snapshot.candidates.sort_by_key(|(id, _)| *id);
    collections
        .save_compiled_bundle(
            command.organization_id,
            command.policy_collection_id,
            &PolicyBundle {
                pack,
                sources: bundle_sources,
                obligations,
            },
            &snapshot,
            &assignments,
        )
        .await
}

#[must_use]
pub fn obligation_namespace(policy_source_id: PolicySourceId) -> String {
    let compact = policy_source_id.0.simple().to_string();
    format!("s_{}", &compact[..12])
}

fn stable_source_id(policy_source_id: PolicySourceId) -> SourceId {
    SourceId(Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("featherlane-policy-source:{policy_source_id}").as_bytes(),
    ))
}

fn source_for_import(import: &PolicyImport, source_id: SourceId) -> Source {
    Source {
        id: source_id,
        organization_id: import.organization_id,
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
    }
}
