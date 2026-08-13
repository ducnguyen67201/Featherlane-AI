//! Redaction, normalization, quality checks, and evidence finalization.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use governance_domain::{
    Actor, ActorType, EvalRunId, EventId, EventType, EvidenceBundle, InvocationId, NormalizedEvent,
    OrganizationId, ScenarioId, TraceDefect, TraceQualityStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
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
                "governance.event.type".to_owned(),
                "governance.input".to_owned(),
                "governance.output".to_owned(),
                "governance.actor.id".to_owned(),
                "governance.actor.type".to_owned(),
                "governance.run_id".to_owned(),
                "governance.invocation_id".to_owned(),
                "governance.scenario_id".to_owned(),
                "governance.terminal_state".to_owned(),
                "decision".to_owned(),
                "retry_attempt".to_owned(),
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

impl RedactionPolicy {
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
        _ => {}
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NormalizationContext {
    pub organization_id: OrganizationId,
    pub eval_run_id: EvalRunId,
    pub invocation_id: InvocationId,
    pub scenario_id: ScenarioId,
}

pub fn normalize_spans(
    context: NormalizationContext,
    mut spans: Vec<TraceSpan>,
    policy: &RedactionPolicy,
) -> Vec<NormalizedEvent> {
    spans.sort_by_key(|span| span.started_at);
    let span_to_event: BTreeMap<String, EventId> = spans
        .iter()
        .map(|span| (span.span_id.clone(), EventId::new()))
        .collect();

    spans
        .into_iter()
        .enumerate()
        .map(|(index, mut span)| {
            policy.redact_attributes(&mut span.attributes);
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
                .unwrap_or(Value::Null);
            let output = span
                .attributes
                .remove("governance.output")
                .unwrap_or(Value::Null);
            NormalizedEvent {
                schema_version: "1.0".to_owned(),
                organization_id: context.organization_id,
                eval_run_id: context.eval_run_id,
                invocation_id: context.invocation_id,
                scenario_id: context.scenario_id,
                trace_id: span.trace_id,
                id: span_to_event[&span.span_id],
                parent_event_id: span
                    .parent_span_id
                    .as_ref()
                    .and_then(|parent| span_to_event.get(parent).copied()),
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
            }
        })
        .collect()
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

    let blocking = defects.iter().any(|defect| defect.blocking);
    let status = if blocking {
        TraceQualityStatus::Insufficient
    } else if defects.is_empty() {
        TraceQualityStatus::Complete
    } else {
        TraceQualityStatus::Degraded
    };
    (status, defects)
}

pub fn finalize_evidence(
    context: NormalizationContext,
    target_version: String,
    terminal_state: Option<String>,
    events: Vec<NormalizedEvent>,
    side_effects: Vec<Value>,
) -> EvidenceBundle {
    let (trace_quality, trace_defects) = assess_trace_quality(&events);
    let canonical =
        serde_json::to_vec(&(target_version.as_str(), &events, &side_effects)).unwrap_or_default();
    let evidence_sha256 = format!("{:x}", Sha256::digest(canonical));
    EvidenceBundle {
        organization_id: context.organization_id,
        eval_run_id: context.eval_run_id,
        invocation_id: context.invocation_id,
        scenario_id: context.scenario_id,
        target_version,
        terminal_state,
        events,
        side_effects,
        trace_quality,
        trace_defects,
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
        .get("governance.event.type")
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
        _ if span.name.contains("final") || span.name.contains("terminal") => {
            EventType::FinalOutput
        }
        _ => EventType::AgentStart,
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
        _ => ActorType::System,
    }
}

#[allow(dead_code)]
fn redact_object(object: &mut Map<String, Value>, fragments: &[String]) {
    object.retain(|key, value| {
        if fragments
            .iter()
            .any(|fragment| key.to_ascii_lowercase().contains(fragment))
        {
            return false;
        }
        redact_nested(value, fragments);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_removes_secrets_and_unlisted_content() {
        let mut attributes = BTreeMap::from([
            (
                "authorization".to_owned(),
                Value::String("Bearer secret".to_owned()),
            ),
            ("decision".to_owned(), Value::String("approved".to_owned())),
            ("raw.prompt".to_owned(), Value::String("private".to_owned())),
        ]);
        let removed = RedactionPolicy::default().redact_attributes(&mut attributes);
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes["decision"], Value::String("approved".to_owned()));
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn missing_terminal_is_insufficient() {
        let context = NormalizationContext {
            organization_id: OrganizationId::new(),
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
        };
        let span = TraceSpan {
            trace_id: "trace".to_owned(),
            span_id: "root".to_owned(),
            parent_span_id: None,
            name: "agent".to_owned(),
            started_at: OffsetDateTime::now_utc(),
            ended_at: Some(OffsetDateTime::now_utc()),
            attributes: BTreeMap::new(),
        };
        let events = normalize_spans(context, vec![span], &RedactionPolicy::default());
        let (quality, defects) = assess_trace_quality(&events);
        assert_eq!(quality, TraceQualityStatus::Insufficient);
        assert!(
            defects
                .iter()
                .any(|defect| defect.code == "missing_terminal")
        );
    }
}
