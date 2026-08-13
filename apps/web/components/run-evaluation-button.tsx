"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { FlaskConical, LoaderCircle } from "lucide-react";

export function RunEvaluationButton({ policyPackId }: { policyPackId?: string }) {
  const router = useRouter();
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runEvaluation() {
    setRunning(true);
    setError(null);
    try {
      const response = await fetch("/api/evaluations", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          target_id: "refund-agent-staging",
          target_version: "unversioned",
          policy_pack_id: policyPackId,
          boundary_kind: "explicit_ci",
        }),
      });
      if (!response.ok) throw new Error("The local evaluation API is unavailable.");
      const result = (await response.json()) as { id: string };
      router.push(`/evaluations/${result.id}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Evaluation could not start.");
      setRunning(false);
    }
  }

  return (
    <div className="run-action">
      <button className="primary-button" onClick={runEvaluation} disabled={running || !policyPackId}>
        {running ? <LoaderCircle className="spin" size={16} /> : <FlaskConical size={16} />}
        {running ? "Starting…" : "Run evaluation"}
      </button>
      {!policyPackId && <span role="status">Approve a policy pack before starting a run.</span>}
      {error && <span role="alert">{error}</span>}
    </div>
  );
}
