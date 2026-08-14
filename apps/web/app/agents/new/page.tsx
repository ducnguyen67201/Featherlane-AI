import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { ConnectTargetForm } from "@/components/connect-target-form";
import { PageHeader } from "@/components/ui";
import { getPolicies } from "@/lib/api";

export default async function ConnectTargetPage() {
  const policies = await getPolicies();
  const approvedPolicies = (policies.data ?? [])
    .filter((policy) => policy.status === "approved")
    .map(({ id, title }) => ({ id, title }));
  return (
    <div className="page target-setup-page">
      <Link className="back-link" href="/agents"><ArrowLeft size={14} />Connected agents</Link>
      <PageHeader
        eyebrow="Execution lane / New target"
        title="Connect a test target"
        description="Point Featherlane at a non-production HTTP wrapper around any agent SDK or workflow engine."
      />
      <ConnectTargetForm policies={approvedPolicies} />
    </div>
  );
}
