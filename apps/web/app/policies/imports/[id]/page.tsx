import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowLeft } from "lucide-react";
import { PolicyImportProgress } from "@/components/policy-import-progress";
import { getPolicyImport } from "@/lib/api";
import { PageHeader } from "@/components/ui";

export default async function PolicyImportPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const result = await getPolicyImport(id);
  if (!result.data && !result.error) notFound();
  return (
    <div className="page">
      <Link className="back-link" href="/policies"><ArrowLeft size={14} /> Policy packs</Link>
      <PageHeader
        eyebrow="Policy source ingestion"
        title="Import processing"
        description="The original artifact, parser identity, extraction provenance, and review decisions remain linked for audit evidence."
      />
      {result.data ? <PolicyImportProgress initialImport={result.data} /> : (
        <section className="panel policy-empty"><h2>Import status unavailable</h2><p>{result.error}</p></section>
      )}
    </div>
  );
}
