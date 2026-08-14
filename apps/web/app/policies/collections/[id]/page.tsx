import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowLeft, FileText, ShieldCheck } from "lucide-react";
import { PolicyCollectionActions } from "@/components/policy-collection-actions";
import { PolicyCollectionMemberActions } from "@/components/policy-collection-member-actions";
import { PolicySourceAcquisition } from "@/components/policy-source-acquisition";
import { IngestionBatchProgress } from "@/components/ingestion-batch-progress";
import { PageHeader, StateBadge } from "@/components/ui";
import { getPolicyCollection, getPolicyCollectionReadiness, getPolicyImport, getSourceConnections } from "@/lib/api";

export default async function PolicyCollectionPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [detail, readiness, connections] = await Promise.all([getPolicyCollection(id), getPolicyCollectionReadiness(id), getSourceConnections()]);
  if (!detail.data && !detail.error) notFound();
  if (!detail.data) return <div className="page"><section className="panel policy-empty"><h2>Collection unavailable</h2><p>{detail.error}</p></section></div>;
  const imports = await Promise.all(detail.data.members.map((member) => getPolicyImport(member.policy_import_id)));
  const collection = detail.data.collection;
  const state = readiness.data;
  return <div className="page">
    <Link className="back-link" href="/policies"><ArrowLeft size={14} /> Policies</Link>
    <PageHeader eyebrow={`${collection.key} / v${collection.version}`} title={collection.title} description="Review every exact source revision, then compile one deterministic multi-source draft pack." action={<PolicyCollectionActions collectionId={id} version={collection.version} title={collection.title} ready={Boolean(state && state.blockers.length === 0 && state.collection_blockers.length === 0)} compiledPackId={collection.compiled_policy_pack_id} />} />
    <section className="collection-stats"><article><FileText size={18} /><span>Sources</span><strong>{state?.source_count ?? detail.data.members.length}</strong></article><article><ShieldCheck size={18} /><span>Review complete</span><strong>{state?.review_complete_count ?? 0}</strong></article><article><ShieldCheck size={18} /><span>Approved rules</span><strong>{state?.approved_rule_count ?? 0}</strong></article><article><StateBadge state={collection.status} /></article></section>
    {collection.status === "draft" && <PolicySourceAcquisition collectionId={id} connections={connections.data ?? []} />}
    {detail.data.batches.length > 0 && <section className="panel"><div className="section-header"><div><h2>Recent ingestion batches</h2><p>Progress and per-item results remain available after refresh.</p></div></div>{detail.data.batches.slice(0, 5).map((batch) => <IngestionBatchProgress key={batch.id} initialBatch={batch} />)}</section>}
    <section className="panel"><div className="section-header"><div><h2>Exact source revisions</h2><p>Compilation freezes this membership.</p></div></div><div className="import-list">{imports.map((result, index) => { const source = result.data; const member = detail.data!.members[index]; return source ? <div className="collection-member" key={member.policy_import_id}><Link href={`/policies/imports/${member.policy_import_id}/review?collection=${id}`}><div><strong>{source.title}</strong><span>revision {source.revision} · {source.candidate_count} candidates</span></div><StateBadge state={source.status} /></Link>{collection.status === "draft" && <PolicyCollectionMemberActions collectionId={id} importId={member.policy_import_id} />}</div> : <div key={member.policy_import_id} className="inline-api-error">Source unavailable</div>; })}</div></section>
    {(state?.collection_blockers.length || state?.blockers.length) ? <section className="panel blocker-list"><div className="section-header"><div><h2>Compilation blockers</h2><p>Resolve these without changing the evidence.</p></div></div>{state.collection_blockers.map((blocker) => <p key={blocker}>{blocker}</p>)}{state.blockers.map((source) => <div key={source.policy_import_id}><Link href={`/policies/imports/${source.policy_import_id}/review?collection=${id}`}>{source.title}</Link><span>{source.blockers.join(" · ")}</span></div>)}</section> : null}
  </div>;
}
