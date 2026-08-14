import Link from "next/link";
import { Activity, AlertTriangle, Bot, CheckCircle2, RadioTower, ScrollText } from "lucide-react";
import { ActivityChart } from "@/components/activity-chart";
import { RunTable } from "@/components/run-table";
import { MetricCard, PageHeader, SectionHeader, StateBadge } from "@/components/ui";
import { getAgents, getOverview } from "@/lib/api";

export default async function OverviewPage() {
  const [overview, agents] = await Promise.all([getOverview(), getAgents()]);
  const completeTargets = agents.filter((agent) => agent.latest_trace_quality === "complete").length;
  const needsInstrumentation = agents.filter((agent) => agent.latest_trace_quality && agent.latest_trace_quality !== "complete").length;
  return (
    <div className="page">
      <PageHeader
        eyebrow="Governance workspace / Overview"
        title="Agent governance, backed by evidence"
        description="Test agent trajectories against approved policy packs before deployment and inspect production traces without taking over the customer workflow."
        action={<Link className="primary-button" href="/agents"><Bot size={16} />Configure an agent</Link>}
      />

      <section className="metric-grid" aria-label="Governance summary">
        <MetricCard label="Pass rate" value={`${overview.pass_rate.toFixed(1)}%`} detail="Persisted live runs" />
        <MetricCard label="Evaluations" value={overview.evaluations_30d.toLocaleString()} detail="Across active targets" />
        <MetricCard label="Trace coverage" value={`${overview.trace_coverage.toFixed(1)}%`} detail="Complete evidence" />
        <MetricCard label="Open findings" value={overview.open_findings.toString()} detail="Failed deterministic rules" />
      </section>

      <section className="split-grid">
        <article className="panel chart-panel">
          <SectionHeader title="Evaluation activity" description="Deterministic outcomes by day" href="/evaluations" linkLabel="View all runs" />
          <ActivityChart data={overview.daily_activity} />
        </article>
        <article className="panel posture-panel">
          <SectionHeader title="Governance posture" description="Current evidence boundaries" />
          <div className="posture-score">
            <div className="score-ring"><strong>{Math.round(overview.trace_coverage)}</strong><span>/ 100</span></div>
            <div><strong>Evidence coverage</strong><p>Complete evidence on {completeTargets} of {agents.length} connected targets.</p></div>
          </div>
          <div className="posture-list">
            <div><CheckCircle2 className="pass-text" size={17} /><span>Database policy packs</span><b>{overview.policy_packs}</b></div>
            <div><RadioTower className="accent-text" size={17} /><span>Complete trace coverage</span><b>{completeTargets} agents</b></div>
            <div><AlertTriangle className="warn-text" size={17} /><span>Needs instrumentation</span><b>{needsInstrumentation} agents</b></div>
          </div>
        </article>
      </section>

      <section className="panel">
        <SectionHeader title="Recent evaluations" description="Latest CI and active test runs" href="/evaluations" linkLabel="Evaluation history" />
        <RunTable runs={overview.recent_runs} />
      </section>

      <section className="panel">
        <SectionHeader title="Connected agents" description="Target reachability and trace readiness" href="/agents" linkLabel="Manage targets" />
        <div className="agent-strip">
          {agents.map((agent, index) => {
            const Icon = index === 0 ? Bot : index === 1 ? Activity : ScrollText;
            return (
              <article key={agent.id}>
                <div className="agent-icon"><Icon size={18} /></div>
                <div><strong>{agent.name}</strong><span>{agent.driver} · {agent.environment}</span></div>
                <StateBadge state={agent.status} />
                <div className="coverage"><span>{agent.latest_trace_quality ?? "No completed run"}</span><i><b style={{ width: agent.latest_trace_quality === "complete" ? "100%" : agent.latest_trace_quality ? "55%" : "0%" }} /></i></div>
              </article>
            );
          })}
        </div>
      </section>

      <p className="certification-note">Featherlane produces policy-conformance evidence. Results are not a legal opinion or compliance certification.</p>
    </div>
  );
}
