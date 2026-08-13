import { Download, Filter } from "lucide-react";
import { PageHeader, SectionHeader } from "@/components/ui";
import { RunEvaluationButton } from "@/components/run-evaluation-button";
import { RunTable } from "@/components/run-table";
import { getEvaluations } from "@/lib/api";

export default async function EvaluationsPage() {
  const runs = await getEvaluations();
  return (
    <div className="page">
      <PageHeader
        eyebrow="Evidence / Evaluations"
        title="Evaluation runs"
        description="CI-triggered and active test executions, with separate pass, fail, and inconclusive outcomes."
        action={<RunEvaluationButton />}
      />
      <section className="panel">
        <div className="table-toolbar">
          <SectionHeader title="All runs" description={`${runs.length} recent evaluation records`} />
          <div><button className="secondary-button"><Filter size={15} />Filter</button><button className="secondary-button"><Download size={15} />Export evidence</button></div>
        </div>
        <RunTable runs={runs} />
      </section>
    </div>
  );
}
