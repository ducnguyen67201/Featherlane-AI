import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowLeft, Braces, Clock3, RadioTower } from "lucide-react";
import { getAgent, getPolicies } from "@/lib/api";
import { TargetActions } from "@/components/target-actions";
import { StateBadge } from "@/components/ui";

export default async function TargetDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [target, policies] = await Promise.all([getAgent(id), getPolicies()]);
  if (!target) notFound();
  const approvedPolicies = (policies.data ?? [])
    .filter((policy) => policy.status === "approved")
    .map(({ id: policyId, title }) => ({ id: policyId, title }));

  return (
    <div className="page target-detail-page">
      <Link className="back-link" href="/agents"><ArrowLeft size={14} />Connected agents</Link>
      <div className="run-hero">
        <div>
          <div className="eyebrow">Target / {target.manifest.target_id}</div>
          <div className="run-title"><h1>{target.name}</h1><StateBadge state={target.status} /></div>
          <p>{target.manifest.driver_type.replaceAll("_", " ")} · {target.environment} · {target.version}</p>
        </div>
      </div>

      <section className="detail-grid">
        <article className="panel">
          <div className="section-header"><div><h2>Connection readiness</h2><p>Latest server-side GET probe</p></div><StateBadge state={target.status} /></div>
          <dl className="manifest-facts">
            <div><dt><RadioTower size={14} />Latest trace quality</dt><dd>{target.latest_trace_quality ?? "No completed run"}</dd></div>
            <div><dt><Clock3 size={14} />Checked</dt><dd>{new Date(target.checked_at).toLocaleString()}</dd></div>
          </dl>
          {target.issues.length > 0 && <ul className="issue-list">{target.issues.map((issue) => <li key={issue}>{issue}</li>)}</ul>}
        </article>
        <article className="panel manifest-panel">
          <div className="section-header"><div><h2>Integration manifest</h2><p>Secret references are shown; their values never are.</p></div><Braces size={17} /></div>
          <pre tabIndex={0}>{JSON.stringify(target.manifest, null, 2)}</pre>
        </article>
      </section>

      <TargetActions target={target} policies={approvedPolicies} />
    </div>
  );
}
