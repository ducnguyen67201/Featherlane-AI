import Link from "next/link";
import { ArrowLeft } from "lucide-react";
import { EvaluationResult } from "@/components/evaluation-result";
import { getEvaluation } from "@/lib/api";

export default async function EvaluationDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const detail = await getEvaluation(id);
  if (!detail) {
    return <div className="page"><Link className="back-link" href="/evaluations"><ArrowLeft size={14} />All evaluations</Link><section className="panel"><h1>Evaluation unavailable</h1><p>The run was not found or the governance API is unavailable. No synthetic result was substituted.</p></section></div>;
  }
  return <EvaluationResult detail={detail} />;
}
