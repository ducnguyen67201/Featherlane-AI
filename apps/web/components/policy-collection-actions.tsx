"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { CopyPlus, LoaderCircle, PackageCheck } from "lucide-react";

export function PolicyCollectionActions({ collectionId, version, title, ready, compiledPackId }: { collectionId: string; version: number; title: string; ready: boolean; compiledPackId: string | null }) {
  const router = useRouter(); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  if (compiledPackId) return <div className="collection-actions"><a className="primary-button" href={`/policies?pack=${compiledPackId}`}>View compiled pack</a><button className="secondary-button" disabled={busy} onClick={async () => { setBusy(true); setError(""); const response = await fetch(`/api/policy-collections/${collectionId}/clone`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ version: version + 1, title }) }); const payload = await response.json().catch(() => null) as { id?: string; detail?: string } | null; if (!response.ok || !payload?.id) { setError(payload?.detail ?? `Clone failed (${response.status})`); setBusy(false); return; } router.push(`/policies/collections/${payload.id}`); }}><CopyPlus size={15} /> Create version {version + 1}</button>{error && <span role="alert">{error}</span>}</div>;
  return <div className="collection-actions"><button className="primary-button" disabled={!ready || busy} onClick={async () => { setBusy(true); setError(""); const response = await fetch(`/api/policy-collections/${collectionId}/compile`, { method: "POST" }); const payload = await response.json().catch(() => null) as { detail?: string } | null; if (!response.ok) { setError(payload?.detail ?? `Compilation failed (${response.status})`); setBusy(false); return; } router.refresh(); }}>{busy ? <LoaderCircle className="spin" size={16} /> : <PackageCheck size={16} />} Compile draft pack</button>{error && <span role="alert">{error}</span>}</div>;
}
