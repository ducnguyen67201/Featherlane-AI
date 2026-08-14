import { formatDuration } from "./format";
import type {
  EvaluationRun,
  EvaluationSummary,
  EvidenceEvent,
  RuleResult,
  Verdict,
} from "./types";

export function words(value: string): string {
  return value.replaceAll("_", " ").replaceAll("-", " ");
}

export function shortId(value: string | null | undefined, size = 8): string {
  if (!value) return "—";
  return value.length <= size ? value : value.slice(0, size);
}

export function runDuration(run: EvaluationRun): string {
  const started = Date.parse(run.created_at);
  const ended = Date.parse(run.completed_at ?? run.updated_at);
  if (!Number.isFinite(started) || !Number.isFinite(ended)) return "—";
  return formatDuration(Math.max(0, ended - started));
}

export function traceWaterfall(events: EvidenceEvent[]) {
  const timedEvents = events.flatMap((event) => {
    const startedAt = Date.parse(event.started_at);
    if (!Number.isFinite(startedAt)) return [];
    const parsedEnd = event.ended_at ? Date.parse(event.ended_at) : startedAt;
    const endedAt = Number.isFinite(parsedEnd) ? Math.max(startedAt, parsedEnd) : startedAt;
    return [{ event, startedAt, endedAt }];
  });

  if (timedEvents.length === 0) return { durationMs: 0, segments: [] };

  const origin = Math.min(...timedEvents.map(({ startedAt }) => startedAt));
  const finish = Math.max(...timedEvents.map(({ endedAt }) => endedAt));
  const durationMs = Math.max(0, finish - origin);
  const scaleMs = Math.max(1, durationMs);
  const toPercent = (value: number) => Math.round((value / scaleMs) * 1_000_000) / 10_000;

  return {
    durationMs,
    segments: timedEvents
      .sort((a, b) => a.event.sequence - b.event.sequence)
      .map(({ event, startedAt, endedAt }) => ({
        event,
        offsetMs: startedAt - origin,
        durationMs: endedAt - startedAt,
        startPercent: toPercent(startedAt - origin),
        widthPercent: toPercent(endedAt - startedAt),
      })),
  };
}

export function evidenceCitations(
  result: RuleResult,
  events: EvidenceEvent[],
): EvidenceEvent[] {
  const byId = new Map(events.map((event) => [event.id, event]));
  return result.evidence_event_ids.flatMap((id) => byId.get(id) ?? []);
}

export function citedEventIds(summary: EvaluationSummary | null): Set<string> {
  return new Set(summary?.results.flatMap((result) => result.evidence_event_ids) ?? []);
}

export function verdictTitle(verdict: Verdict | null): string {
  switch (verdict) {
    case "PASS": return "Policy controls satisfied";
    case "FAIL": return "Policy violation detected";
    case "INCONCLUSIVE": return "More evidence required";
    default: return "Evaluation in progress";
  }
}

export function verdictSummary(
  verdict: Verdict | null,
  summary: EvaluationSummary | null,
  traceQuality: string | null,
): string {
  if (!summary || !verdict) {
    return "Featherlane is collecting and normalizing trace evidence before evaluating policy controls.";
  }
  const controls = summary.passed + summary.failed + summary.inconclusive;
  if (verdict === "PASS") {
    return `${summary.passed} of ${controls} deterministic controls passed against ${words(traceQuality ?? "available")} trace evidence.`;
  }
  if (verdict === "FAIL") {
    return `${summary.failed} of ${controls} controls failed with evidence cited from the evaluated trajectory.`;
  }
  return `${summary.inconclusive} of ${controls} controls could not be decided from the available evidence.`;
}

function attributeString(event: EvidenceEvent, key: string): string | null {
  const value = event.attributes[key];
  return typeof value === "string" ? value : null;
}

export function eventService(event: EvidenceEvent): string {
  return attributeString(event, "service.name") ?? event.actor.id;
}

export function eventFact(event: EvidenceEvent): string | null {
  const decision = attributeString(event, "decision");
  if (decision) return `Decision: ${words(decision)}`;

  if (event.input && typeof event.input === "object" && !Array.isArray(event.input)) {
    const input = event.input as Record<string, unknown>;
    if (typeof input.amount === "number") {
      const currency = typeof input.currency === "string" ? ` ${input.currency}` : "";
      return `Amount: ${input.amount.toLocaleString("en-US")}${currency}`;
    }
  }

  const terminal = attributeString(event, "featherlane.run.terminal_state")
    ?? attributeString(event, "terminal_state");
  return terminal ? `State: ${words(terminal)}` : null;
}

export function eventRelationship(event: EvidenceEvent): string | null {
  if (event.linked_event_ids.length > 0) {
    return `Linked to ${event.linked_event_ids.length} earlier event${event.linked_event_ids.length === 1 ? "" : "s"}`;
  }
  if (event.parent_event_id) return "Child of the preceding workflow event";
  return null;
}
