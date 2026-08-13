import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { PolicyReviewWorkspace } from "@/components/policy-review-workspace";
import { getPolicyCandidates, getPolicyImport } from "@/lib/api";
import { PageHeader } from "@/components/ui";

export default async function PolicyReviewPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const [importResult, candidateResult] = await Promise.all([getPolicyImport(id), getPolicyCandidates(id)]);
  return (
    <div className="page">
      <Link className="back-link" href={`/policies/imports/${id}`}><ArrowLeft size={14} /> Import status</Link>
      <PageHeader
        eyebrow="Human review gate"
        title={importResult.data?.title ?? "Policy candidate review"}
        description="Verify source provenance, compare every candidate to its exact excerpt, correct the deterministic mapping, and record an accountable decision."
      />
      {importResult.data && candidateResult.data ? (
        <PolicyReviewWorkspace initialImport={importResult.data} initialCandidates={candidateResult.data} />
      ) : (
        <section className="panel policy-empty"><h2>Review workspace unavailable</h2><p>{importResult.error ?? candidateResult.error}</p></section>
      )}
    </div>
  );
}
