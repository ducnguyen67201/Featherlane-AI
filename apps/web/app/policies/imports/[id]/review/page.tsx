import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { PolicyReviewWorkspace } from "@/components/policy-review-workspace";
import { getEventTypes, getPolicyCandidates, getPolicyImport, getPolicyPack } from "@/lib/api";
import { requireSession } from "@/lib/session";
import { PageHeader } from "@/components/ui";

export default async function PolicyReviewPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const session = await requireSession();
  const [importResult, candidateResult, eventTypes] = await Promise.all([
    getPolicyImport(id),
    getPolicyCandidates(id),
    getEventTypes(),
  ]);
  const compiledPack = importResult.data?.compiled_policy_pack_id
    ? await getPolicyPack(importResult.data.compiled_policy_pack_id)
    : null;
  const isCompiled = importResult.data?.status === "compiled";
  const compiledDescription = compiledPack?.status === "approved"
    ? "These source decisions are frozen and the resulting policy pack is enabled for evaluations. Disable it to stop new evaluations, or upload a new source version to propose changes."
    : compiledPack?.status === "disabled"
      ? "These source decisions are frozen and the resulting policy pack is disabled. Re-enable it for evaluations, or upload a new source version to propose changes."
      : "These reviewed source decisions are frozen. Publish the resulting draft policy pack to make it available for evaluations, or upload a new source version to propose changes.";
  return (
    <div className="page">
      <Link className="back-link" href={`/policies/imports/${id}`}><ArrowLeft size={14} /> Import status</Link>
      <PageHeader
        eyebrow={isCompiled ? "Compiled source evidence" : "Human review gate"}
        title={importResult.data?.title ?? "Policy candidate review"}
        description={isCompiled
          ? compiledDescription
          : "Verify source provenance, compare every candidate to its exact excerpt, correct the deterministic mapping, and record an accountable decision."}
      />
      {importResult.data && candidateResult.data ? (
        <PolicyReviewWorkspace
          initialImport={importResult.data}
          initialCandidates={candidateResult.data}
          reviewerIdentity={session.user.email || session.user.id}
          compiledPackStatus={compiledPack?.status ?? null}
          eventTypes={eventTypes}
        />
      ) : (
        <section className="panel policy-empty"><h2>Review workspace unavailable</h2><p>{importResult.error ?? candidateResult.error}</p></section>
      )}
    </div>
  );
}
