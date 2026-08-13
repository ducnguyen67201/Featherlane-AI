"use client";

import Link from "next/link";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { Ban, Check, ExternalLink, LoaderCircle, Play, Upload } from "lucide-react";

export function PolicyPackActions({ packId, status, sourceImportId, showSourceReview = true }: {
  packId: string;
  status: string;
  sourceImportId?: string;
  showSourceReview?: boolean;
}) {
  const router = useRouter();
  const [pendingAction, setPendingAction] = useState<"publish" | "disable" | "enable" | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function publish() {
    setPendingAction("publish");
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
      setPendingAction(null);
    }
  }

  async function changeAvailability(action: "disable" | "enable") {
    setPendingAction(action);
    setMessage(null);
    try {
      const response = await fetch(`/api/policy-packs/${encodeURIComponent(packId)}/${action}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          notes: action === "disable"
            ? "Disabled from the Featherlane policy console."
            : "Enabled from the Featherlane policy console.",
        }),
      });
      const payload = (await response.json().catch(() => null)) as { detail?: string } | null;
      if (!response.ok) {
        throw new Error(payload?.detail ?? `Policy ${action} failed (${response.status}).`);
      }
      setMessage(action === "disable"
        ? "Policy pack disabled; new evaluations cannot select it."
        : "Policy pack enabled and available for evaluations.");
      router.refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : `Policy ${action} failed.`);
    } finally {
      setPendingAction(null);
    }
  }

  return (
    <div className="policy-card-actions">
      {sourceImportId && showSourceReview && (
        <Link className="secondary-button" href={`/policies/imports/${sourceImportId}/review`}>
          <ExternalLink size={14} /> View source decisions
        </Link>
      )}
      {sourceImportId && (
        <Link className="secondary-button" href={`/policies/imports/new?replaces=${encodeURIComponent(sourceImportId)}`}>
          <Upload size={14} /> Upload new version
        </Link>
      )}
      {status === "draft" && (
        <button className="primary-button" type="button" disabled={pendingAction !== null} onClick={() => void publish()}>
          {pendingAction === "publish" ? <LoaderCircle className="spin" size={14} /> : <Check size={14} />}
          {pendingAction === "publish" ? "Publishing…" : "Approve & publish"}
        </button>
      )}
      {status === "approved" && (
        <>
          <span className="published-policy-state"><Check size={14} /> Enabled for evaluations</span>
          <button className="secondary-button" type="button" disabled={pendingAction !== null} onClick={() => void changeAvailability("disable")}>
            {pendingAction === "disable" ? <LoaderCircle className="spin" size={14} /> : <Ban size={14} />}
            {pendingAction === "disable" ? "Disabling…" : "Disable"}
          </button>
        </>
      )}
      {status === "disabled" && (
        <>
          <span className="disabled-policy-state"><Ban size={14} /> Disabled</span>
          <button className="primary-button" type="button" disabled={pendingAction !== null} onClick={() => void changeAvailability("enable")}>
            {pendingAction === "enable" ? <LoaderCircle className="spin" size={14} /> : <Play size={14} />}
            {pendingAction === "enable" ? "Enabling…" : "Enable"}
          </button>
        </>
      )}
      {message && <span className="policy-action-message" role="status">{message}</span>}
    </div>
  );
}
