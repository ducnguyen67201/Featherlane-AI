import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { PolicyImportForm } from "@/components/policy-import-form";
import { getPolicyImport } from "@/lib/api";
import { PageHeader } from "@/components/ui";

export default async function NewPolicyImportPage({ searchParams }: {
  searchParams: Promise<{ replaces?: string | string[] }>;
}) {
  const params = await searchParams;
  const replacementId = Array.isArray(params.replaces) ? params.replaces[0] : params.replaces;
  const replacementResult = replacementId ? await getPolicyImport(replacementId) : null;
  const replacement = replacementResult?.data ?? undefined;
  return (
    <div className="page">
      <Link className="back-link" href="/policies"><ArrowLeft size={14} /> Policy packs</Link>
      <PageHeader
        eyebrow={replacement ? "Policy source revision" : "Policy source ingestion"}
        title={replacement ? `Upload a new version of ${replacement.title}` : "Import a policy source"}
        description={replacement
          ? "Upload the changed document. Featherlane keeps the same source identity, creates a new immutable import revision, and extracts a new set of candidates for review."
          : "Upload the authoritative source or paste its text. Featherlane parses it in an isolated worker, extracts grounded policy candidates, and holds them for human review."}
      />
      {replacementId && !replacement ? (
        <section className="panel policy-empty"><h2>Source version unavailable</h2><p>{replacementResult?.error ?? "The source import could not be found."}</p></section>
      ) : (
        <PolicyImportForm replacement={replacement} />
      )}
    </div>
  );
}
