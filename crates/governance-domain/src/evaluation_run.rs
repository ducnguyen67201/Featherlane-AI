use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    DomainError, EvalRunId, InvocationId, OrganizationId, PolicyPackId, RunVerdict, ScenarioId,
    TraceQualityStatus,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunBoundaryKind {
    WorkflowExecution,
    AgentTask,
    VoiceCall,
    ExplicitCi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationRunState {
    Created,
    Collecting,
    Settling,
    Finalizing,
    Evaluating,
    Completed,
    Cancelled,
    Failed,
}

impl EvaluationRunState {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    #[must_use]
    pub fn accepts_spans(self) -> bool {
        matches!(self, Self::Created | Self::Collecting | Self::Settling)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionReason {
    Explicit,
    TerminalEvent,
    TargetTerminalResponse,
    IdleTimeout,
    MaxDuration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationRun {
    pub id: EvalRunId,
    pub organization_id: OrganizationId,
    pub target_id: String,
    pub target_version: String,
    pub policy_pack_id: PolicyPackId,
    pub policy_pack_key: String,
    pub policy_pack_version: u32,
    pub policy_content_sha256: String,
    pub scenario_id: ScenarioId,
    #[serde(default)]
    pub rule_ids: Vec<String>,
    pub boundary_kind: RunBoundaryKind,
    pub external_run_id: Option<String>,
    pub primary_invocation_id: InvocationId,
    pub state: EvaluationRunState,
    pub completion_reason: Option<CompletionReason>,
    pub terminal_state: Option<String>,
    pub verdict: Option<RunVerdict>,
    pub trace_quality: Option<TraceQualityStatus>,
    pub evidence_sha256: Option<String>,
    pub span_count: u64,
    pub trace_count: u64,
    pub event_count: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_seen_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub settle_until: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub hard_deadline_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub finalized_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

impl EvaluationRun {
    /// Applies a valid lifecycle transition.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested transition skips a lifecycle boundary
    /// or attempts to reopen a terminal run.
    pub fn transition_to(
        &mut self,
        next: EvaluationRunState,
        now: OffsetDateTime,
    ) -> Result<(), DomainError> {
        if self.state == next {
            return Ok(());
        }
        let allowed = matches!(
            (self.state, next),
            (
                EvaluationRunState::Created,
                EvaluationRunState::Collecting | EvaluationRunState::Settling
            ) | (EvaluationRunState::Collecting, EvaluationRunState::Settling)
                | (EvaluationRunState::Settling, EvaluationRunState::Finalizing)
                | (
                    EvaluationRunState::Finalizing,
                    EvaluationRunState::Evaluating
                )
                | (
                    EvaluationRunState::Evaluating,
                    EvaluationRunState::Completed
                )
        ) || (!self.state.is_terminal()
            && matches!(
                next,
                EvaluationRunState::Cancelled | EvaluationRunState::Failed
            ));

        if !allowed {
            return Err(DomainError::InvalidTransition(format!(
                "{:?} -> {:?}",
                self.state, next
            )));
        }
        self.state = next;
        self.updated_at = now;
        if next == EvaluationRunState::Completed {
            self.completed_at = Some(now);
        }
        Ok(())
    }

    /// Records an idempotent completion boundary and enters the settle window.
    ///
    /// # Errors
    ///
    /// Returns an error if a previous completion signal used a different terminal
    /// state or completion reason.
    pub fn begin_settling(
        &mut self,
        reason: CompletionReason,
        terminal_state: Option<String>,
        settle_until: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<(), DomainError> {
        if let Some(previous) = self.completion_reason {
            if previous != reason || self.terminal_state != terminal_state {
                return Err(DomainError::InvalidTransition(
                    "conflicting completion signal".to_owned(),
                ));
            }
            if self.state == EvaluationRunState::Settling {
                self.settle_until = Some(settle_until.min(self.hard_deadline_at));
                self.updated_at = now;
                return Ok(());
            }
        }
        self.transition_to(EvaluationRunState::Settling, now)?;
        self.completion_reason = Some(reason);
        self.terminal_state = terminal_state;
        self.settle_until = Some(settle_until.min(self.hard_deadline_at));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime};

    use super::*;

    fn run() -> EvaluationRun {
        let now = OffsetDateTime::now_utc();
        EvaluationRun {
            id: EvalRunId::new(),
            organization_id: OrganizationId::new(),
            target_id: "target".to_owned(),
            target_version: "git:test".to_owned(),
            policy_pack_id: PolicyPackId::new(),
            policy_pack_key: "pack".to_owned(),
            policy_pack_version: 1,
            policy_content_sha256: "sha".to_owned(),
            scenario_id: ScenarioId::new(),
            rule_ids: Vec::new(),
            boundary_kind: RunBoundaryKind::ExplicitCi,
            external_run_id: None,
            primary_invocation_id: InvocationId::new(),
            state: EvaluationRunState::Created,
            completion_reason: None,
            terminal_state: None,
            verdict: None,
            trace_quality: None,
            evidence_sha256: None,
            span_count: 0,
            trace_count: 0,
            event_count: 0,
            created_at: now,
            updated_at: now,
            last_seen_at: None,
            settle_until: None,
            hard_deadline_at: now + Duration::minutes(5),
            finalized_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn lifecycle_rejects_skipped_finalization() {
        let mut run = run();
        let error = run
            .transition_to(EvaluationRunState::Completed, OffsetDateTime::now_utc())
            .expect_err("created run cannot complete directly");
        assert!(matches!(error, DomainError::InvalidTransition(_)));
    }

    #[test]
    fn repeated_completion_is_idempotent() {
        let mut run = run();
        let now = OffsetDateTime::now_utc();
        let settle_until = now + Duration::seconds(2);
        run.begin_settling(
            CompletionReason::Explicit,
            Some("completed".to_owned()),
            settle_until,
            now,
        )
        .expect("first completion should settle");
        run.begin_settling(
            CompletionReason::Explicit,
            Some("completed".to_owned()),
            settle_until,
            now,
        )
        .expect("same completion should be idempotent");
        assert_eq!(run.state, EvaluationRunState::Settling);
    }

    #[test]
    fn conflicting_completion_is_rejected() {
        let mut run = run();
        let now = OffsetDateTime::now_utc();
        run.begin_settling(
            CompletionReason::Explicit,
            Some("completed".to_owned()),
            now,
            now,
        )
        .expect("first completion should settle");
        let error = run
            .begin_settling(
                CompletionReason::TerminalEvent,
                Some("failed".to_owned()),
                now,
                now,
            )
            .expect_err("conflicting completion must fail");
        assert!(matches!(error, DomainError::InvalidTransition(_)));
    }

    #[test]
    fn legacy_v1_evidence_fixture_deserializes_with_defaults() {
        let bundle: crate::EvidenceBundle =
            serde_json::from_str(include_str!("../../../fixtures/traces/refund-pass.json"))
                .expect("v1.0 evidence remains readable");
        assert_eq!(bundle.schema_version, "1.0");
        assert!(bundle.invocation_ids.is_empty());
        assert!(
            bundle
                .events
                .iter()
                .all(|event| event.linked_event_ids.is_empty())
        );
    }

    #[test]
    fn event_ids_are_stable_per_source_span() {
        let organization_id = OrganizationId::new();
        let eval_run_id = EvalRunId::new();
        let first = crate::EventId::from_source_span(organization_id, eval_run_id, "trace", "span");
        let second =
            crate::EventId::from_source_span(organization_id, eval_run_id, "trace", "span");
        assert_eq!(first, second);
        assert_ne!(
            first,
            crate::EventId::from_source_span(organization_id, EvalRunId::new(), "trace", "span")
        );
    }
}
