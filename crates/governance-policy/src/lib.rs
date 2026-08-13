//! Policy-pack validation and compilation for database import boundaries.

use governance_domain::{
    CompiledRule, Obligation, ObligationId, OrganizationId, PolicyBundle, PolicyPack, PolicyPackId,
    ReviewStatus, ReviewerApproval, Source, SourceConfidence, SourceId, SourceLocator, SourceType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    pub key: String,
    pub version: u32,
    pub title: String,
    pub status: ReviewStatus,
    pub rules: Vec<CompiledRule>,
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("invalid policy: {0}")]
    InvalidPolicy(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyImportRequest {
    pub key: String,
    pub version: u32,
    pub title: String,
    pub rules: Vec<CompiledRule>,
    pub sources: Vec<PolicySourceImport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySourceImport {
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    pub content_sha256: String,
    pub confidence: SourceConfidence,
    pub obligations: Vec<ObligationImport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObligationImport {
    pub key: String,
    pub statement: String,
    pub locator: SourceLocator,
    #[serde(default)]
    pub applicability: Value,
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    pub reviewer_id: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub reviewed_at: Option<OffsetDateTime>,
}

/// Structurally validates a policy document received at an import boundary.
fn validate_policy_document(document: &PolicyDocument) -> Result<(), PolicyError> {
    if document.key.trim().is_empty() {
        return Err(PolicyError::InvalidPolicy(
            "policy key cannot be empty".to_owned(),
        ));
    }
    if document.rules.is_empty() {
        return Err(PolicyError::InvalidPolicy(
            "policy must contain at least one rule".to_owned(),
        ));
    }
    if document.rules.iter().any(|rule| rule.assertions.is_empty()) {
        return Err(PolicyError::InvalidPolicy(
            "every rule must contain at least one assertion".to_owned(),
        ));
    }
    Ok(())
}

/// Compiles a validated JSON/API policy document for database persistence.
///
/// # Errors
///
/// Returns an error when the document is structurally incomplete.
pub fn compile_policy_document(
    organization_id: OrganizationId,
    document: PolicyDocument,
    content_sha256: String,
) -> Result<PolicyPack, PolicyError> {
    validate_policy_document(&document)?;
    let published_at = (document.status == ReviewStatus::Approved).then(OffsetDateTime::now_utc);

    Ok(PolicyPack {
        id: PolicyPackId::new(),
        organization_id,
        key: document.key,
        version: document.version,
        title: document.title,
        status: document.status,
        content_sha256,
        published_at,
        rules: document.rules,
    })
}

/// Builds a complete draft policy aggregate from the canonical JSON import contract.
///
/// # Errors
///
/// Returns an error when hashes, reviews, rule references, or required fields are invalid.
pub fn build_policy_bundle(
    organization_id: OrganizationId,
    request: &PolicyImportRequest,
) -> Result<PolicyBundle, PolicyError> {
    if request.key.trim().is_empty() || request.rules.is_empty() || request.sources.is_empty() {
        return Err(PolicyError::InvalidPolicy(
            "key, at least one rule, and at least one source are required".to_owned(),
        ));
    }
    let canonical = serde_json::to_vec(request)
        .map_err(|error| PolicyError::InvalidPolicy(error.to_string()))?;
    let pack = compile_policy_document(
        organization_id,
        PolicyDocument {
            key: request.key.clone(),
            version: request.version,
            title: request.title.clone(),
            status: ReviewStatus::Draft,
            rules: request.rules.clone(),
        },
        format!("{:x}", Sha256::digest(canonical)),
    )?;
    let mut sources = Vec::with_capacity(request.sources.len());
    let mut obligations = Vec::new();
    for imported_source in &request.sources {
        if !valid_sha256(&imported_source.content_sha256) {
            return Err(PolicyError::InvalidPolicy(
                "every source content_sha256 must be a 64-character hexadecimal digest".to_owned(),
            ));
        }
        let source_id = SourceId::new();
        sources.push(Source {
            id: source_id,
            organization_id,
            source_type: imported_source.source_type,
            title: imported_source.title.clone(),
            jurisdiction: imported_source.jurisdiction.clone(),
            effective_from: None,
            content_sha256: imported_source.content_sha256.to_ascii_lowercase(),
            confidence: imported_source.confidence,
        });
        for imported_obligation in &imported_source.obligations {
            if !valid_sha256(&imported_obligation.locator.excerpt_sha256) {
                return Err(PolicyError::InvalidPolicy(
                    "every obligation excerpt_sha256 must be a 64-character hexadecimal digest"
                        .to_owned(),
                ));
            }
            let review = match (
                imported_obligation.reviewer_id.as_ref(),
                imported_obligation.reviewed_at,
            ) {
                (Some(reviewer_id), Some(reviewed_at)) => Some(ReviewerApproval {
                    status: ReviewStatus::Approved,
                    reviewer_id: reviewer_id.clone(),
                    reviewed_at,
                }),
                (None, None) => None,
                _ => {
                    return Err(PolicyError::InvalidPolicy(
                        "obligation reviewer_id and reviewed_at must be supplied together"
                            .to_owned(),
                    ));
                }
            };
            let mut locator = imported_obligation.locator.clone();
            locator.excerpt_sha256.make_ascii_lowercase();
            obligations.push(Obligation {
                id: ObligationId::new(),
                organization_id,
                source_id,
                key: imported_obligation.key.clone(),
                statement: imported_obligation.statement.clone(),
                locator,
                applicability: imported_obligation.applicability.clone(),
                exceptions: imported_obligation.exceptions.clone(),
                required_evidence: imported_obligation.required_evidence.clone(),
                review,
            });
        }
    }
    let obligation_keys: std::collections::BTreeSet<&str> =
        obligations.iter().map(|item| item.key.as_str()).collect();
    if obligation_keys.len() != obligations.len() {
        return Err(PolicyError::InvalidPolicy(
            "obligation keys must be unique across all imported sources".to_owned(),
        ));
    }
    if let Some(rule) = request
        .rules
        .iter()
        .find(|rule| !obligation_keys.contains(rule.obligation_key.as_str()))
    {
        return Err(PolicyError::InvalidPolicy(format!(
            "rule {} references missing obligation {}",
            rule.id, rule.obligation_key
        )));
    }
    Ok(PolicyBundle {
        pack,
        sources,
        obligations,
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use governance_domain::{
        EventMatcher, EventType, MissingEvidencePolicy, RuleAssertion, Severity,
    };

    fn policy_document() -> PolicyDocument {
        PolicyDocument {
            key: "refund-governance".to_owned(),
            version: 1,
            title: "Refund governance".to_owned(),
            status: ReviewStatus::Approved,
            rules: vec![CompiledRule {
                id: "refund_requires_approval".to_owned(),
                version: 1,
                obligation_key: "REFUND-004".to_owned(),
                severity: Severity::Critical,
                trigger: EventMatcher {
                    event_type: EventType::ToolCall,
                    name: Some("issue_refund".to_owned()),
                    attribute_equals: BTreeMap::default(),
                    numeric_argument: None,
                },
                assertions: vec![RuleAssertion::ExistsBefore {
                    matcher: EventMatcher {
                        event_type: EventType::HumanApprovalDecision,
                        name: Some("approval".to_owned()),
                        attribute_equals: BTreeMap::default(),
                        numeric_argument: None,
                    },
                }],
                evidence_required: vec!["human_approval_decision".to_owned()],
                on_missing_evidence: MissingEvidencePolicy::NotObservable,
            }],
        }
    }

    #[test]
    fn compiles_approved_pack_with_content_hash() {
        let pack =
            compile_policy_document(OrganizationId::new(), policy_document(), "a".repeat(64))
                .expect("policy should compile");
        assert_eq!(pack.rules.len(), 1);
        assert_eq!(pack.content_sha256.len(), 64);
        assert!(pack.published_at.is_some());
    }

    #[test]
    fn rejects_empty_rule_set() {
        let mut document = policy_document();
        document.rules.clear();
        assert!(matches!(
            compile_policy_document(OrganizationId::new(), document, "a".repeat(64)),
            Err(PolicyError::InvalidPolicy(_))
        ));
    }

    #[test]
    fn canonical_import_fixture_stays_a_draft() {
        let request: PolicyImportRequest = serde_json::from_str(include_str!(
            "../../../fixtures/policies/refund-governance.import.json"
        ))
        .expect("canonical fixture should remain compatible");
        let bundle = build_policy_bundle(OrganizationId::new(), &request)
            .expect("canonical fixture should build");
        assert_eq!(bundle.pack.status, ReviewStatus::Draft);
        assert!(bundle.pack.published_at.is_none());
        assert!(!bundle.obligations.is_empty());
    }
}
