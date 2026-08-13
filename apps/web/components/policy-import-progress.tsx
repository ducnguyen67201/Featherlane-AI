"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { AlertTriangle, CheckCircle2, LoaderCircle, RotateCcw } from "lucide-react";
import { importProgress, isActiveImportStatus } from "@/lib/policy-import";
import type { PolicyImport } from "@/lib/types";
import { StateBadge } from "./ui";

export function PolicyImportProgress({ initialImport }: { initialImport: PolicyImport }) {
  const router = useRouter();
  const [policyImport, setPolicyImport] = useState(initialImport);
  const [error, setError] = useState("");
  const [retrying, setRetrying] = useState(false);
  const [visibilityVersion, setVisibilityVersion] = useState(0);
  const [pollVersion, setPollVersion] = useState(0);

  useEffect(() => {
    const handleVisibility = () => setVisibilityVersion((version) => version + 1);
    document.addEventListener("visibilitychange", handleVisibility);
    return () => document.removeEventListener("visibilitychange", handleVisibility);
  }, []);

  useEffect(() => {
    if (!isActiveImportStatus(policyImport.status) || document.hidden) return;
    const controller = new AbortController();
    const timer = window.setTimeout(async () => {
      try {
        const response = await fetch(`/api/policy-imports/${policyImport.id}`, {
          cache: "no-store",
          signal: controller.signal,
        });
        if (!response.ok) throw new Error(`Status refresh failed (${response.status})`);
        setPolicyImport((await response.json()) as PolicyImport);
        setError("");
      } catch (cause) {
        if (!controller.signal.aborted) {
          setError(cause instanceof Error ? cause.message : "Status refresh failed.");
          setPollVersion((version) => version + 1);
        }
      }
    }, 2_000);
    return () => {
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [policyImport, visibilityVersion, pollVersion]);

  async function retry() {
    setRetrying(true);
    setError("");
    const response = await fetch(`/api/policy-imports/${policyImport.id}/retry`, { method: "POST" });
    const payload = (await response.json().catch(() => null)) as PolicyImport | { detail?: string } | null;
    if (!response.ok || !payload || !("status" in payload)) {
      setError(payload && "detail" in payload ? payload.detail ?? "Retry failed." : "Retry failed.");
      setRetrying(false);
      return;
    }
    setPolicyImport(payload);
    setRetrying(false);
  }

  const progress = importProgress(policyImport.status);
  const readyForReview = ["review_required", "ready_to_compile", "compiled"].includes(policyImport.status);
  const isFailure = policyImport.status.startsWith("failed") || policyImport.status === "needs_ocr";

  return (
    <>
      <section className="panel import-progress-panel" aria-live="polite">
        <div className="import-progress-head">
          <div className={`progress-icon${isFailure ? " failure" : ""}`}>
            {isActiveImportStatus(policyImport.status) ? <LoaderCircle className="spin" size={21} /> : isFailure ? <AlertTriangle size={21} /> : <CheckCircle2 size={21} />}
          </div>
          <div>
            <span className="eyebrow">Import status</span>
            <h2>{policyImport.title}</h2>
            <p>{statusMessage(policyImport)}</p>
          </div>
          <StateBadge state={policyImport.status} />
        </div>
        <div className="progress-track" aria-label={`${progress}% complete`}><span style={{ width: `${progress}%` }} /></div>
        <div className="progress-stages" aria-hidden="true"><span>Stored</span><span>Parsed</span><span>Extracted</span><span>Review</span></div>
        {policyImport.failure_detail && <div className="form-error">{policyImport.failure_detail}</div>}
        {error && <div className="form-error" role="alert">{error}</div>}
        <div className="progress-actions">
          {policyImport.status === "failed_retryable" && (
            <button className="secondary-button" type="button" onClick={() => void retry()} disabled={retrying}>
              <RotateCcw size={15} /> {retrying ? "Retrying…" : "Retry extraction"}
            </button>
          )}
          {(policyImport.status === "needs_ocr" || policyImport.status === "failed_terminal") && (
            <Link className="secondary-button" href="/policies/imports/new">Upload replacement source</Link>
          )}
          {readyForReview && policyImport.status !== "compiled" && (
            <Link className="primary-button" href={`/policies/imports/${policyImport.id}/review`}>Review candidates</Link>
          )}
          {policyImport.compiled_policy_pack_id && (
            <Link className="primary-button" href="/policies" onClick={() => router.refresh()}>View policy packs</Link>
          )}
        </div>
      </section>

      <section className="import-metadata-grid">
        <article><span>Source</span><strong>{policyImport.source_type.replaceAll("_", " ")} · revision {policyImport.revision}</strong><small>{policyImport.jurisdiction} · {policyImport.policy_source_id.slice(0, 12)}…</small></article>
        <article><span>Artifact</span><strong>{formatBytes(policyImport.byte_length)}</strong><small className="mono">{policyImport.content_sha256.slice(0, 14)}…</small></article>
        <article><span>Coverage</span><strong>{policyImport.coverage.processed_chunks}/{policyImport.coverage.total_chunks} chunks</strong><small>{policyImport.coverage.failed_chunks.length} failed</small></article>
        <article><span>Candidates</span><strong>{policyImport.candidate_count}</strong><small>{policyImport.model_name ?? "Waiting for extraction"}</small></article>
      </section>
    </>
  );
}

function statusMessage(policyImport: PolicyImport) {
  const messages: Record<PolicyImport["status"], string> = {
    uploading: "Securing the original source artifact.",
    queued: "Waiting for an isolated extraction worker.",
    parsing: "Parsing and normalizing source text.",
    extracting: "Extracting source-grounded policy candidates.",
    review_required: "Extraction is complete. A human must verify the source and review every candidate.",
    ready_to_compile: "All review gates passed. This import can be compiled into a draft policy pack.",
    compiled: "The approved candidates were compiled into a database-backed draft policy pack.",
    needs_ocr: "This PDF has no usable embedded text. Upload an OCR-processed copy.",
    failed_retryable: "Extraction encountered a temporary failure and can be retried safely.",
    failed_terminal: "This source could not be processed safely. Correct the source and start a new import.",
  };
  return messages[policyImport.status];
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
