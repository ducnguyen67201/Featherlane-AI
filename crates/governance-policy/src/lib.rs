//! Policy-pack validation and compilation for database import boundaries.

use governance_domain::{CompiledRule, OrganizationId, PolicyPack, PolicyPackId, ReviewStatus};
use serde::{Deserialize, Serialize};
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
}
