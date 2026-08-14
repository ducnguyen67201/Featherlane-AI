//! Redaction, run correlation, deterministic normalization, and evidence finalization.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use governance_domain::{
    Actor, ActorType, CompletionReason, EvalRunId, EventId, EventType, EvidenceBundle,
    InvocationId, NormalizedEvent, ObservedEvent, OrganizationId, ScenarioId, TraceDefect,
    TraceQualityStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

pub const ATTR_EVAL_RUN_ID: &str = "featherlane.eval_run.id";
pub const ATTR_INVOCATION_ID: &str = "featherlane.invocation.id";
pub const ATTR_SCENARIO_ID: &str = "featherlane.scenario.id";
pub const ATTR_EXTERNAL_RUN_ID: &str = "featherlane.external_run.id";
pub const ATTR_EVENT_TYPE: &str = "featherlane.event.type";
pub const ATTR_RUN_TERMINAL: &str = "featherlane.run.terminal";
pub const ATTR_TERMINAL_STATE: &str = "featherlane.run.terminal_state";

const LEGACY_RUN_ID: &str = "governance.run_id";
const LEGACY_INVOCATION_ID: &str = "governance.invocation_id";
const LEGACY_SCENARIO_ID: &str = "governance.scenario_id";
const LEGACY_EVENT_TYPE: &str = "governance.event.type";
const LEGACY_TERMINAL_STATE: &str = "governance.terminal_state";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpanLink {
    pub trace_id: String,
    pub span_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    #[serde(default)]
    pub links: Vec<SpanLink>,
    pub name: String,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub resource_attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub instrumentation_scope: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

pub type ObservedSpan = TraceSpan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelemetryLimits {
    pub max_spans_per_request: usize,
    pub max_spans_per_run: usize,
    pub max_attributes_per_span: usize,
    pub max_string_bytes: usize,
}

impl Default for TelemetryLimits {
    fn default() -> Self {
        Self {
            max_spans_per_request: 10_000,
            max_spans_per_run: 100_000,
            max_attributes_per_span: 128,
            max_string_bytes: 16_384,
        }
    }
}

impl TelemetryLimits {
    /// Validates a bounded span before persistence.
    ///
    /// # Errors
    ///
    /// Returns a typed error when identifiers or attributes exceed safe limits.
    pub fn validate_span(&self, span: &TraceSpan) -> Result<(), TelemetryError> {
        if span.trace_id.len() != 32
            || !span.trace_id.bytes().all(|value| value.is_ascii_hexdigit())
            || span.trace_id.bytes().all(|value| value == b'0')
        {
            return Err(TelemetryError::InvalidTraceId);
        }
        if span.span_id.len() != 16
            || !span.span_id.bytes().all(|value| value.is_ascii_hexdigit())
            || span.span_id.bytes().all(|value| value == b'0')
            || span.parent_span_id.as_ref().is_some_and(|parent| {
                parent.len() != 16
                    || !parent.bytes().all(|value| value.is_ascii_hexdigit())
                    || parent.bytes().all(|value| value == b'0')
            })
            || span.links.iter().any(|link| {
                link.trace_id.len() != 32
                    || !link.trace_id.bytes().all(|value| value.is_ascii_hexdigit())
                    || link.trace_id.bytes().all(|value| value == b'0')
                    || link.span_id.len() != 16
                    || !link.span_id.bytes().all(|value| value.is_ascii_hexdigit())
                    || link.span_id.bytes().all(|value| value == b'0')
            })
        {
            return Err(TelemetryError::InvalidSpanId);
        }
        if span
            .attributes
            .len()
            .saturating_add(span.resource_attributes.len())
            > self.max_attributes_per_span
        {
            return Err(TelemetryError::LimitExceeded(
                "span attribute count exceeded".to_owned(),
            ));
        }
        if span.name.len() > self.max_string_bytes
            || span
                .attributes
                .iter()
                .chain(&span.resource_attributes)
                .any(|(key, value)| {
                    key.len() > self.max_string_bytes
                        || value_exceeds_limits(value, self.max_string_bytes, 0)
                })
        {
            return Err(TelemetryError::LimitExceeded(
                "span string value exceeded".to_owned(),
            ));
        }
        Ok(())
    }
}

fn value_exceeds_limits(value: &Value, string_limit: usize, depth: usize) -> bool {
    if depth > 8 {
        return true;
    }
    match value {
        Value::String(value) => value.len() > string_limit,
        Value::Array(values) => {
            values.len() > 128
                || values
                    .iter()
                    .any(|value| value_exceeds_limits(value, string_limit, depth + 1))
        }
        Value::Object(values) => {
            values.len() > 128
                || values.iter().any(|(key, value)| {
                    key.len() > string_limit || value_exceeds_limits(value, string_limit, depth + 1)
                })
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum TelemetryError {
    #[error("trace id must be 32 hexadecimal characters")]
    InvalidTraceId,
    #[error("span id must be 16 hexadecimal characters")]
    InvalidSpanId,
    #[error("invalid correlation identifier in {0}")]
    InvalidCorrelationId(String),
    #[error("telemetry limit exceeded: {0}")]
    LimitExceeded(String),
}

#[derive(Clone, Debug)]
pub struct RedactionPolicy {
    allowed_attributes: BTreeSet<String>,
    sensitive_key_fragments: Vec<String>,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            allowed_attributes: BTreeSet::from([
                "openinference.span.kind".to_owned(),
                "gen_ai.operation.name".to_owned(),
                "gen_ai.tool.name".to_owned(),
                "gen_ai.conversation.id".to_owned(),
                "gen_ai.agent.id".to_owned(),
                ATTR_EVAL_RUN_ID.to_owned(),
                ATTR_INVOCATION_ID.to_owned(),
                ATTR_SCENARIO_ID.to_owned(),
                ATTR_EXTERNAL_RUN_ID.to_owned(),
                ATTR_EVENT_TYPE.to_owned(),
                ATTR_RUN_TERMINAL.to_owned(),
                ATTR_TERMINAL_STATE.to_owned(),
                LEGACY_EVENT_TYPE.to_owned(),
                "governance.input".to_owned(),
                "governance.output".to_owned(),
                "governance.actor.id".to_owned(),
                "governance.actor.type".to_owned(),
                LEGACY_RUN_ID.to_owned(),
                LEGACY_INVOCATION_ID.to_owned(),
                LEGACY_SCENARIO_ID.to_owned(),
                LEGACY_TERMINAL_STATE.to_owned(),
                "terminal_state".to_owned(),
                "decision".to_owned(),
                "retry_attempt".to_owned(),
                "service.name".to_owned(),
            ]),
            sensitive_key_fragments: vec![
                "authorization".to_owned(),
                "cookie".to_owned(),
                "password".to_owned(),
                "secret".to_owned(),
                "token".to_owned(),
                "api_key".to_owned(),
            ],
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum NormalizationError {
    #[error("inline evidence contains more than 1000 observations")]
    TooManyObservations,
    #[error("inline evidence contains an empty event name")]
    EmptyEventName,
}

/// Converts untrusted wrapper observations into server-owned normalized events.
///
/// # Errors
///
/// Returns an error when the observation count exceeds the contract limit or
/// an observation has no name.
pub fn normalize_observations(
    context: NormalizationContext,
    trace_id: &str,
    target_id: &str,
    mut observations: Vec<ObservedEvent>,
    policy: &RedactionPolicy,
) -> Result<Vec<NormalizedEvent>, NormalizationError> {
    if observations.len() > 1_000 {
        return Err(NormalizationError::TooManyObservations);
    }
    if observations
        .iter()
        .any(|event| event.name.trim().is_empty())
    {
        return Err(NormalizationError::EmptyEventName);
    }
    let ids: Vec<EventId> = observations.iter().map(|_| EventId::new()).collect();
    let root_id = ids.first().copied();
    let now = OffsetDateTime::now_utc();
    Ok(observations
        .iter_mut()
        .enumerate()
        .map(|(index, observation)| {
            redact_nested(&mut observation.input, &policy.sensitive_key_fragments);
            redact_nested(&mut observation.output, &policy.sensitive_key_fragments);
            policy.redact_attributes(&mut observation.attributes);
            if observation.actor.id.trim().is_empty() {
                target_id.clone_into(&mut observation.actor.id);
            }
            NormalizedEvent {
                schema_version: "1.0".to_owned(),
                organization_id: context.organization_id,
                eval_run_id: context.eval_run_id,
                invocation_id: context.invocation_id,
                scenario_id: context.scenario_id,
                trace_id: trace_id.to_owned(),
                id: ids[index],
                parent_event_id: if index == 0 { None } else { root_id },
                linked_event_ids: Vec::new(),
                sequence: u64::try_from(index + 1).unwrap_or(u64::MAX),
                started_at: now,
                ended_at: Some(now),
                actor: observation.actor.clone(),
                event_type: observation.event_type,
                name: observation.name.clone(),
                input: observation.input.clone(),
                output: observation.output.clone(),
                attributes: observation.attributes.clone(),
                source_span_id: None,
                redacted: true,
            }
        })
        .collect())
}

impl RedactionPolicy {
    /// Removes secret-like keys recursively from an evidence value.
    pub fn redact_value(&self, value: &mut Value) {
        redact_nested(value, &self.sensitive_key_fragments);
    }

    #[must_use]
    pub fn with_allowed_attributes(mut self, attributes: impl IntoIterator<Item = String>) -> Self {
        self.allowed_attributes.extend(attributes);
        self
    }

    pub fn redact_attributes(&self, attributes: &mut BTreeMap<String, Value>) -> Vec<String> {
        let mut removed = Vec::new();
        attributes.retain(|key, value| {
            let normalized = key.to_ascii_lowercase();
            let sensitive = self
                .sensitive_key_fragments
                .iter()
                .any(|fragment| normalized.contains(fragment));
            let allowed = self.allowed_attributes.contains(key);
            if sensitive || !allowed {
                removed.push(key.clone());
                false
            } else {
                redact_nested(value, &self.sensitive_key_fragments);
                true
            }
        });
        removed
    }

    pub fn redact_span(&self, span: &mut TraceSpan) -> Vec<String> {
        let mut removed = self.redact_attributes(&mut span.resource_attributes);
        removed.extend(self.redact_attributes(&mut span.attributes));
        removed
    }
}

fn redact_nested(value: &mut Value, fragments: &[String]) {
    match value {
        Value::Object(object) => {
            object.retain(|key, child| {
                let normalized = key.to_ascii_lowercase();
                if fragments
                    .iter()
                    .any(|fragment| normalized.contains(fragment))
                {
                    return false;
                }
                redact_nested(child, fragments);
                true
            });
        }
        Value::Array(values) => {
            for child in values {
                redact_nested(child, fragments);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationCandidate {
    pub eval_run_id: Option<EvalRunId>,
    pub invocation_id: Option<InvocationId>,
    pub scenario_id: Option<ScenarioId>,
    pub external_run_id: Option<String>,
    pub terminal: bool,
    pub terminal_state: Option<String>,
}

/// Extracts Featherlane correlation values without trusting tenant identity.
///
/// # Errors
///
/// Returns an error when a present typed identifier is malformed.
pub fn extract_correlation(
    attributes: &BTreeMap<String, Value>,
) -> Result<CorrelationCandidate, TelemetryError> {
    Ok(CorrelationCandidate {
        eval_run_id: parse_typed_id(attributes, ATTR_EVAL_RUN_ID, LEGACY_RUN_ID, EvalRunId)?,
        invocation_id: parse_typed_id(
            attributes,
            ATTR_INVOCATION_ID,
            LEGACY_INVOCATION_ID,
            InvocationId,
        )?,
        scenario_id: parse_typed_id(attributes, ATTR_SCENARIO_ID, LEGACY_SCENARIO_ID, ScenarioId)?,
        external_run_id: string_attribute(attributes, ATTR_EXTERNAL_RUN_ID),
        terminal: attributes
            .get(ATTR_RUN_TERMINAL)
            .and_then(Value::as_bool)
            .unwrap_or(false),
        terminal_state: string_attribute(attributes, ATTR_TERMINAL_STATE)
            .or_else(|| string_attribute(attributes, LEGACY_TERMINAL_STATE)),
    })
}

/// Extracts correlation from resource defaults and span-local attributes.
/// Span-local values take precedence, matching OpenTelemetry attribute scoping.
///
/// # Errors
///
/// Returns an error when a present typed identifier is malformed.
pub fn extract_span_correlation(span: &TraceSpan) -> Result<CorrelationCandidate, TelemetryError> {
    let mut attributes = span.resource_attributes.clone();
    attributes.extend(span.attributes.clone());
    extract_correlation(&attributes)
}

fn parse_typed_id<T>(
    attributes: &BTreeMap<String, Value>,
    canonical: &str,
    legacy: &str,
    constructor: impl FnOnce(Uuid) -> T,
) -> Result<Option<T>, TelemetryError> {
    let Some((key, value)) = attributes
        .get(canonical)
        .and_then(Value::as_str)
        .map(|value| (canonical, value))
        .or_else(|| {
            attributes
                .get(legacy)
                .and_then(Value::as_str)
                .map(|value| (legacy, value))
        })
    else {
        return Ok(None);
    };
    Uuid::parse_str(value)
        .map(constructor)
        .map(Some)
        .map_err(|_| TelemetryError::InvalidCorrelationId(key.to_owned()))
}

fn string_attribute(attributes: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    attributes
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Clone, Copy, Debug)]
pub struct NormalizationContext {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub invocation_id: InvocationId,
    pub scenario_id: ScenarioId,
}

#[derive(Clone, Debug)]
pub struct FinalizationMetadata {
    pub target_version: String,
    pub policy_content_sha256: String,
    pub completion_reason: Option<CompletionReason>,
    pub terminal_state: Option<String>,
    pub finalized_at: OffsetDateTime,
}

fn normalize_spans_with_defects(
    context: NormalizationContext,
    mut spans: Vec<TraceSpan>,
    policy: &RedactionPolicy,
) -> (Vec<NormalizedEvent>, Vec<TraceDefect>) {
    for span in &mut spans {
        for (key, value) in span.resource_attributes.clone() {
            span.attributes.entry(key).or_insert(value);
        }
        policy.redact_span(span);
    }

    let (ordered_indices, mut defects) = causal_order(&spans);
    let span_to_event: HashMap<(String, String), EventId> = spans
        .iter()
        .map(|span| {
            (
                (span.trace_id.clone(), span.span_id.clone()),
                EventId::from_source_span(
                    context.organization_id,
                    context.eval_run_id,
                    &span.trace_id,
                    &span.span_id,
                ),
            )
        })
        .collect();

    let mut events = Vec::with_capacity(spans.len());
    for (index, span_index) in ordered_indices.into_iter().enumerate() {
        let mut span = spans[span_index].clone();
        let correlation = match extract_span_correlation(&span) {
            Ok(correlation) => correlation,
            Err(error) => {
                defects.push(defect("invalid_correlation", &error.to_string(), true));
                CorrelationCandidate {
                    eval_run_id: None,
                    invocation_id: None,
                    scenario_id: None,
                    external_run_id: None,
                    terminal: false,
                    terminal_state: None,
                }
            }
        };
        let event_type = infer_event_type(&span);
        let actor = Actor {
            actor_type: infer_actor_type(event_type),
            id: span
                .attributes
                .get("governance.actor.id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        };
        let input = span
            .attributes
            .remove("governance.input")
            .map_or(Value::Null, parse_structured_attribute);
        let output = span
            .attributes
            .remove("governance.output")
            .map_or(Value::Null, parse_structured_attribute);
        let parent_event_id = span.parent_span_id.as_ref().and_then(|parent| {
            span_to_event
                .get(&(span.trace_id.clone(), parent.clone()))
                .copied()
        });
        let linked_event_ids = span
            .links
            .iter()
            .filter_map(|link| {
                span_to_event
                    .get(&(link.trace_id.clone(), link.span_id.clone()))
                    .copied()
            })
            .collect();
        events.push(NormalizedEvent {
            schema_version: "1.1".to_owned(),
            organization_id: context.organization_id,
            eval_run_id: context.eval_run_id,
            invocation_id: correlation.invocation_id.unwrap_or(context.invocation_id),
            scenario_id: correlation.scenario_id.unwrap_or(context.scenario_id),
            trace_id: span.trace_id.clone(),
            id: span_to_event[&(span.trace_id.clone(), span.span_id.clone())],
            parent_event_id,
            linked_event_ids,
            sequence: u64::try_from(index + 1).unwrap_or(u64::MAX),
            started_at: span.started_at,
            ended_at: span.ended_at,
            actor,
            event_type,
            name: span
                .attributes
                .get("gen_ai.tool.name")
                .and_then(Value::as_str)
                .unwrap_or(&span.name)
                .to_owned(),
            input,
            output,
            attributes: span.attributes,
            source_span_id: Some(span.span_id),
            redacted: true,
        });
    }
    (events, defects)
}

fn parse_structured_attribute(value: Value) -> Value {
    if let Value::String(serialized) = &value {
        serde_json::from_str(serialized).unwrap_or(value)
    } else {
        value
    }
}

fn causal_order(spans: &[TraceSpan]) -> (Vec<usize>, Vec<TraceDefect>) {
    let positions: HashMap<(String, String), usize> = spans
        .iter()
        .enumerate()
        .map(|(index, span)| ((span.trace_id.clone(), span.span_id.clone()), index))
        .collect();
    let mut indegree = vec![0_usize; spans.len()];
    let mut outgoing = vec![HashSet::<usize>::new(); spans.len()];
    let mut defects = Vec::new();

    for (child_index, span) in spans.iter().enumerate() {
        if let Some(parent_span_id) = &span.parent_span_id {
            if let Some(parent_index) =
                positions.get(&(span.trace_id.clone(), parent_span_id.clone()))
            {
                add_edge(*parent_index, child_index, &mut indegree, &mut outgoing);
                if spans[*parent_index].started_at > span.started_at {
                    defects.push(defect(
                        "clock_skew",
                        &format!("span {} starts before its parent", span.span_id),
                        false,
                    ));
                }
            } else {
                defects.push(defect(
                    "orphan_parent",
                    &format!("span {} references an unknown parent", span.span_id),
                    false,
                ));
            }
        }
        for link in &span.links {
            if let Some(link_index) = positions.get(&(link.trace_id.clone(), link.span_id.clone()))
            {
                add_edge(*link_index, child_index, &mut indegree, &mut outgoing);
            } else {
                defects.push(defect(
                    "orphan_link",
                    &format!("span {} references an unknown link", span.span_id),
                    false,
                ));
            }
        }
    }

    let mut ready = BTreeSet::new();
    for (index, span) in spans.iter().enumerate() {
        if indegree[index] == 0 {
            ready.insert(sort_key(span, index));
        }
    }
    let mut ordered = Vec::with_capacity(spans.len());
    while let Some(next) = ready.pop_first() {
        let index = next.3;
        ordered.push(index);
        for child in &outgoing[index] {
            indegree[*child] = indegree[*child].saturating_sub(1);
            if indegree[*child] == 0 {
                ready.insert(sort_key(&spans[*child], *child));
            }
        }
    }

    if ordered.len() != spans.len() {
        defects.push(defect(
            "causal_cycle",
            "span parent/link relationships contain a cycle",
            true,
        ));
        let present: HashSet<usize> = ordered.iter().copied().collect();
        let mut remaining: Vec<usize> = (0..spans.len())
            .filter(|index| !present.contains(index))
            .collect();
        remaining.sort_by_key(|index| sort_key(&spans[*index], *index));
        ordered.extend(remaining);
    }
    (ordered, defects)
}

fn sort_key(span: &TraceSpan, index: usize) -> (OffsetDateTime, String, String, usize) {
    (
        span.started_at,
        span.trace_id.clone(),
        span.span_id.clone(),
        index,
    )
}

fn add_edge(parent: usize, child: usize, indegree: &mut [usize], outgoing: &mut [HashSet<usize>]) {
    if parent != child && outgoing[parent].insert(child) {
        indegree[child] = indegree[child].saturating_add(1);
    }
}

pub fn assess_trace_quality(events: &[NormalizedEvent]) -> (TraceQualityStatus, Vec<TraceDefect>) {
    let mut defects = Vec::new();
    if events.is_empty() {
        defects.push(defect("empty_trace", "No trace events were received", true));
    }
    if !events.iter().any(|event| event.parent_event_id.is_none()) {
        defects.push(defect("missing_root", "No root event was observed", true));
    }
    if !events
        .iter()
        .any(|event| event.event_type == EventType::FinalOutput)
    {
        defects.push(defect(
            "missing_terminal",
            "No final output or terminal workflow event was observed",
            true,
        ));
    }

    let ids: HashSet<EventId> = events.iter().map(|event| event.id).collect();
    for event in events {
        if event
            .parent_event_id
            .is_some_and(|parent| !ids.contains(&parent))
        {
            defects.push(defect(
                "orphan_parent",
                &format!("Event {} references an unknown parent", event.id),
                false,
            ));
        }
        if event
            .ended_at
            .is_some_and(|ended_at| ended_at < event.started_at)
        {
            defects.push(defect(
                "negative_duration",
                &format!("Event {} ends before it starts", event.id),
                false,
            ));
        }
    }
    (quality_from_defects(&defects), defects)
}

fn quality_from_defects(defects: &[TraceDefect]) -> TraceQualityStatus {
    if defects.iter().any(|defect| defect.blocking) {
        TraceQualityStatus::Insufficient
    } else if defects.is_empty() {
        TraceQualityStatus::Complete
    } else {
        TraceQualityStatus::Degraded
    }
}

/// Finalizes already-normalized inline target observations into an evidence bundle.
#[must_use]
pub fn finalize_evidence(
    context: NormalizationContext,
    target_version: String,
    terminal_state: Option<String>,
    events: Vec<NormalizedEvent>,
    side_effects: Vec<Value>,
) -> EvidenceBundle {
    let (trace_quality, trace_defects) = assess_trace_quality(&events);
    let mut trace_ids = events
        .iter()
        .map(|event| event.trace_id.clone())
        .collect::<Vec<_>>();
    trace_ids.sort();
    trace_ids.dedup();
    let mut invocation_ids = events
        .iter()
        .map(|event| event.invocation_id)
        .collect::<Vec<_>>();
    invocation_ids.push(context.invocation_id);
    invocation_ids.sort();
    invocation_ids.dedup();
    let completion_reason = terminal_state
        .as_ref()
        .map(|_| CompletionReason::TargetTerminalResponse);
    let finalized_at = OffsetDateTime::now_utc();
    let canonical = serde_json::to_vec(&(
        "1.1",
        target_version.as_str(),
        &trace_ids,
        &invocation_ids,
        completion_reason,
        terminal_state.as_deref(),
        &events,
        &side_effects,
        &trace_defects,
    ))
    .unwrap_or_default();
    let evidence_sha256 = format!("{:x}", Sha256::digest(canonical));
    EvidenceBundle {
        schema_version: "1.1".to_owned(),
        organization_id: context.organization_id,
        eval_run_id: context.eval_run_id,
        invocation_id: context.invocation_id,
        invocation_ids,
        scenario_id: context.scenario_id,
        target_version,
        policy_content_sha256: String::new(),
        trace_ids,
        completion_reason,
        terminal_state,
        events,
        side_effects,
        trace_quality,
        trace_defects,
        finalized_at: Some(finalized_at),
        evidence_sha256,
    }
}

pub fn finalize_observed_spans(
    context: NormalizationContext,
    metadata: FinalizationMetadata,
    spans: Vec<TraceSpan>,
    side_effects: Vec<Value>,
    policy: &RedactionPolicy,
) -> EvidenceBundle {
    let mut trace_ids: Vec<String> = spans.iter().map(|span| span.trace_id.clone()).collect();
    trace_ids.sort();
    trace_ids.dedup();
    let mut invocation_ids: Vec<InvocationId> = spans
        .iter()
        .filter_map(|span| extract_span_correlation(span).ok()?.invocation_id)
        .collect();
    invocation_ids.push(context.invocation_id);
    invocation_ids.sort();
    invocation_ids.dedup();

    let (events, mut trace_defects) = normalize_spans_with_defects(context, spans, policy);
    let (_, quality_defects) = assess_trace_quality(&events);
    trace_defects.extend(quality_defects);
    if matches!(
        metadata.completion_reason,
        Some(
            CompletionReason::Explicit
                | CompletionReason::TerminalEvent
                | CompletionReason::TargetTerminalResponse
        )
    ) {
        trace_defects.retain(|defect| defect.code != "missing_terminal");
    }
    if matches!(
        metadata.completion_reason,
        Some(CompletionReason::IdleTimeout | CompletionReason::MaxDuration)
    ) {
        trace_defects.push(defect(
            "forced_timeout",
            "The run was finalized by a bounded timeout",
            true,
        ));
    }
    trace_defects.sort_by(|left, right| {
        (&left.code, &left.message, left.blocking).cmp(&(
            &right.code,
            &right.message,
            right.blocking,
        ))
    });
    trace_defects.dedup();
    let trace_quality = quality_from_defects(&trace_defects);
    let canonical = serde_json::to_vec(&(
        "1.1",
        metadata.target_version.as_str(),
        metadata.policy_content_sha256.as_str(),
        &trace_ids,
        &invocation_ids,
        metadata.completion_reason,
        metadata.terminal_state.as_deref(),
        &events,
        &side_effects,
        &trace_defects,
    ))
    .unwrap_or_else(|error| {
        format!(
            "canonicalization-error:{}:{}:{}",
            context.organization_id, context.eval_run_id, error
        )
        .into_bytes()
    });
    let evidence_sha256 = format!("{:x}", Sha256::digest(canonical));
    EvidenceBundle {
        schema_version: "1.1".to_owned(),
        organization_id: context.organization_id,
        eval_run_id: context.eval_run_id,
        invocation_id: context.invocation_id,
        invocation_ids,
        scenario_id: context.scenario_id,
        target_version: metadata.target_version,
        policy_content_sha256: metadata.policy_content_sha256,
        trace_ids,
        completion_reason: metadata.completion_reason,
        terminal_state: metadata.terminal_state,
        events,
        side_effects,
        trace_quality,
        trace_defects,
        finalized_at: Some(metadata.finalized_at),
        evidence_sha256,
    }
}

fn defect(code: &str, message: &str, blocking: bool) -> TraceDefect {
    TraceDefect {
        code: code.to_owned(),
        message: message.to_owned(),
        blocking,
    }
}

fn infer_event_type(span: &TraceSpan) -> EventType {
    if let Some(explicit) = span
        .attributes
        .get(ATTR_EVENT_TYPE)
        .or_else(|| span.attributes.get(LEGACY_EVENT_TYPE))
        .and_then(Value::as_str)
        .and_then(parse_event_type)
    {
        return explicit;
    }
    match span
        .attributes
        .get("openinference.span.kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .as_str()
    {
        "TOOL" => EventType::ToolCall,
        "LLM" => EventType::ModelCall,
        "RETRIEVER" => EventType::Retrieval,
        "GUARDRAIL" => EventType::GuardrailDecision,
        "AGENT" | "CHAIN" => EventType::AgentStart,
        _ if span
            .attributes
            .get("gen_ai.operation.name")
            .and_then(Value::as_str)
            .is_some_and(|name| matches!(name, "invoke_agent" | "invoke_workflow")) =>
        {
            EventType::AgentStart
        }
        _ => EventType::Unclassified,
    }
}

fn parse_event_type(value: &str) -> Option<EventType> {
    serde_json::from_value(Value::String(value.to_owned())).ok()
}

fn infer_actor_type(event_type: EventType) -> ActorType {
    match event_type {
        EventType::ScenarioInput => ActorType::User,
        EventType::ModelCall | EventType::ModelResult => ActorType::Model,
        EventType::ToolCall | EventType::ToolResult | EventType::SideEffect => ActorType::Tool,
        EventType::HumanApprovalRequest | EventType::HumanApprovalDecision => ActorType::Human,
        EventType::AgentStart
        | EventType::FinalOutput
        | EventType::Handoff
        | EventType::Retrieval => ActorType::Agent,
        EventType::GuardrailDecision
        | EventType::Retry
        | EventType::Error
        | EventType::Timeout
        | EventType::Cancellation
        | EventType::Unclassified => ActorType::System,
    }
}

#[cfg(test)]
mod tests {
    use governance_domain::{CompletionReason, EventType};
    use serde_json::json;
    use time::{Duration, OffsetDateTime};

    use super::*;

    fn context() -> NormalizationContext {
        NormalizationContext {
            organization_id: OrganizationId::new(),
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        }
    }

    fn span(trace_id: &str, span_id: &str, offset_ms: i64, event_type: &str) -> TraceSpan {
        let started_at = OffsetDateTime::UNIX_EPOCH + Duration::milliseconds(offset_ms);
        TraceSpan {
            trace_id: trace_id.to_owned(),
            span_id: span_id.to_owned(),
            parent_span_id: None,
            links: Vec::new(),
            name: event_type.to_owned(),
            started_at,
            ended_at: Some(started_at + Duration::milliseconds(1)),
            attributes: BTreeMap::from([(ATTR_EVENT_TYPE.to_owned(), json!(event_type))]),
            resource_attributes: BTreeMap::new(),
            instrumentation_scope: None,
            status: None,
        }
    }

    #[test]
    fn span_validation_rejects_malformed_ids_and_limits() {
        let limits = TelemetryLimits {
            max_attributes_per_span: 1,
            max_string_bytes: 8,
            ..TelemetryLimits::default()
        };

        let mut short_trace = span("short", "0000000000000001", 0, "tool_call");
        assert_eq!(
            limits.validate_span(&short_trace),
            Err(TelemetryError::InvalidTraceId)
        );

        short_trace.trace_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
        short_trace.span_id = "0000000000000000".to_owned();
        assert_eq!(
            limits.validate_span(&short_trace),
            Err(TelemetryError::InvalidSpanId)
        );

        let mut malformed_link = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            0,
            "tool_call",
        );
        malformed_link.links.push(SpanLink {
            trace_id: "bad".to_owned(),
            span_id: "0000000000000002".to_owned(),
        });
        assert_eq!(
            limits.validate_span(&malformed_link),
            Err(TelemetryError::InvalidSpanId)
        );

        let mut too_many_attributes = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            0,
            "tool_call",
        );
        too_many_attributes
            .attributes
            .insert("decision".to_owned(), json!("ok"));
        assert!(matches!(
            limits.validate_span(&too_many_attributes),
            Err(TelemetryError::LimitExceeded(_))
        ));

        let mut long_string = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            0,
            "tool",
        );
        long_string
            .attributes
            .insert(ATTR_EVENT_TYPE.to_owned(), json!("too-long-value"));
        assert!(matches!(
            limits.validate_span(&long_string),
            Err(TelemetryError::LimitExceeded(_))
        ));
    }

    #[test]
    fn redaction_drops_unapproved_and_secret_attributes() {
        let policy = RedactionPolicy::default();
        let mut attributes = BTreeMap::from([
            ("decision".to_owned(), json!("approved")),
            ("authorization".to_owned(), json!("secret")),
            ("arbitrary".to_owned(), json!("value")),
        ]);
        let removed = policy.redact_attributes(&mut attributes);
        assert_eq!(
            attributes,
            BTreeMap::from([("decision".to_owned(), json!("approved"))])
        );
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn canonical_run_id_wins_over_legacy_value() {
        let canonical = EvalRunId::new();
        let legacy = EvalRunId::new();
        let attributes = BTreeMap::from([
            (ATTR_EVAL_RUN_ID.to_owned(), json!(canonical.to_string())),
            (LEGACY_RUN_ID.to_owned(), json!(legacy.to_string())),
        ]);
        assert_eq!(
            extract_correlation(&attributes)
                .expect("correlation should parse")
                .eval_run_id,
            Some(canonical)
        );
    }

    #[test]
    fn conversation_id_is_not_an_implicit_run_boundary() {
        let attributes = BTreeMap::from([(
            "gen_ai.conversation.id".to_owned(),
            json!("long-lived-conversation"),
        )]);

        assert_eq!(
            extract_correlation(&attributes)
                .expect("standard GenAI attributes should parse")
                .external_run_id,
            None
        );
    }

    #[test]
    fn span_correlation_overrides_resource_default() {
        let resource_run = EvalRunId::new();
        let span_run = EvalRunId::new();
        let mut observed = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            1,
            "tool_call",
        );
        observed
            .resource_attributes
            .insert(ATTR_EVAL_RUN_ID.to_owned(), json!(resource_run.to_string()));
        observed
            .attributes
            .insert(ATTR_EVAL_RUN_ID.to_owned(), json!(span_run.to_string()));

        assert_eq!(
            extract_span_correlation(&observed)
                .expect("correlation should parse")
                .eval_run_id,
            Some(span_run)
        );
    }

    #[test]
    fn cross_trace_link_orders_approval_before_tool() {
        let context = context();
        let mut tool = span(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "0000000000000002",
            1,
            "tool_call",
        );
        tool.links.push(SpanLink {
            trace_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            span_id: "0000000000000001".to_owned(),
        });
        let approval = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            20,
            "human_approval_decision",
        );
        let final_output = span(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "0000000000000003",
            30,
            "final_output",
        );
        let evidence = finalize_observed_spans(
            context,
            FinalizationMetadata {
                target_version: "git:test".to_owned(),
                policy_content_sha256: "sha".to_owned(),
                completion_reason: Some(CompletionReason::Explicit),
                terminal_state: Some("completed".to_owned()),
                finalized_at: OffsetDateTime::now_utc(),
            },
            vec![tool, final_output, approval],
            Vec::new(),
            &RedactionPolicy::default(),
        );
        assert_eq!(
            evidence.events[0].event_type,
            EventType::HumanApprovalDecision
        );
        assert_eq!(evidence.events[1].event_type, EventType::ToolCall);
        assert_eq!(evidence.trace_ids.len(), 2);
    }

    #[test]
    fn input_batch_order_does_not_change_evidence_hash() {
        let context = context();
        let first = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            1,
            "scenario_input",
        );
        let second = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000002",
            2,
            "final_output",
        );
        let metadata = FinalizationMetadata {
            target_version: "git:test".to_owned(),
            policy_content_sha256: "sha".to_owned(),
            completion_reason: Some(CompletionReason::Explicit),
            terminal_state: Some("completed".to_owned()),
            finalized_at: OffsetDateTime::UNIX_EPOCH,
        };
        let first_bundle = finalize_observed_spans(
            context,
            metadata.clone(),
            vec![first.clone(), second.clone()],
            Vec::new(),
            &RedactionPolicy::default(),
        );
        let second_bundle = finalize_observed_spans(
            context,
            metadata,
            vec![second, first],
            Vec::new(),
            &RedactionPolicy::default(),
        );
        assert_eq!(first_bundle.evidence_sha256, second_bundle.evidence_sha256);
    }

    #[test]
    fn forced_timeout_is_blocking_without_terminal_evidence() {
        let evidence = finalize_observed_spans(
            context(),
            FinalizationMetadata {
                target_version: "git:test".to_owned(),
                policy_content_sha256: "sha".to_owned(),
                completion_reason: Some(CompletionReason::MaxDuration),
                terminal_state: Some("timed_out".to_owned()),
                finalized_at: OffsetDateTime::UNIX_EPOCH,
            },
            vec![span(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "0000000000000001",
                1,
                "tool_call",
            )],
            Vec::new(),
            &RedactionPolicy::default(),
        );

        assert_eq!(evidence.trace_quality, TraceQualityStatus::Insufficient);
        assert!(
            evidence
                .trace_defects
                .iter()
                .any(|defect| defect.code == "forced_timeout" && defect.blocking)
        );
    }

    #[test]
    fn unknown_span_is_unclassified_instead_of_fabricated_agent_start() {
        let mut unknown = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            1,
            "unclassified",
        );
        unknown.attributes.clear();
        let (events, _) =
            normalize_spans_with_defects(context(), vec![unknown], &RedactionPolicy::default());
        assert_eq!(events[0].event_type, EventType::Unclassified);
    }

    #[test]
    fn causal_cycle_is_deterministic_and_blocking() {
        let mut first = span(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0000000000000001",
            1,
            "tool_call",
        );
        let mut second = span(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "0000000000000002",
            1,
            "final_output",
        );
        first.links.push(SpanLink {
            trace_id: second.trace_id.clone(),
            span_id: second.span_id.clone(),
        });
        second.links.push(SpanLink {
            trace_id: first.trace_id.clone(),
            span_id: first.span_id.clone(),
        });
        let bundle = finalize_observed_spans(
            context(),
            FinalizationMetadata {
                target_version: "git:test".to_owned(),
                policy_content_sha256: "sha".to_owned(),
                completion_reason: Some(CompletionReason::Explicit),
                terminal_state: Some("completed".to_owned()),
                finalized_at: OffsetDateTime::UNIX_EPOCH,
            },
            vec![second, first],
            Vec::new(),
            &RedactionPolicy::default(),
        );
        assert_eq!(bundle.trace_quality, TraceQualityStatus::Insufficient);
        assert!(
            bundle
                .trace_defects
                .iter()
                .any(|defect| defect.code == "causal_cycle")
        );
    }

    #[test]
    fn inline_normalization_owns_context_and_redacts_nested_secrets() {
        let context = NormalizationContext {
            organization_id: OrganizationId::new(),
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        };
        let observations = vec![ObservedEvent {
            event_type: EventType::FinalOutput,
            name: "done".to_owned(),
            actor: Actor {
                actor_type: ActorType::Agent,
                id: String::new(),
            },
            input: serde_json::json!({"nested": {"api_key": "remove", "safe": true}}),
            output: serde_json::json!({"token": "remove", "message": "ok"}),
            attributes: BTreeMap::from([
                ("terminal_state".to_owned(), serde_json::json!("completed")),
                ("raw.prompt".to_owned(), serde_json::json!("remove")),
            ]),
        }];
        let events = normalize_observations(
            context,
            "server-trace",
            "registered-target",
            observations,
            &RedactionPolicy::default(),
        )
        .expect("observations should normalize");
        assert_eq!(events[0].organization_id, context.organization_id);
        assert_eq!(events[0].eval_run_id, context.eval_run_id);
        assert_eq!(events[0].trace_id, "server-trace");
        assert_eq!(events[0].actor.id, "registered-target");
        assert_eq!(
            events[0].input,
            serde_json::json!({"nested": {"safe": true}})
        );
        assert_eq!(events[0].output, serde_json::json!({"message": "ok"}));
        assert_eq!(
            events[0].attributes,
            BTreeMap::from([("terminal_state".to_owned(), serde_json::json!("completed"))])
        );
    }

    #[test]
    fn side_effect_values_are_recursively_redacted() {
        let mut side_effect = serde_json::json!({
            "destination": "sandbox-ledger",
            "request": {
                "authorization": "Bearer remove",
                "payload": [{"api_key": "remove", "amount": 700}]
            }
        });

        RedactionPolicy::default().redact_value(&mut side_effect);

        assert_eq!(
            side_effect,
            serde_json::json!({
                "destination": "sandbox-ledger",
                "request": {"payload": [{"amount": 700}]}
            })
        );
    }
}
