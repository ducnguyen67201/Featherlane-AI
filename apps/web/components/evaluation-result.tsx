import type { CSSProperties, ReactNode } from "react";
import Link from "next/link";
import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  Bot,
  Check,
  CheckCircle2,
  CircleDashed,
  Clock3,
  FileCheck2,
  Fingerprint,
  Flag,
  GitCommitHorizontal,
  GitMerge,
  Network,
  RadioTower,
  ShieldCheck,
  UserRound,
  Wrench,
  XCircle,
} from "lucide-react";
import { EvidenceDownload } from "@/components/evidence-download";
import { RunRefresh } from "@/components/run-refresh";
import { StateBadge, VerdictBadge } from "@/components/ui";
import {
  citedEventIds,
  eventFact,
  eventRelationship,
  eventService,
  evidenceCitations,
  runDuration,
  shortId,
  traceWaterfall,
  verdictSummary,
  verdictTitle,
  words,
} from "@/lib/evaluation-view";
import { formatDuration } from "@/lib/format";
import type {
  EvaluationRunDetail,
  EvidenceEvent,
  RuleResult,
  Verdict,
} from "@/lib/types";
import { displayVerdict } from "@/lib/types";

function eventIcon(event: EvidenceEvent): ReactNode {
  switch (event.event_type) {
    case "human_approval_decision": return <UserRound size={17} />;
    case "tool_call": return <Wrench size={17} />;
    case "final_output": return <Flag size={17} />;
    default: return <Bot size={17} />;
  }
}

function resultIcon(status: string): ReactNode {
  switch (status.toLowerCase()) {
    case "pass": return <CheckCircle2 size={19} />;
    case "fail": return <XCircle size={19} />;
    default: return <AlertTriangle size={19} />;
  }
}

function formatTimestamp(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}

function formatEventTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "time unavailable";
  return new Intl.DateTimeFormat("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    fractionalSecondDigits: 3,
  }).format(date);
}

function TraceLegend({ traceIds }: { traceIds: string[] }) {
  return (
    <div className="trace-legend" aria-label="Trace legend">
      {traceIds.map((traceId, index) => (
        <span className={`trace-chip trace-tone-${index % 4}`} key={traceId}>
          <i aria-hidden="true" />Trace {index + 1}<code>{shortId(traceId)}</code>
        </span>
      ))}
    </div>
  );
}

const WATERFALL_TICKS = [0, 0.25, 0.5, 0.75, 1];

function TraceWaterfallView({
  events,
  traceIndexes,
  citedIds,
}: {
  events: EvidenceEvent[];
  traceIndexes: Map<string, number>;
  citedIds: Set<string>;
}) {
  const waterfall = traceWaterfall(events);
  if (waterfall.segments.length === 0) return null;

  return (
    <section className="trace-waterfall" aria-labelledby="trace-waterfall-title">
      <div className="waterfall-heading">
        <div>
          <span>Temporal view</span>
          <h3 id="trace-waterfall-title">Trace waterfall</h3>
          <p>Each bar is positioned and sized from the event&apos;s observed start and end timestamps.</p>
        </div>
        <div className="waterfall-duration"><Clock3 size={14} /><span>Observed window</span><strong>{formatDuration(waterfall.durationMs)}</strong></div>
      </div>

      <div className="waterfall-scroll" tabIndex={0} aria-label="Horizontally scrollable trace waterfall">
        <div className="waterfall-chart">
          <div className="waterfall-scale">
            <span>Event / service</span>
            <div>
              {WATERFALL_TICKS.map((tick) => (
                <i
                  key={tick}
                  style={{
                    left: `${tick * 100}%`,
                    transform: `translateX(${tick === 0 ? 0 : tick === 1 ? -100 : -50}%)`,
                  }}
                >
                  +{formatDuration(Math.round(waterfall.durationMs * tick))}
                </i>
              ))}
            </div>
            <span>Timing</span>
          </div>

          <div className="waterfall-rows">
            {waterfall.segments.map((segment) => {
              const { event } = segment;
              const traceIndex = traceIndexes.get(event.trace_id) ?? 0;
              const style = {
                "--waterfall-left": `${segment.startPercent}%`,
                "--waterfall-width": `${segment.widthPercent}%`,
              } as CSSProperties;
              return (
                <a
                  className={`waterfall-row trace-tone-${traceIndex % 4}`}
                  href={`#event-${event.id}`}
                  key={event.id}
                  aria-label={`${event.name}, starts ${formatDuration(segment.offsetMs)} after the first event and lasts ${formatDuration(segment.durationMs)}`}
                >
                  <div className="waterfall-label">
                    <span><b>#{String(event.sequence).padStart(2, "0")}</b>{event.name}</span>
                    <small><i aria-hidden="true" />Trace {traceIndex + 1} · {eventService(event)}</small>
                  </div>
                  <div className="waterfall-track">
                    <span
                      className={`waterfall-bar${citedIds.has(event.id) ? " cited" : ""}`}
                      style={style}
                      title={`${words(event.event_type)} · ${formatDuration(segment.durationMs)}`}
                    >
                      <span>{words(event.event_type)}</span>
                      {citedIds.has(event.id) && <FileCheck2 size={11} aria-label="Cited by policy" />}
                    </span>
                  </div>
                  <div className="waterfall-timing"><strong>+{formatDuration(segment.offsetMs)}</strong><span>{formatDuration(segment.durationMs)}</span></div>
                </a>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

function RuleResultCard({ result, events }: { result: RuleResult; events: EvidenceEvent[] }) {
  const citations = evidenceCitations(result, events);
  const status = result.status.toLowerCase();
  return (
    <article className={`control-result control-${status}`}>
      <div className="control-result-icon" aria-hidden="true">{resultIcon(status)}</div>
      <div className="control-result-body">
        <div className="control-result-title">
          <div>
            <span className="control-id">{result.rule_id}</span>
            <h3>{status === "pass" ? "Control satisfied" : status === "fail" ? "Control failed" : "Evidence incomplete"}</h3>
          </div>
          <div className="control-result-status"><span>{result.severity}</span><StateBadge state={status} /></div>
        </div>
        <p>{result.message}</p>
        <div className="citation-block">
          <span>Evidence used</span>
          {citations.length > 0 ? (
            <div className="citation-list">
              {citations.map((citation) => (
                <a href={`#event-${citation.id}`} key={citation.id}>
                  <span>#{String(citation.sequence).padStart(2, "0")}</span>
                  <strong>{citation.name}</strong>
                  <small>{words(citation.eventType)}</small>
                  <ArrowRight size={12} aria-hidden="true" />
                </a>
              ))}
            </div>
          ) : <small>No event citation was persisted for this result.</small>}
        </div>
      </div>
    </article>
  );
}

function EvidenceEventCard({
  event,
  traceIndex,
  cited,
}: {
  event: EvidenceEvent;
  traceIndex: number;
  cited: boolean;
}) {
  const fact = eventFact(event);
  const relationship = eventRelationship(event);
  const hasDetails = event.input !== null
    || event.output !== null
    || Object.keys(event.attributes).length > 0;
  return (
    <article
      className={`evidence-event trace-tone-${traceIndex % 4}${cited ? " cited" : ""}`}
      id={`event-${event.id}`}
    >
      <div className="event-rail" aria-hidden="true">
        <span>{String(event.sequence).padStart(2, "0")}</span>
        <i />
      </div>
      <div className="event-icon" aria-hidden="true">{eventIcon(event)}</div>
      <div className="event-content">
        <div className="event-heading">
          <div>
            <span>{words(event.event_type)}</span>
            <h3>{event.name}</h3>
          </div>
          {cited && <span className="cited-label"><FileCheck2 size={12} />Cited by policy</span>}
        </div>
        <div className="event-facts">
          <span><strong>{words(event.actor.actor_type)}</strong>{event.actor.id}</span>
          <span><strong>Service</strong>{eventService(event)}</span>
          {fact && <span><strong>Observed</strong>{fact}</span>}
          {relationship && <span><strong>Relationship</strong>{relationship}</span>}
        </div>
        {hasDetails && (
          <details className="event-details">
            <summary>Inspect normalized evidence</summary>
            <pre>{JSON.stringify({ input: event.input, output: event.output, attributes: event.attributes }, null, 2)}</pre>
          </details>
        )}
      </div>
      <div className="event-source">
        <time dateTime={event.started_at}>{formatEventTime(event.started_at)}</time>
        <span><i aria-hidden="true" />Trace {traceIndex + 1}</span>
        <code>{shortId(event.source_span_id)}</code>
      </div>
    </article>
  );
}

export function EvaluationResult({ detail }: { detail: EvaluationRunDetail }) {
  const { run, summary, evidence } = detail;
  const verdict: Verdict | null = run.verdict ? displayVerdict(run.verdict) : null;
  const events = evidence?.events ?? [];
  const traceIds = evidence?.trace_ids?.length
    ? evidence.trace_ids
    : [...new Set(events.map((event) => event.trace_id))];
  const traceIndexes = new Map(traceIds.map((traceId, index) => [traceId, index]));
  const citedIds = citedEventIds(summary);
  const controls = summary ? summary.passed + summary.failed + summary.inconclusive : 0;
  const evidenceHash = evidence?.evidence_sha256 ?? run.evidence_sha256;
  const heroTone = verdict?.toLowerCase() ?? "pending";
  const completionDetail = run.completion_reason === "terminal_event"
    ? "Triggered automatically when the agent session ended"
    : run.completion_reason
      ? `Triggered by ${words(run.completion_reason)}`
      : "Waiting for the workflow boundary";

  return (
    <div className="page evaluation-result-page">
      <RunRefresh state={run.state} />
      <Link className="back-link" href="/evaluations"><ArrowLeft size={14} />All evaluations</Link>

      <section className={`evaluation-outcome outcome-${heroTone}`} aria-labelledby="evaluation-outcome-title">
        <div className="outcome-copy">
          <div className="outcome-kicker"><ShieldCheck size={14} />Trace evaluation · {words(run.state)}</div>
          <div className="outcome-heading">
            <div>
              <h1 id="evaluation-outcome-title">{verdictTitle(verdict)}</h1>
              <p>{verdictSummary(verdict, summary, evidence?.trace_quality ?? run.trace_quality)}</p>
            </div>
            {verdict ? <VerdictBadge verdict={verdict} /> : <StateBadge state={run.state} />}
          </div>
          <div className="outcome-subjects">
            <span><Bot size={13} /><strong>Target</strong>{run.target_id}</span>
            <span><FileCheck2 size={13} /><strong>Policy</strong>{run.policy_pack_key} v{run.policy_pack_version}</span>
            <span><GitCommitHorizontal size={13} /><strong>Version</strong>{run.target_version}</span>
          </div>
        </div>
        <div className="outcome-actions">
          {evidence && <EvidenceDownload evidence={evidence} runId={run.id} />}
          <code>{shortId(run.id, 12)}</code>
        </div>
        <div className="evaluation-flow" aria-label="Evaluation processing stages">
          <div><span><RadioTower size={15} /></span><p><strong>Trace received</strong>{run.span_count} spans across {run.trace_count} traces</p></div>
          <i aria-hidden="true"><ArrowRight size={14} /></i>
          <div><span><GitMerge size={15} /></span><p><strong>Evidence normalized</strong>{run.event_count} causally ordered events</p></div>
          <i aria-hidden="true"><ArrowRight size={14} /></i>
          <div><span><ShieldCheck size={15} /></span><p><strong>Controls evaluated</strong>{controls || "Pending"} deterministic checks</p></div>
          <i aria-hidden="true"><ArrowRight size={14} /></i>
          <div className="flow-verdict"><span>{verdict === "PASS" ? <Check size={15} /> : verdict === "FAIL" ? <XCircle size={15} /> : <CircleDashed size={15} />}</span><p><strong>Verdict produced</strong>{verdict ? words(verdict.toLowerCase()) : words(run.state)}</p></div>
        </div>
      </section>

      <section className="evaluation-stat-grid" aria-label="Evaluation metrics">
        <article><span>Controls</span><strong>{summary ? `${summary.passed}/${controls}` : "—"}</strong><small>{summary ? `${summary.failed} failed · ${summary.inconclusive} inconclusive` : "Evaluation pending"}</small></article>
        <article><span>Trace quality</span><strong>{words(evidence?.trace_quality ?? run.trace_quality ?? "pending")}</strong><small>{evidence?.trace_defects.length ?? 0} evidence defects</small></article>
        <article><span>Run duration</span><strong>{runDuration(run)}</strong><small>{completionDetail}</small></article>
        <article><span>Evidence integrity</span><strong>{evidenceHash ? "Verified" : "Pending"}</strong><small>{evidenceHash ? `${shortId(evidenceHash, 16)}…` : "Hash created at finalization"}</small></article>
      </section>

      {!evidence && (
        <section className="panel collecting-panel">
          <CircleDashed className="spin" size={22} />
          <div><h2>Collecting trace evidence</h2><p>Export spans with these correlation attributes. The result UI will populate after finalization.</p></div>
          <pre>{`featherlane.eval_run.id=${run.id}\nfeatherlane.invocation.id=${run.primary_invocation_id}\nfeatherlane.scenario.id=${run.scenario_id}`}</pre>
        </section>
      )}

      <section className="evaluation-content-grid">
        <div className="panel controls-panel">
          <div className="section-header">
            <div><h2>Policy results</h2><p>Every conclusion links back to the exact normalized events that support it.</p></div>
            {summary && <span className="section-count">{controls} controls</span>}
          </div>
          {summary?.results.length
            ? <div className="control-results">{summary.results.map((result) => <RuleResultCard events={events} key={result.id ?? result.rule_id} result={result} />)}</div>
            : <p className="empty-inline">No policy results have been finalized yet.</p>}
        </div>

        <aside className="panel evaluation-context">
          <div className="section-header"><div><h2>Evaluation context</h2><p>Pinned inputs and immutable output</p></div></div>
          <dl>
            <div><dt><Network size={14} />Boundary</dt><dd>{words(run.boundary_kind)}</dd></div>
            {run.external_run_id && <div><dt><Fingerprint size={14} />External session</dt><dd><code title={run.external_run_id}>{shortId(run.external_run_id, 20)}{run.external_run_id.length > 20 ? "…" : ""}</code></dd></div>}
            <div><dt><Fingerprint size={14} />Run ID</dt><dd><code title={run.id}>{shortId(run.id, 16)}…</code></dd></div>
            <div><dt><GitCommitHorizontal size={14} />Target version</dt><dd><code>{run.target_version}</code></dd></div>
            <div><dt><FileCheck2 size={14} />Policy version</dt><dd>{run.policy_pack_key} v{run.policy_pack_version}</dd></div>
            <div><dt><Clock3 size={14} />Started</dt><dd>{formatTimestamp(run.created_at)}</dd></div>
            <div><dt><CheckCircle2 size={14} />Finalized</dt><dd>{formatTimestamp(run.finalized_at)}</dd></div>
          </dl>
          <div className="integrity-note">
            <Fingerprint size={15} />
            <div><strong>Immutable evidence</strong><span>{evidenceHash ? `${shortId(evidenceHash, 24)}…` : "Created after finalization"}</span></div>
          </div>
        </aside>
      </section>

      {evidence && (
        <section className="panel trajectory-panel">
          <div className="trajectory-header">
            <div>
              <div className="eyebrow">Evidence trajectory</div>
              <h2>What happened across the workflow</h2>
              <p>Events are causally ordered across trace boundaries before policy evaluation.</p>
            </div>
            <TraceLegend traceIds={traceIds} />
          </div>

          {evidence.trace_defects.length > 0 && (
            <div className="trace-defects">
              {evidence.trace_defects.map((defect) => (
                <p key={`${defect.code}-${defect.message}`}><AlertTriangle size={14} /><strong>{words(defect.code)}</strong>{defect.message}<StateBadge state={defect.blocking ? "blocking" : "degraded"} /></p>
              ))}
            </div>
          )}

          <TraceWaterfallView citedIds={citedIds} events={events} traceIndexes={traceIndexes} />

          <div className="evidence-list-heading">
            <div><span>Normalized evidence</span><h3>Event detail</h3></div>
            <p>Select a waterfall row or policy citation to jump to its full evidence record.</p>
          </div>

          <div className="evidence-events">
            {events.map((event) => (
              <EvidenceEventCard
                cited={citedIds.has(event.id)}
                event={event}
                key={event.id}
                traceIndex={traceIndexes.get(event.trace_id) ?? 0}
              />
            ))}
          </div>

          <footer className="trajectory-footer">
            <span><CheckCircle2 size={14} />{events.length} normalized events</span>
            <span><Network size={14} />{traceIds.length} correlated traces</span>
            <span><ShieldCheck size={14} />Sensitive fields redacted</span>
            <span><Fingerprint size={14} />Bundle {evidence.schema_version ?? "1.0"}</span>
          </footer>
        </section>
      )}
    </div>
  );
}
