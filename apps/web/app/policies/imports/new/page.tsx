import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { PolicyImportForm } from "@/components/policy-import-form";
import { PageHeader } from "@/components/ui";

export default function NewPolicyImportPage() {
  return (
    <div className="page">
      <Link className="back-link" href="/policies"><ArrowLeft size={14} /> Policy packs</Link>
      <PageHeader
        eyebrow="Policy source ingestion"
        title="Import a policy source"
        description="Upload the authoritative source or paste its text. Featherlane parses it in an isolated worker, extracts grounded policy candidates, and holds them for human review."
      />
      <PolicyImportForm />
    </div>
  );
}
