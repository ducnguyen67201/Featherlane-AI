import Link from "next/link";
import { ArrowLeft, Braces, GitCommitHorizontal, RadioTower } from "lucide-react";
import { RunRefresh } from "@/components/run-refresh";
import { MetricCard, StateBadge, VerdictBadge } from "@/components/ui";
import { getEvaluation } from "@/lib/api";
import type { Verdict } from "@/lib/types";

export default async function EvaluationDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const detail = await getEvaluation(id);
  if (!detail) {
    return <div className="page"><Link className="back-link" href="/evaluations"><ArrowLeft size={14} />All evaluations</Link><section className="panel"><h1>Evaluation unavailable</h1><p>The run was not found or the governance API is unavailable. No synthetic result was substituted.</p></section></div>;
  }
  const { run, summary, evidence } = detail;
  const verdict = run.verdict?.toUpperCase() as Verdict | undefined;
  return (
    <div className="page run-detail">
      <RunRefresh state={run.state} />
      <Link className="back-link" href="/evaluations"><ArrowLeft size={14} />All evaluations</Link>
      <div className="run-hero">
        <div>
          <div className="eyebrow">Evaluation / {run.id.slice(0, 8)}</div>
          <div className="run-title"><h1>{run.target_id}</h1>{verdict ? <VerdictBadge verdict={verdict} /> : <StateBadge state={run.state} />}</div>
          <p>Evaluating <strong>{run.policy_pack_key} v{run.policy_pack_version}</strong>. This is evidence relevant to policy conformance, not a certification.</p>
        </div>
        {evidence && <button className="secondary-button"><Braces size={15} />Download JSON</button>}
      </div>

      <section className="metric-grid run-metrics">
        <MetricCard label="Lifecycle" value={run.state} detail={run.completion_reason ? `Completed by ${run.completion_reason}` : "Waiting for explicit completion"} />
        <MetricCard label="Spans" value={run.span_count.toString()} detail={`${run.trace_count} traces · ${run.event_count} events`} />
        <MetricCard label="Trace quality" value={run.trace_quality ?? "pending"} detail="Assessed only after finalization" />
        <MetricCard label="Boundary" value={run.boundary_kind.replaceAll("_", " ")} detail={run.external_run_id ?? "Explicit Featherlane run ID"} />
      </section>

      {!evidence && <section className="panel"><h2>Collecting evidence</h2><p>Add these attributes to exported spans, then explicitly complete this run after the full workflow or agent task ends.</p><pre>{`featherlane.eval_run.id=${run.id}\nfeatherlane.invocation.id=${run.primary_invocation_id}\nfeatherlane.scenario.id=${run.scenario_id}`}</pre></section>}

      <section className="detail-grid">
        <article className="panel">
          <div className="section-header"><div><h2>Rule results</h2><p>Only persisted deterministic results are shown</p></div></div>
          {summary?.results.length ? <div className="finding-list">{summary.results.map((finding) => <div className="finding" key={finding.rule_id}><div><strong>{finding.rule_id.replaceAll("_", " ")}</strong><p>{finding.message}</p></div><div className="finding-meta"><span>{finding.severity}</span><StateBadge state={finding.status} /></div></div>)}</div> : <p>No rule results exist yet.</p>}
        </article>
        <aside className="panel run-context"><div className="section-header"><div><h2>Run context</h2><p>Immutable execution metadata</p></div></div><dl>
          <div><dt><GitCommitHorizontal size={14} />Target version</dt><dd>{run.target_version}</dd></div>
          <div><dt><RadioTower size={14} />Evaluation run</dt><dd><code>{run.id}</code></dd></div>
          <div><dt>Policy SHA</dt><dd><code>{run.policy_content_sha256.slice(0, 16)}</code></dd></div>
          <div><dt>Started</dt><dd>{new Date(run.created_at).toLocaleString()}</dd></div>
        </dl></aside>
      </section>

      {evidence && <section className="panel"><div className="section-header"><div><h2>Evidence trajectory</h2><p>Run-global causal ordering across every trace</p></div></div>{evidence.trace_defects.length > 0 && <div>{evidence.trace_defects.map((defect) => <p key={`${defect.code}-${defect.message}`}><StateBadge state={defect.blocking ? "blocking" : "degraded"} /> {defect.message}</p>)}</div>}<div className="timeline">{evidence.events.map((event) => <div className="timeline-event" key={event.id}><span className="sequence">{String(event.sequence).padStart(2, "0")}</span><i /><div><span>{event.event_type.replaceAll("_", " ")}</span><strong>{event.name}</strong></div><code>{event.actor.id}</code><StateBadge state="observed" /></div>)}</div></section>}
    </div>
  );
}
