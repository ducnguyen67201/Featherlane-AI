import { Bot, Cable, Plus, RadioTower, RotateCcw } from "lucide-react";
import { getAgents } from "@/lib/api";
import { PageHeader, StateBadge } from "@/components/ui";

export default async function AgentsPage() {
  const agents = await getAgents();
  return (
    <div className="page">
      <PageHeader
        eyebrow="Execution lane / Targets"
        title="Connected agents"
        description="Drive HTTP agents and webhook workflows with synthetic events, propagate trace context, and evaluate the resulting evidence."
        action={<button className="primary-button"><Plus size={16} />Connect target</button>}
      />
      <section className="agent-grid">
        {agents.map((agent) => (
          <article className="panel agent-card" key={agent.id}>
            <div className="agent-card-head">
              <span className="agent-icon large"><Bot size={20} /></span>
              <div><h2>{agent.name}</h2><code>{agent.version}</code></div>
              <StateBadge state={agent.status} />
            </div>
            <dl>
              <div><dt><Cable size={14} />Adapter</dt><dd>{agent.driver}</dd></div>
              <div><dt><RadioTower size={14} />Trace coverage</dt><dd>{agent.trace_coverage}%</dd></div>
              <div><dt><RotateCcw size={14} />Last evaluated</dt><dd>{agent.last_evaluated}</dd></div>
            </dl>
            <div className="coverage wide"><i><b style={{ width: `${agent.trace_coverage}%` }} /></i><span>{agent.environment}</span></div>
            <button className="secondary-button full-button">View integration manifest</button>
          </article>
        ))}
      </section>
      <section className="panel integration-contract">
        <div><span>01</span><strong>Send synthetic event</strong><p>HTTP text, webhook, human decision, timer, or system event.</p></div>
        <div><span>02</span><strong>Propagate context</strong><p>W3C traceparent plus run, scenario, and invocation identifiers.</p></div>
        <div><span>03</span><strong>Observe trajectory</strong><p>OpenTelemetry/OpenInference spans become normalized governance evidence.</p></div>
        <div><span>04</span><strong>Evaluate evidence</strong><p>Rules return pass, fail, or inconclusive without controlling the agent.</p></div>
      </section>
    </div>
  );
}
