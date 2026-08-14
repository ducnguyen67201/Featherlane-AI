import { describe, expect, it } from "vitest";
import {
  citedEventIds,
  eventFact,
  evidenceCitations,
  runDuration,
  traceWaterfall,
  verdictSummary,
} from "./evaluation-view";
import { displayVerdict, type EvaluationRun, type EvidenceEvent } from "./types";

const approval: EvidenceEvent = {
  id: "approval-event",
  trace_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  source_span_id: "aaaaaaaaaaaaaaaa",
  parent_event_id: null,
  linked_event_ids: [],
  sequence: 1,
  started_at: "2026-08-13T05:00:00Z",
  ended_at: "2026-08-13T05:00:00.01Z",
  event_type: "human_approval_decision",
  name: "approval",
  actor: { actor_type: "human", id: "reviewer" },
  input: null,
  output: null,
  attributes: { decision: "approved", "service.name": "approval-service" },
  redacted: true,
};

describe("evaluation result presentation", () => {
  it("accepts both API and legacy verdict casing", () => {
    expect(displayVerdict("PASS")).toBe("PASS");
    expect(displayVerdict("inconclusive")).toBe("INCONCLUSIVE");
  });

  it("maps rule citations to the exact normalized evidence event", () => {
    const result = {
      rule_id: "pol_001_v1",
      severity: "critical",
      status: "pass",
      message: "all deterministic assertions passed",
      evidence_event_ids: [approval.id, "missing-event"],
    };
    expect(evidenceCitations(result, [approval])).toEqual([approval]);
    expect(citedEventIds({
      verdict: "PASS",
      results: [result],
      passed: 1,
      failed: 0,
      inconclusive: 0,
    })).toEqual(new Set([approval.id, "missing-event"]));
  });

  it("builds useful human-readable outcome and event facts", () => {
    expect(verdictSummary("PASS", {
      verdict: "PASS",
      results: [],
      passed: 3,
      failed: 0,
      inconclusive: 0,
    }, "complete")).toBe("3 of 3 deterministic controls passed against complete trace evidence.");
    expect(eventFact(approval)).toBe("Decision: approved");
  });

  it("derives wall-clock run duration from persisted timestamps", () => {
    const run = {
      created_at: "2026-08-13T13:57:56.828Z",
      updated_at: "2026-08-13T13:58:07.965Z",
      completed_at: "2026-08-13T13:58:07.965Z",
    } as EvaluationRun;
    expect(runDuration(run)).toBe("11.1 s");
  });

  it("scales normalized events into a chronological trace waterfall", () => {
    const refund: EvidenceEvent = {
      ...approval,
      id: "refund-event",
      trace_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      sequence: 2,
      started_at: "2026-08-13T05:00:00.020Z",
      ended_at: "2026-08-13T05:00:00.030Z",
      event_type: "tool_call",
      name: "issue_refund",
    };

    const waterfall = traceWaterfall([refund, approval]);
    expect(waterfall.durationMs).toBe(30);
    expect(waterfall.segments.map((segment) => ({
      id: segment.event.id,
      offsetMs: segment.offsetMs,
      durationMs: segment.durationMs,
      startPercent: segment.startPercent,
      widthPercent: segment.widthPercent,
    }))).toEqual([
      { id: "approval-event", offsetMs: 0, durationMs: 10, startPercent: 0, widthPercent: 33.3333 },
      { id: "refund-event", offsetMs: 20, durationMs: 10, startPercent: 66.6667, widthPercent: 33.3333 },
    ]);
  });

  it("omits events without a usable start time from the waterfall", () => {
    const invalid = { ...approval, id: "invalid", started_at: "unknown" };
    expect(traceWaterfall([invalid])).toEqual({ durationMs: 0, segments: [] });
  });
});
