"use client";

import { useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { LoaderCircle, Plus } from "lucide-react";

export function ImportPolicyButton() {
  const input = useRef<HTMLInputElement>(null);
  const router = useRouter();
  const [status, setStatus] = useState("");
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(false);

  async function importPolicy(file: File) {
    setLoading(true);
    setError(false);
    setStatus("Importing policy aggregate…");
    try {
      const body = await file.text();
      JSON.parse(body);
      const response = await fetch("/api/policy-packs", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
      });
      if (!response.ok) {
        const problem = (await response.json().catch(() => null)) as { detail?: string } | null;
        throw new Error(problem?.detail ?? `Import failed (${response.status})`);
      }
      setStatus("Draft saved in PostgreSQL");
      router.refresh();
    } catch (cause) {
      setError(true);
      setStatus(cause instanceof Error ? cause.message : "Policy import failed");
    } finally {
      setLoading(false);
      if (input.current) input.current.value = "";
    }
  }

  return (
    <div className="import-policy-action">
      <input
        ref={input}
        type="file"
        accept="application/json,.json"
        aria-label="Choose policy import JSON"
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) void importPolicy(file);
        }}
      />
      <button className="primary-button" type="button" onClick={() => input.current?.click()} disabled={loading}>
        {loading ? <LoaderCircle className="spin" size={16} /> : <Plus size={16} />}
        Import policy JSON
      </button>
      {status && <span className={`import-policy-status${error ? " error" : ""}`} role="status">{status}</span>}
    </div>
  );
}
