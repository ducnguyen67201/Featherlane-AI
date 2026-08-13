import { Activity, AlertTriangle, Bot, CheckCircle2, RadioTower, ScrollText } from "lucide-react";
import { ActivityChart } from "@/components/activity-chart";
import { RunEvaluationButton } from "@/components/run-evaluation-button";
import { RunTable } from "@/components/run-table";
import { MetricCard, PageHeader, SectionHeader, StateBadge } from "@/components/ui";
import { getAgents, getOverview, getPolicies } from "@/lib/api";

export default async function OverviewPage() {
  const [overview, agents, policies] = await Promise.all([getOverview(), getAgents(), getPolicies()]);
  const policyPackId = policies.data?.find((policy) => policy.status === "approved")?.id;
  return (
    <div className="page">
      <PageHeader
        eyebrow="Governance workspace / Overview"
        title="Agent governance, backed by evidence"
        description="Test agent trajectories against approved policy packs before deployment and inspect production traces without taking over the customer workflow."
        action={<RunEvaluationButton policyPackId={policyPackId} />}
      />

      <section className="metric-grid" aria-label="Governance summary">
        <MetricCard label="Pass rate" value={`${overview.pass_rate}%`} detail="Last 30 days" trend="+2.4%" />
        <MetricCard label="Evaluations" value={overview.evaluations_30d.toLocaleString()} detail="Across active targets" trend="+18" />
        <MetricCard label="Trace coverage" value={`${overview.trace_coverage}%`} detail="Observable evidence" trend="+1.8%" />
        <MetricCard label="Open findings" value={overview.open_findings.toString()} detail="2 high severity" />
      </section>

      <section className="split-grid">
        <article className="panel chart-panel">
          <SectionHeader title="Evaluation activity" description="Deterministic outcomes by day" href="/evaluations" linkLabel="View all runs" />
          <ActivityChart data={overview.daily_activity} />
        </article>
        <article className="panel posture-panel">
          <SectionHeader title="Governance posture" description="Current evidence boundaries" />
          <div className="posture-score">
            <div className="score-ring"><strong>92</strong><span>/ 100</span></div>
            <div><strong>Strong coverage</strong><p>Critical controls are observable on 2 of 3 targets.</p></div>
          </div>
          <div className="posture-list">
            <div><CheckCircle2 className="pass-text" size={17} /><span>Database policy packs</span><b>{overview.policy_packs}</b></div>
            <div><RadioTower className="accent-text" size={17} /><span>Complete trace coverage</span><b>2 agents</b></div>
            <div><AlertTriangle className="warn-text" size={17} /><span>Needs instrumentation</span><b>1 agent</b></div>
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
                <div className="coverage"><span>{agent.trace_coverage}% trace</span><i><b style={{ width: `${agent.trace_coverage}%` }} /></i></div>
              </article>
            );
          })}
        </div>
      </section>

      <p className="certification-note">Featherlane produces policy-conformance evidence. Results are not a legal opinion or compliance certification.</p>
    </div>
  );
}
