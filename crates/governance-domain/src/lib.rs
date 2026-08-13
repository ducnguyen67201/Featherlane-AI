//! Pure domain contracts for Featherlane's governance evaluation engine.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }
    };
}

id_type!(OrganizationId);
id_type!(SourceId);
id_type!(ObligationId);
id_type!(PolicyPackId);
id_type!(TargetId);
id_type!(ScenarioId);
id_type!(EvalRunId);
id_type!(InvocationId);
id_type!(EventId);
id_type!(RuleResultId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    PrimaryLaw,
    OfficialGuidance,
    Standard,
    CompanyPolicy,
    ExpertInterpretation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Draft,
    Approved,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConfidence {
    OfficialVerified,
    SnapshotOfficialProvenance,
    SnapshotUnverifiedProvenance,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceLocator {
    pub page: Option<u32>,
    pub section: Option<String>,
    pub source_url: Option<String>,
    pub excerpt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewerApproval {
    pub status: ReviewStatus,
    pub reviewer_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub reviewed_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub organization_id: OrganizationId,
    pub source_type: SourceType,
    pub title: String,
    pub jurisdiction: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub effective_from: Option<OffsetDateTime>,
    pub content_sha256: String,
    pub confidence: SourceConfidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: ObligationId,
    pub organization_id: OrganizationId,
    pub source_id: SourceId,
    pub key: String,
    pub statement: String,
    pub locator: SourceLocator,
    #[serde(default)]
    pub applicability: Value,
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    pub review: Option<ReviewerApproval>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Advisory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingEvidencePolicy {
    NotObservable,
    Fail,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventMatcher {
    pub event_type: EventType,
    pub name: Option<String>,
    #[serde(default)]
    pub attribute_equals: BTreeMap<String, Value>,
    pub numeric_argument: Option<NumericArgumentMatcher>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumericArgumentMatcher {
    pub path: String,
    pub greater_than: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleAssertion {
    ExistsBefore { matcher: EventMatcher },
    Absent { matcher: EventMatcher },
    MaxCount { matcher: EventMatcher, count: u32 },
    TerminalState { state: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompiledRule {
    pub id: String,
    pub version: u32,
    pub obligation_key: String,
    pub severity: Severity,
    pub trigger: EventMatcher,
    pub assertions: Vec<RuleAssertion>,
    #[serde(default)]
    pub evidence_required: Vec<String>,
    pub on_missing_evidence: MissingEvidencePolicy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyPack {
    pub id: PolicyPackId,
    pub organization_id: OrganizationId,
    pub key: String,
    pub version: u32,
    pub title: String,
    pub status: ReviewStatus,
    pub content_sha256: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    pub rules: Vec<CompiledRule>,
}

/// Complete database persistence unit produced by the policy-ingestion lane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolicyBundle {
    pub pack: PolicyPack,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub obligations: Vec<Obligation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyPackApproval {
    pub reviewer_id: String,
    #[serde(default)]
    pub notes: String,
    #[serde(with = "time::serde::rfc3339")]
    pub approved_at: OffsetDateTime,
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    #[default]
    ScenarioInput,
    AgentStart,
    ModelCall,
    ModelResult,
    ToolCall,
    ToolResult,
    Retrieval,
    Handoff,
    GuardrailDecision,
    HumanApprovalRequest,
    HumanApprovalDecision,
    FinalOutput,
    SideEffect,
    Retry,
    Error,
    Timeout,
    Cancellation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Agent,
    Model,
    Tool,
    Human,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub actor_type: ActorType,
    pub id: String,
}

/// Untrusted, SDK-neutral observation returned by an active test target.
///
/// Featherlane assigns all tenancy, run, trace, and event identifiers while
/// normalizing these observations into immutable evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservedEvent {
    pub event_type: EventType,
    pub name: String,
    pub actor: Actor,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub schema_version: String,
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub invocation_id: InvocationId,
    pub scenario_id: ScenarioId,
    pub trace_id: String,
    pub id: EventId,
    pub parent_event_id: Option<EventId>,
    pub sequence: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub actor: Actor,
    pub event_type: EventType,
    pub name: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    pub source_span_id: Option<String>,
    pub redacted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceQualityStatus {
    Complete,
    Degraded,
    Insufficient,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TraceDefect {
    pub code: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub invocation_id: InvocationId,
    pub scenario_id: ScenarioId,
    pub target_version: String,
    pub terminal_state: Option<String>,
    pub events: Vec<NormalizedEvent>,
    #[serde(default)]
    pub side_effects: Vec<Value>,
    pub trace_quality: TraceQualityStatus,
    #[serde(default)]
    pub trace_defects: Vec<TraceDefect>,
    pub evidence_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Pass,
    Fail,
    Uncertain,
    NotObservable,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleResult {
    pub id: RuleResultId,
    pub rule_id: String,
    pub severity: Severity,
    pub status: RuleStatus,
    pub message: String,
    #[serde(default)]
    pub evidence_event_ids: Vec<EventId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunVerdict {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSummary {
    pub eval_run_id: EvalRunId,
    pub verdict: RunVerdict,
    pub results: Vec<RuleResult>,
    pub passed: usize,
    pub failed: usize,
    pub inconclusive: usize,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    #[error("evidence is incomplete: {0}")]
    EvidenceIncomplete(String),
    #[error("invalid lifecycle transition: {0}")]
    InvalidTransition(String),
    #[error("policy pack must be approved before publishing")]
    UnapprovedPolicyPack,
    #[error("invalid domain value: {0}")]
    InvalidValue(String),
}

impl PolicyPack {
    /// Confirms that a versioned pack can enter the executable policy registry.
    ///
    /// # Errors
    ///
    /// Returns an error when the pack is not approved or contains no rules.
    pub fn ensure_publishable(&self) -> Result<(), DomainError> {
        if self.status != ReviewStatus::Approved {
            return Err(DomainError::UnapprovedPolicyPack);
        }
        if self.rules.is_empty() {
            return Err(DomainError::InvalidValue(
                "policy pack must contain at least one rule".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_pack_cannot_publish() {
        let pack = PolicyPack {
            id: PolicyPackId::new(),
            organization_id: OrganizationId::new(),
            key: "test".to_owned(),
            version: 1,
            title: "Test".to_owned(),
            status: ReviewStatus::Draft,
            content_sha256: "abc".to_owned(),
            published_at: None,
            rules: vec![],
        };

        assert_eq!(
            pack.ensure_publishable(),
            Err(DomainError::UnapprovedPolicyPack)
        );
    }

    #[test]
    fn identifiers_round_trip_through_json() {
        let id = OrganizationId::new();
        let encoded = serde_json::to_string(&id).expect("identifier should serialize");
        let decoded: OrganizationId =
            serde_json::from_str(&encoded).expect("identifier should deserialize");
        assert_eq!(decoded, id);
    }
}
