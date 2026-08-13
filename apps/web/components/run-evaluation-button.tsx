"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { FlaskConical, LoaderCircle } from "lucide-react";

export function RunEvaluationButton() {
  const router = useRouter();
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runEvaluation() {
    setRunning(true);
    setError(null);
    try {
      const api = process.env.NEXT_PUBLIC_GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";
      const response = await fetch(`${api}/v1/evaluations`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ target: "refund-agent-staging" }),
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
      <button className="primary-button" onClick={runEvaluation} disabled={running}>
        {running ? <LoaderCircle className="spin" size={16} /> : <FlaskConical size={16} />}
        {running ? "Starting…" : "Run evaluation"}
      </button>
      {error && <span role="alert">{error}</span>}
    </div>
  );
}
