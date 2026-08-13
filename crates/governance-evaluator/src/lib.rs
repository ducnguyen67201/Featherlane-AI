//! Deterministic, evidence-first evaluation.

use governance_domain::{
    CompiledRule, EvaluationSummary, EventId, EventMatcher, EvidenceBundle, MissingEvidencePolicy,
    NormalizedEvent, RuleAssertion, RuleResult, RuleResultId, RuleStatus, RunVerdict, Severity,
    TraceQualityStatus,
};
use serde_json::Value;

pub fn evaluate_rule(rule: &CompiledRule, evidence: &EvidenceBundle) -> RuleResult {
    if evidence.trace_quality == TraceQualityStatus::Insufficient {
        return missing_evidence_result(rule, "trace quality is insufficient for this rule");
    }

    let missing: Vec<&str> = rule
        .evidence_required
        .iter()
        .map(String::as_str)
        .filter(|requirement| !has_required_evidence(requirement, evidence))
        .collect();
    if !missing.is_empty() {
        return missing_evidence_result(
            rule,
            &format!("required evidence was not observed: {}", missing.join(", ")),
        );
    }

    let triggers: Vec<&NormalizedEvent> = evidence
        .events
        .iter()
        .filter(|event| event_matches(event, &rule.trigger))
        .collect();

    if triggers.is_empty() {
        return result(
            rule,
            RuleStatus::Pass,
            "rule was not triggered by this invocation",
            vec![],
        );
    }

    let mut cited = Vec::new();
    for trigger in triggers {
        cited.push(trigger.id);
        for assertion in &rule.assertions {
            match evaluate_assertion(assertion, trigger, evidence) {
                AssertionOutcome::Pass(event_ids) => cited.extend(event_ids),
                AssertionOutcome::Fail(message, event_ids) => {
                    cited.extend(event_ids);
                    cited.sort_unstable();
                    cited.dedup();
                    return result(rule, RuleStatus::Fail, &message, cited);
                }
                AssertionOutcome::NotObservable(message) => {
                    return missing_evidence_result(rule, &message);
                }
            }
        }
    }

    cited.sort_unstable();
    cited.dedup();
    result(
        rule,
        RuleStatus::Pass,
        "all deterministic assertions passed",
        cited,
    )
}

pub fn evaluate_pack(
    eval_run_id: governance_domain::EvalRunId,
    rules: &[CompiledRule],
    evidence: &EvidenceBundle,
) -> EvaluationSummary {
    let results: Vec<RuleResult> = rules
        .iter()
        .map(|rule| evaluate_rule(rule, evidence))
        .collect();
    let passed = results
        .iter()
        .filter(|result| result.status == RuleStatus::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == RuleStatus::Fail)
        .count();
    let inconclusive = results.len().saturating_sub(passed + failed);

    let blocking_failure = results.iter().any(|result| {
        result.status == RuleStatus::Fail
            && matches!(result.severity, Severity::Critical | Severity::High)
    });
    let verdict = if blocking_failure {
        RunVerdict::Fail
    } else if inconclusive > 0 {
        RunVerdict::Inconclusive
    } else {
        RunVerdict::Pass
    };

    EvaluationSummary {
        eval_run_id,
        verdict,
        results,
        passed,
        failed,
        inconclusive,
    }
}

enum AssertionOutcome {
    Pass(Vec<EventId>),
    Fail(String, Vec<EventId>),
    NotObservable(String),
}

fn evaluate_assertion(
    assertion: &RuleAssertion,
    trigger: &NormalizedEvent,
    evidence: &EvidenceBundle,
) -> AssertionOutcome {
    let events = &evidence.events;
    match assertion {
        RuleAssertion::ExistsBefore { matcher } => {
            let matching = events
                .iter()
                .filter(|event| event.sequence < trigger.sequence && event_matches(event, matcher))
                .max_by_key(|event| event.sequence);
            matching.map_or_else(
                || {
                    AssertionOutcome::Fail(
                        format!(
                            "required {} event was not observed before {}",
                            display_event_type(matcher),
                            trigger.name
                        ),
                        vec![],
                    )
                },
                |event| AssertionOutcome::Pass(vec![event.id]),
            )
        }
        RuleAssertion::Absent { matcher } => {
            let matching: Vec<EventId> = events
                .iter()
                .filter(|event| event_matches(event, matcher))
                .map(|event| event.id)
                .collect();
            if matching.is_empty() {
                AssertionOutcome::Pass(vec![])
            } else {
                AssertionOutcome::Fail(
                    format!(
                        "prohibited {} event was observed",
                        display_event_type(matcher)
                    ),
                    matching,
                )
            }
        }
        RuleAssertion::MaxCount { matcher, count } => {
            let matching: Vec<EventId> = events
                .iter()
                .filter(|event| event_matches(event, matcher))
                .map(|event| event.id)
                .collect();
            if matching.len() <= *count as usize {
                AssertionOutcome::Pass(matching)
            } else {
                AssertionOutcome::Fail(
                    format!("event count {} exceeded maximum {count}", matching.len()),
                    matching,
                )
            }
        }
        RuleAssertion::TerminalState { state } => match &trigger.ended_at {
            Some(_) if state_matches(state, evidence) => AssertionOutcome::Pass(vec![trigger.id]),
            Some(_) => AssertionOutcome::Fail(
                format!("required terminal state {state} was not observed"),
                vec![],
            ),
            None => AssertionOutcome::NotObservable(
                "terminal state cannot be established without a terminal event".to_owned(),
            ),
        },
    }
}

fn state_matches(state: &str, evidence: &EvidenceBundle) -> bool {
    evidence.terminal_state.as_deref() == Some(state)
        || evidence.events.iter().any(|event| {
            event
                .attributes
                .get("terminal_state")
                .or_else(|| event.attributes.get("featherlane.run.terminal_state"))
                .or_else(|| event.attributes.get("governance.terminal_state"))
                .and_then(Value::as_str)
                == Some(state)
        })
}

fn event_matches(event: &NormalizedEvent, matcher: &EventMatcher) -> bool {
    if event.event_type != matcher.event_type {
        return false;
    }
    if matcher
        .name
        .as_ref()
        .is_some_and(|name| event.name != *name)
    {
        return false;
    }
    if !matcher
        .attribute_equals
        .iter()
        .all(|(key, expected)| event.attributes.get(key) == Some(expected))
    {
        return false;
    }
    if let Some(numeric) = &matcher.numeric_argument {
        let Some(value) = value_at_path(&event.input, &numeric.path).and_then(Value::as_f64) else {
            return false;
        };
        if value <= numeric.greater_than {
            return false;
        }
    }
    true
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, part| current.get(part))
}

fn has_required_evidence(requirement: &str, evidence: &EvidenceBundle) -> bool {
    let requirement = requirement.trim();
    if requirement == "terminal_state" {
        return evidence.terminal_state.is_some();
    }

    if let Some(attribute) = requirement.strip_prefix("attribute:") {
        let (key, expected) = attribute
            .split_once('=')
            .map_or((attribute, None), |(key, value)| (key, Some(value)));
        return evidence.events.iter().any(|event| {
            event.attributes.get(key).is_some_and(|value| {
                expected.is_none_or(|expected| {
                    value.as_str() == Some(expected)
                        || serde_json::from_str::<Value>(expected)
                            .is_ok_and(|parsed| parsed == *value)
                })
            })
        });
    }

    if let Some((event_type, path)) = requirement.split_once('.')
        && let Ok(event_type) = serde_json::from_value::<governance_domain::EventType>(
            Value::String(event_type.to_owned()),
        )
    {
        let supported_path = path.starts_with("input.")
            || path.starts_with("output.")
            || path.starts_with("attributes.");
        if supported_path {
            return evidence
                .events
                .iter()
                .filter(|event| event.event_type == event_type)
                .any(|event| event_has_path(event, path));
        }
    }

    evidence.events.iter().any(|event| {
        serde_json::to_value(event.event_type)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .as_deref()
            == Some(requirement)
            || event.attributes.contains_key(requirement)
    })
}

fn event_has_path(event: &NormalizedEvent, path: &str) -> bool {
    if let Some(path) = path.strip_prefix("input.") {
        return value_at_path(&event.input, path).is_some();
    }
    if let Some(path) = path.strip_prefix("output.") {
        return value_at_path(&event.output, path).is_some();
    }
    if let Some(path) = path.strip_prefix("attributes.") {
        return event.attributes.contains_key(path)
            || value_at_path(
                &Value::Object(event.attributes.clone().into_iter().collect()),
                path,
            )
            .is_some();
    }
    false
}

fn display_event_type(matcher: &EventMatcher) -> String {
    serde_json::to_value(matcher.event_type)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn missing_evidence_result(rule: &CompiledRule, message: &str) -> RuleResult {
    let status = match rule.on_missing_evidence {
        MissingEvidencePolicy::NotObservable => RuleStatus::NotObservable,
        MissingEvidencePolicy::Fail => RuleStatus::Fail,
        MissingEvidencePolicy::Error => RuleStatus::Error,
    };
    result(rule, status, message, vec![])
}

fn result(
    rule: &CompiledRule,
    status: RuleStatus,
    message: &str,
    evidence_event_ids: Vec<EventId>,
) -> RuleResult {
    RuleResult {
        id: RuleResultId::new(),
        rule_id: rule.id.clone(),
        severity: rule.severity,
        status,
        message: message.to_owned(),
        evidence_event_ids,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use governance_domain::{
        Actor, ActorType, EvalRunId, EventId, EventType, EvidenceBundle, InvocationId,
        MissingEvidencePolicy, NumericArgumentMatcher, OrganizationId, ScenarioId, Severity,
        TraceQualityStatus,
    };
    use serde_json::json;
    use time::OffsetDateTime;

    use super::*;

    fn rule() -> CompiledRule {
        CompiledRule {
            id: "refund_requires_approval".to_owned(),
            version: 1,
            obligation_key: "REFUND-004".to_owned(),
            severity: Severity::Critical,
            trigger: EventMatcher {
                event_type: EventType::ToolCall,
                name: Some("issue_refund".to_owned()),
                attribute_equals: BTreeMap::new(),
                numeric_argument: Some(NumericArgumentMatcher {
                    path: "amount".to_owned(),
                    greater_than: 500.0,
                }),
            },
            assertions: vec![RuleAssertion::ExistsBefore {
                matcher: EventMatcher {
                    event_type: EventType::HumanApprovalDecision,
                    name: Some("approval".to_owned()),
                    attribute_equals: BTreeMap::from([("decision".to_owned(), json!("approved"))]),
                    numeric_argument: None,
                },
            }],
            evidence_required: vec![],
            on_missing_evidence: MissingEvidencePolicy::NotObservable,
        }
    }

    fn event(sequence: u64, event_type: EventType, name: &str) -> NormalizedEvent {
        NormalizedEvent {
            schema_version: "1.0".to_owned(),
            organization_id: OrganizationId::new(),
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            scenario_id: ScenarioId::new(),
            trace_id: "trace".to_owned(),
            id: EventId::new(),
            parent_event_id: None,
            linked_event_ids: vec![],
            sequence,
            started_at: OffsetDateTime::now_utc(),
            ended_at: Some(OffsetDateTime::now_utc()),
            actor: Actor {
                actor_type: ActorType::Agent,
                id: "refund-agent".to_owned(),
            },
            event_type,
            name: name.to_owned(),
            input: json!({}),
            output: json!(null),
            attributes: BTreeMap::new(),
            source_span_id: None,
            redacted: true,
        }
    }

    fn evidence(events: Vec<NormalizedEvent>) -> EvidenceBundle {
        EvidenceBundle {
            schema_version: "1.1".to_owned(),
            organization_id: OrganizationId::new(),
            eval_run_id: EvalRunId::new(),
            invocation_id: InvocationId::new(),
            invocation_ids: vec![],
            scenario_id: ScenarioId::new(),
            target_version: "git:test".to_owned(),
            policy_content_sha256: "policy-test".to_owned(),
            trace_ids: vec!["trace".to_owned()],
            completion_reason: None,
            terminal_state: Some("completed".to_owned()),
            events,
            side_effects: vec![],
            trace_quality: TraceQualityStatus::Complete,
            trace_defects: vec![],
            finalized_at: Some(OffsetDateTime::now_utc()),
            evidence_sha256: "test".to_owned(),
        }
    }

    #[test]
    fn approval_before_refund_passes() {
        let mut approval = event(1, EventType::HumanApprovalDecision, "approval");
        approval
            .attributes
            .insert("decision".to_owned(), json!("approved"));
        let mut refund = event(2, EventType::ToolCall, "issue_refund");
        refund.input = json!({"amount": 700.0});

        let result = evaluate_rule(&rule(), &evidence(vec![approval, refund]));
        assert_eq!(result.status, RuleStatus::Pass);
    }

    #[test]
    fn refund_before_approval_fails() {
        let mut refund = event(1, EventType::ToolCall, "issue_refund");
        refund.input = json!({"amount": 700.0});
        let mut approval = event(2, EventType::HumanApprovalDecision, "approval");
        approval
            .attributes
            .insert("decision".to_owned(), json!("approved"));

        let result = evaluate_rule(&rule(), &evidence(vec![refund, approval]));
        assert_eq!(result.status, RuleStatus::Fail);
    }

    #[test]
    fn insufficient_trace_never_passes() {
        let mut bundle = evidence(vec![]);
        bundle.trace_quality = TraceQualityStatus::Insufficient;
        let result = evaluate_rule(&rule(), &bundle);
        assert_eq!(result.status, RuleStatus::NotObservable);
    }

    #[test]
    fn missing_required_approval_is_not_a_vacuous_pass() {
        let mut required_rule = rule();
        required_rule.evidence_required = vec!["human_approval_decision".to_owned()];

        let result = evaluate_rule(&required_rule, &evidence(vec![]));

        assert_eq!(result.status, RuleStatus::NotObservable);
        assert!(result.message.contains("human_approval_decision"));
    }

    #[test]
    fn missing_required_evidence_honors_fail_policy() {
        let mut required_rule = rule();
        required_rule.evidence_required = vec!["human_approval_decision".to_owned()];
        required_rule.on_missing_evidence = MissingEvidencePolicy::Fail;

        let result = evaluate_rule(&required_rule, &evidence(vec![]));

        assert_eq!(result.status, RuleStatus::Fail);
    }

    #[test]
    fn approval_from_another_trace_satisfies_required_evidence() {
        let mut required_rule = rule();
        required_rule.evidence_required = vec!["human_approval_decision".to_owned()];
        let mut approval = event(1, EventType::HumanApprovalDecision, "approval");
        approval.trace_id = "approval-trace".to_owned();
        approval
            .attributes
            .insert("decision".to_owned(), json!("approved"));
        let mut refund = event(2, EventType::ToolCall, "issue_refund");
        refund.trace_id = "execution-trace".to_owned();
        refund.input = json!({"amount": 700.0});

        let result = evaluate_rule(&required_rule, &evidence(vec![approval, refund]));

        assert_eq!(result.status, RuleStatus::Pass);
    }

    #[test]
    fn dotted_required_evidence_paths_resolve_against_typed_events() {
        let mut required_rule = rule();
        required_rule.evidence_required = vec![
            "tool_call.input.amount".to_owned(),
            "human_approval_decision.attributes.decision".to_owned(),
        ];
        let mut approval = event(1, EventType::HumanApprovalDecision, "approval");
        approval
            .attributes
            .insert("decision".to_owned(), json!("approved"));
        let mut refund = event(2, EventType::ToolCall, "issue_refund");
        refund.input = json!({"amount": 700.0});

        let result = evaluate_rule(&required_rule, &evidence(vec![approval, refund]));

        assert_eq!(result.status, RuleStatus::Pass);
    }

    #[test]
    fn attribute_requirements_support_presence_and_typed_values() {
        let mut observed = event(1, EventType::HumanApprovalDecision, "approval");
        observed
            .attributes
            .insert("decision".to_owned(), json!("approved"));
        observed
            .attributes
            .insert("retry_attempt".to_owned(), json!(2));

        for requirement in [
            "attribute:decision",
            "attribute:decision=approved",
            "attribute:retry_attempt=2",
        ] {
            let mut required_rule = rule();
            required_rule.evidence_required = vec![requirement.to_owned()];
            assert_eq!(
                evaluate_rule(&required_rule, &evidence(vec![observed.clone()])).status,
                RuleStatus::Pass,
                "{requirement} should match"
            );
        }
    }
}
