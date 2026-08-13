import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowLeft, Braces, CheckCircle2, Clock3, DollarSign, GitCommitHorizontal, RadioTower } from "lucide-react";
import { getEvaluation } from "@/lib/api";
import { formatDuration } from "@/lib/format";
import { MetricCard, StateBadge, VerdictBadge } from "@/components/ui";

export default async function EvaluationDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const run = await getEvaluation(id);
  if (!run) notFound();
  return (
    <div className="page run-detail">
      <Link className="back-link" href="/evaluations"><ArrowLeft size={14} />All evaluations</Link>
      <div className="run-hero">
        <div>
          <div className="eyebrow">Evaluation / {run.id.slice(0, 8)}</div>
          <div className="run-title"><h1>{run.target}</h1><VerdictBadge verdict={run.verdict} /></div>
          <p>Tested against <strong>{run.policy_pack}</strong>. This is evidence relevant to policy conformance, not a certification.</p>
        </div>
        <button className="secondary-button"><Braces size={15} />Download JSON</button>
      </div>

      <section className="metric-grid run-metrics">
        <MetricCard label="Rules passed" value={run.passed.toString()} detail={`${run.failed} failed · ${run.inconclusive} inconclusive`} />
        <MetricCard label="Duration" value={formatDuration(run.duration_ms)} detail="Target execution + evaluation" />
        <MetricCard label="Trace quality" value={run.trace_quality} detail="Normalized evidence coverage" />
        <MetricCard label="Estimated cost" value={run.cost_usd === null ? "Unavailable" : `$${run.cost_usd.toFixed(2)}`} detail="No target cost source configured" />
      </section>

      <section className="detail-grid">
        <article className="panel">
          <div className="section-header"><div><h2>Rule results</h2><p>Deterministic checks cite the evidence below</p></div></div>
          <div className="finding-list">
            {run.findings.length === 0 && <p className="empty-inline">No findings were persisted for this run.</p>}
            {run.findings.map((finding) => (
              <div className="finding" key={finding.rule_id}>
                <span className={`finding-icon ${finding.status}`}><CheckCircle2 size={16} /></span>
                <div><strong>{finding.rule_id.replaceAll("_", " ")}</strong><p>{finding.message}</p></div>
                <div className="finding-meta"><span>{finding.severity}</span><StateBadge state={finding.status} /></div>
              </div>
            ))}
          </div>
        </article>

        <aside className="panel run-context">
          <div className="section-header"><div><h2>Run context</h2><p>Immutable execution metadata</p></div></div>
          <dl>
            <div><dt><GitCommitHorizontal size={14} />Target version</dt><dd>{run.target_version}</dd></div>
            <div><dt><RadioTower size={14} />Trace quality</dt><dd><StateBadge state={run.trace_quality} /></dd></div>
            <div><dt><Clock3 size={14} />Started</dt><dd>{new Date(run.created_at).toLocaleString()}</dd></div>
            <div><dt><DollarSign size={14} />Cost</dt><dd>{run.cost_usd === null ? "Unavailable" : `$${run.cost_usd.toFixed(2)}`}</dd></div>
          </dl>
        </aside>
      </section>

      <section className="panel">
        <div className="section-header"><div><h2>Evidence trajectory</h2><p>Normalized events ordered by observed sequence</p></div></div>
        <div className="timeline">
          {run.timeline.length === 0 && <p className="empty-inline">No normalized events were persisted for this run.</p>}
          {run.timeline.map((event) => (
            <div className="timeline-event" key={`${event.sequence}-${event.name}`}>
              <span className="sequence">{String(event.sequence).padStart(2, "0")}</span>
              <i />
              <div><span>{event.event_type.replaceAll("_", " ")}</span><strong>{event.name}</strong></div>
              <code>{event.actor}</code>
              <StateBadge state={event.outcome} />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
