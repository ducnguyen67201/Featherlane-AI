import Link from "next/link";
import { Bot, Cable, Database, Plus, RadioTower, RotateCcw } from "lucide-react";
import { getAgents } from "@/lib/api";
import { PageHeader, StateBadge } from "@/components/ui";

export default async function AgentsPage() {
  const agents = await getAgents();
  return (
    <div className="page">
      <PageHeader
        eyebrow="Execution lane / Targets"
        title="Connected agents"
        description="Drive a test wrapper directly or observe standard OTLP traces and evaluate each completed agent session automatically."
        action={<Link className="primary-button" href="/agents/new"><Plus size={16} />Connect target</Link>}
      />
      {agents.length === 0 ? (
        <section className="panel policy-empty">
          <Database size={27} />
          <h2>No targets connected</h2>
          <p>Register a staging, preview, or sandbox wrapper to run the same evaluation from this console and CI.</p>
          <Link className="primary-button" href="/agents/new"><Plus size={16} />Connect your first target</Link>
        </section>
      ) : (
        <section className="agent-grid">
          {agents.map((agent) => (
            <article className="panel agent-card" key={agent.id}>
              <div className="agent-card-head">
                <span className="agent-icon large"><Bot size={20} /></span>
                <div><h2>{agent.name}</h2><code>{agent.version}</code></div>
                <StateBadge state={agent.status} />
              </div>
              <dl>
                <div><dt><Cable size={14} />Adapter</dt><dd>{agent.driver.replaceAll("_", " ")}</dd></div>
                <div><dt><RadioTower size={14} />Trace quality</dt><dd>{agent.latest_trace_quality?.replaceAll("_", " ") ?? "No runs"}</dd></div>
                <div><dt><Database size={14} />Automatic evaluation</dt><dd>{agent.auto_evaluation_enabled ? agent.automatic_boundary_kind?.replaceAll("_", " ") : "Not configured"}</dd></div>
                <div><dt><RotateCcw size={14} />Last evaluated</dt><dd>{agent.last_evaluated ? new Date(agent.last_evaluated).toLocaleString() : "Never"}</dd></div>
              </dl>
              <div className="target-meta"><span>{agent.environment}</span><span>Checked {new Date(agent.checked_at).toLocaleString()}</span></div>
              <Link className="secondary-button full-button" href={`/agents/${encodeURIComponent(agent.id)}`}>View integration details</Link>
            </article>
          ))}
        </section>
      )}
      <section className="panel integration-contract">
        <div><span>01</span><strong>Identify the agent</strong><p>A target-scoped ingest key supplies trusted organization and agent identity.</p></div>
        <div><span>02</span><strong>Group the session</strong><p>One configured attribute correlates every trace in the business execution.</p></div>
        <div><span>03</span><strong>Signal finished</strong><p>A terminal boolean closes collection after a short settle window.</p></div>
        <div><span>04</span><strong>Evaluate evidence</strong><p>The assigned policy returns pass, fail, or inconclusive without controlling the agent.</p></div>
      </section>
    </div>
  );
}
