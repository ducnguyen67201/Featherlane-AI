"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { ArrowLeft, LoaderCircle, Plus } from "lucide-react";

export default function NewPolicyCollectionPage() {
  const router = useRouter();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <div className="page">
      <Link className="back-link" href="/policies"><ArrowLeft size={14} /> Policies</Link>
      <div className="page-header"><div><div className="eyebrow">Collection-first ingestion</div><h1>Create a policy collection</h1><p>Define the immutable pack identity, then gather and review its exact source revisions.</p></div></div>
      <form className="panel collection-create" onSubmit={async (event) => {
        event.preventDefault(); setBusy(true); setError("");
        const data = new FormData(event.currentTarget);
        const response = await fetch("/api/policy-collections", {
          method: "POST", headers: { "content-type": "application/json" },
          body: JSON.stringify({ key: data.get("key"), version: Number(data.get("version")), title: data.get("title"), idempotency_key: crypto.randomUUID() }),
        });
        const payload = await response.json().catch(() => null) as { id?: string; detail?: string } | null;
        if (!response.ok || !payload?.id) { setError(payload?.detail ?? `Creation failed (${response.status})`); setBusy(false); return; }
        router.push(`/policies/collections/${payload.id}`);
      }}>
        <div className="import-fields">
          <label className="field field-wide"><span>Collection title</span><input name="title" required maxLength={240} placeholder="Customer support governance" /></label>
          <label className="field"><span>Pack key</span><input name="key" required maxLength={120} pattern={"[A-Za-z0-9._\\-]+"} placeholder="customer-support" /></label>
          <label className="field"><span>Version</span><input name="version" type="number" min={1} defaultValue={1} required /></label>
        </div>
        <div className="form-footer"><p>Membership freezes when this collection compiles. Pack approval and agent attachment remain separate human actions.</p><button className="primary-button" disabled={busy}>{busy ? <LoaderCircle className="spin" size={16} /> : <Plus size={16} />} Create collection</button></div>
        {error && <div className="form-error" role="alert">{error}</div>}
      </form>
    </div>
  );
}
