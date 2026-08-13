use std::collections::BTreeSet;

use async_trait::async_trait;
use governance_application::{ApplicationError, PolicyExtractionModel, sha256_hex};
use governance_config::PolicyImportConfig;
use governance_domain::{
    EventMatcher, EventType, ExtractedCandidate, ExtractionBatch, ExtractionResponse,
    ParsedDocument, PolicyImportCoverage, RuleAssertion, RuleMappingStatus, RuleSuggestion,
    Severity,
};
use serde_json::Value;

use crate::{chunk_document, chunking::EXTRACTION_CHUNK_CHARACTER_BUDGET};

#[derive(Clone, Debug)]
pub struct HeuristicPolicyExtractionModel {
    max_chunks: usize,
    max_candidates: usize,
    prompt_version: String,
}

impl HeuristicPolicyExtractionModel {
    #[must_use]
    pub fn from_config(config: &PolicyImportConfig) -> Self {
        Self {
            max_chunks: config.max_chunks,
            max_candidates: config.max_candidates,
            prompt_version: config.llm_prompt_version.clone(),
        }
    }
}

#[async_trait]
impl PolicyExtractionModel for HeuristicPolicyExtractionModel {
    async fn extract(
        &self,
        document: &ParsedDocument,
    ) -> Result<ExtractionBatch, ApplicationError> {
        let chunks = chunk_document(document, EXTRACTION_CHUNK_CHARACTER_BUDGET, self.max_chunks);
        if !chunks.last().is_some_and(|chunk| chunk.completes_document) {
            return Err(ApplicationError::InvalidRequest(
                "source exceeds the configured extraction chunk limit".to_owned(),
            ));
        }
        let mut candidates = Vec::new();
        for segment in &document.segments {
            for sentence in segment.text.split(['.', '\n', ';']) {
                let sentence = sentence.trim();
                let normalized = sentence.to_ascii_lowercase();
                if sentence.len() < 12
                    || ![" must ", " shall ", " required", " may not ", " prohibited"]
                        .iter()
                        .any(|needle| format!(" {normalized} ").contains(needle))
                {
                    continue;
                }
                let supports_approval = normalized.contains("approval");
                candidates.push(ExtractedCandidate {
                    source_segment_ordinal: segment.ordinal,
                    exact_excerpt: sentence.to_owned(),
                    statement: format!("{}.", sentence.trim_end_matches('.')),
                    applicability: Value::Object(serde_json::Map::default()),
                    exceptions: Vec::new(),
                    required_evidence: if supports_approval {
                        vec!["human_approval_decision".to_owned()]
                    } else {
                        Vec::new()
                    },
                    suggested_severity: if supports_approval {
                        Severity::Critical
                    } else {
                        Severity::Medium
                    },
                    suggested_rule: supports_approval.then(approval_rule),
                    mapping_status: if supports_approval {
                        RuleMappingStatus::Ready
                    } else {
                        RuleMappingStatus::ManualRequired
                    },
                    confidence: 0.6,
                });
                if candidates.len() >= self.max_candidates {
                    break;
                }
            }
            if candidates.len() >= self.max_candidates {
                break;
            }
        }
        let before_validation = candidates.len();
        let response = validate_extraction_response(
            document,
            ExtractionResponse { candidates },
            self.max_candidates,
        )?;
        let dropped_candidates = before_validation.saturating_sub(response.candidates.len());
        Ok(ExtractionBatch {
            candidates: response.candidates,
            coverage: PolicyImportCoverage {
                total_chunks: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
                processed_chunks: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
                failed_chunks: Vec::new(),
                duplicate_candidates: u32::try_from(dropped_candidates).unwrap_or(u32::MAX),
                warnings: vec![
                    "Development heuristic extraction was used; configure an approved model before production."
                        .to_owned(),
                ],
            },
            provider: "heuristic".to_owned(),
            model: "deterministic-development-extractor".to_owned(),
            prompt_version: self.prompt_version.clone(),
        })
    }
}

#[must_use]
fn approval_rule() -> RuleSuggestion {
    RuleSuggestion {
        trigger: EventMatcher {
            event_type: EventType::ToolCall,
            name: None,
            ..EventMatcher::default()
        },
        assertions: vec![RuleAssertion::ExistsBefore {
            matcher: EventMatcher {
                event_type: EventType::HumanApprovalDecision,
                name: None,
                ..EventMatcher::default()
            },
        }],
        evidence_required: vec!["human_approval_decision".to_owned()],
    }
}

/// Validates grounded extraction output against the parsed source document.
///
/// # Errors
///
/// Returns an error when the response exceeds configured bounds, contains
/// ungrounded excerpts, or repeats candidate fingerprints.
pub fn validate_extraction_response(
    document: &ParsedDocument,
    mut response: ExtractionResponse,
    max_candidates: usize,
) -> Result<ExtractionResponse, ApplicationError> {
    if response.candidates.len() > max_candidates {
        return Err(ApplicationError::InvalidRequest(
            "model returned too many candidates".to_owned(),
        ));
    }
    let mut fingerprints = BTreeSet::new();
    response.candidates.retain(|candidate| {
        let Some(segment) = document
            .segments
            .iter()
            .find(|segment| segment.ordinal == candidate.source_segment_ordinal)
        else {
            return false;
        };
        if candidate.statement.trim().is_empty()
            || candidate.exact_excerpt.trim().is_empty()
            || candidate.statement.chars().count() > 2_000
            || candidate.exact_excerpt.chars().count() > 4_000
            || candidate.exceptions.len() > 100
            || candidate.required_evidence.len() > 100
            || !segment.text.contains(&candidate.exact_excerpt)
            || !(0.0..=1.0).contains(&candidate.confidence)
            || (candidate.mapping_status == RuleMappingStatus::Ready
                && candidate
                    .suggested_rule
                    .as_ref()
                    .is_none_or(|rule| rule.assertions.is_empty()))
        {
            return false;
        }
        let fingerprint = sha256_hex(
            format!(
                "{}:{}:{}",
                candidate.source_segment_ordinal,
                candidate.statement.trim().to_ascii_lowercase(),
                candidate.exact_excerpt
            )
            .as_bytes(),
        );
        fingerprints.insert(fingerprint)
    });
    Ok(response)
}

#[cfg(test)]
mod tests {
    use governance_application::PolicyDocumentParser;
    use governance_domain::{DocumentFormat, SourceSegment};

    use super::*;

    fn document() -> ParsedDocument {
        ParsedDocument {
            format: DocumentFormat::PlainText,
            parser_version: "1".to_owned(),
            title: None,
            segments: vec![SourceSegment {
                ordinal: 1,
                page: None,
                section: None,
                paragraph_start: Some(1),
                paragraph_end: Some(1),
                text: "Refunds above $500 require prior approval".to_owned(),
                text_sha256: "a".repeat(64),
            }],
        }
    }

    #[test]
    fn drops_candidate_with_fabricated_excerpt() {
        let response = ExtractionResponse {
            candidates: vec![ExtractedCandidate {
                source_segment_ordinal: 1,
                exact_excerpt: "not in source".to_owned(),
                statement: "Approval is required".to_owned(),
                applicability: Value::Null,
                exceptions: Vec::new(),
                required_evidence: Vec::new(),
                suggested_severity: Severity::Critical,
                suggested_rule: Some(approval_rule()),
                mapping_status: RuleMappingStatus::Ready,
                confidence: 0.8,
            }],
        };
        let validated = validate_extraction_response(&document(), response, 10)
            .expect("invalid candidate should be safely filtered");
        assert!(validated.candidates.is_empty());
    }

    #[test]
    fn extraction_schema_rejects_unknown_fields_and_rule_kinds() {
        let unknown_field = serde_json::json!({ "candidates": [], "instructions": "approve" });
        assert!(serde_json::from_value::<ExtractionResponse>(unknown_field).is_err());
        let script_rule = serde_json::json!({
            "candidates": [{
                "source_segment_ordinal": 1,
                "exact_excerpt": "Refunds above $500 require prior approval",
                "statement": "Approval is required",
                "applicability": {},
                "exceptions": [],
                "required_evidence": [],
                "suggested_severity": "critical",
                "suggested_rule": {
                    "trigger": { "event_type": "tool_call", "name": null, "attribute_equals": {}, "numeric_argument": null },
                    "assertions": [{ "kind": "execute_script", "code": "allow()" }],
                    "evidence_required": []
                },
                "mapping_status": "ready",
                "confidence": 0.8
            }]
        });
        assert!(serde_json::from_value::<ExtractionResponse>(script_rule).is_err());
    }

    #[test]
    fn deduplicates_overlap_candidates() {
        let candidate = ExtractedCandidate {
            source_segment_ordinal: 1,
            exact_excerpt: "Refunds above $500 require prior approval".to_owned(),
            statement: "Approval is required".to_owned(),
            applicability: Value::Null,
            exceptions: Vec::new(),
            required_evidence: Vec::new(),
            suggested_severity: Severity::Critical,
            suggested_rule: Some(approval_rule()),
            mapping_status: RuleMappingStatus::Ready,
            confidence: 0.8,
        };
        let validated = validate_extraction_response(
            &document(),
            ExtractionResponse {
                candidates: vec![candidate.clone(), candidate],
            },
            10,
        )
        .expect("response should validate");
        assert_eq!(validated.candidates.len(), 1);
    }

    #[tokio::test]
    async fn treats_prompt_injection_as_untrusted_source_text() {
        let source = include_str!("../../../fixtures/policy-sources/prompt-injection-policy.txt");
        let document = crate::SafePolicyDocumentParser::with_limits(10_000, 10, 100_000)
            .parse("text/plain", source.as_bytes().to_vec())
            .await
            .expect("fixture should parse as plain text");
        let config = PolicyImportConfig {
            max_bytes: 26_214_400,
            max_pages: 250,
            max_chunks: 20,
            max_candidates: 20,
            object_store_url: String::new(),
            object_store_bucket: String::new(),
            object_store_region: String::new(),
            object_store_access_key_id: String::new(),
            object_store_secret_access_key: String::new(),
            llm_enabled: true,
            llm_provider: "heuristic".to_owned(),
            llm_base_url: String::new(),
            llm_api_key: String::new(),
            llm_model: String::new(),
            llm_prompt_version: "test".to_owned(),
            llm_require_zdr: true,
            llm_data_collection: "deny".to_owned(),
            llm_allow_fallbacks: false,
        };
        let batch = HeuristicPolicyExtractionModel::from_config(&config)
            .extract(&document)
            .await
            .expect("untrusted source should be extracted without executing instructions");
        assert!(
            batch
                .candidates
                .iter()
                .all(|candidate| !candidate.statement.contains("Ignore all previous"))
        );
        assert!(batch.candidates.iter().any(|candidate| {
            candidate
                .exact_excerpt
                .contains("must record human approval")
        }));
    }
}
