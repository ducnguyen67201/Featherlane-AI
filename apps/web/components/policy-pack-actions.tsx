"use client";

import Link from "next/link";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { Check, ExternalLink, LoaderCircle } from "lucide-react";

export function PolicyPackActions({ packId, status, sourceImportId }: {
  packId: string;
  status: string;
  sourceImportId?: string;
}) {
  const router = useRouter();
  const [publishing, setPublishing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function publish() {
    setPublishing(true);
    setMessage(null);
    try {
      const response = await fetch(`/api/policy-packs/${encodeURIComponent(packId)}/approve`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ notes: "Approved and published from the Featherlane policy console." }),
      });
      const payload = (await response.json().catch(() => null)) as { detail?: string } | null;
      if (!response.ok) {
        throw new Error(payload?.detail ?? `Policy publication failed (${response.status}).`);
      }
      setMessage("Policy pack approved and ready for evaluations.");
      router.refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Policy publication failed.");
    } finally {
      setPublishing(false);
    }
  }

  return (
    <div className="policy-card-actions">
      {sourceImportId && (
        <Link className="secondary-button" href={`/policies/imports/${sourceImportId}/review`}>
          <ExternalLink size={14} /> View source decisions
        </Link>
      )}
      {status === "draft" && (
        <button className="primary-button" type="button" disabled={publishing} onClick={() => void publish()}>
          {publishing ? <LoaderCircle className="spin" size={14} /> : <Check size={14} />}
          {publishing ? "Publishing…" : "Approve & publish"}
        </button>
      )}
      {status === "approved" && (
        <span className="published-policy-state"><Check size={14} /> Published for evaluations</span>
      )}
      {message && <span className="policy-action-message" role="status">{message}</span>}
    </div>
  );
}
