"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { LoaderCircle, X } from "lucide-react";

export function PolicyCollectionMemberActions({ collectionId, importId }: { collectionId: string; importId: string }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function remove() {
    if (!window.confirm("Remove this exact source revision from the draft collection? The immutable import remains available.")) return;
    setBusy(true); setError("");
    const response = await fetch(`/api/policy-collections/${collectionId}/imports/${importId}`, { method: "DELETE" });
    if (!response.ok) {
      const payload = await response.json().catch(() => null) as { detail?: string } | null;
      setError(payload?.detail ?? `Removal failed (${response.status})`);
      setBusy(false);
      return;
    }
    router.refresh();
  }
  return <span className="member-actions"><button type="button" aria-label="Remove source revision" disabled={busy} onClick={() => void remove()}>{busy ? <LoaderCircle className="spin" size={14} /> : <X size={14} />} Remove</button>{error && <small role="alert">{error}</small>}</span>;
}
