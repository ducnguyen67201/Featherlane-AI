"use client";

import { useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { FileText, LoaderCircle, Upload } from "lucide-react";
import { validatePolicySourceFile } from "@/lib/policy-import";

type InputMode = "file" | "text";

export function PolicyImportForm() {
  const router = useRouter();
  const fileInput = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<InputMode>("file");
  const [droppedFile, setDroppedFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit(form: HTMLFormElement) {
    setBusy(true);
    setError("");
    const data = new FormData(form);
    if (mode === "file") {
      data.delete("text");
      let file = data.get("file");
      if (!(file instanceof File) || file.size === 0) {
        file = droppedFile;
        if (file) data.set("file", file);
      }
      if (!(file instanceof File)) {
        setError("Choose a policy source file.");
        setBusy(false);
        return;
      }
      const validation = validatePolicySourceFile(file);
      if (validation) {
        setError(validation);
        setBusy(false);
        return;
      }
    } else {
      data.delete("file");
      const text = data.get("text");
      if (typeof text !== "string" || text.trim().length < 12) {
        setError("Paste the complete policy text before continuing.");
        setBusy(false);
        return;
      }
    }
    try {
      const effectiveFrom = data.get("effective_from");
      if (typeof effectiveFrom === "string" && effectiveFrom) {
        data.set("effective_from", new Date(effectiveFrom).toISOString());
      }
      const response = await fetch("/api/policy-imports", {
        method: "POST",
        headers: { "idempotency-key": crypto.randomUUID() },
        body: data,
      });
      const payload = (await response.json().catch(() => null)) as { id?: string; detail?: string } | null;
      if (!response.ok || !payload?.id) {
        throw new Error(payload?.detail ?? `Import failed (${response.status})`);
      }
      router.push(`/policies/imports/${payload.id}`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Policy import failed.");
      setBusy(false);
    }
  }

  return (
    <form
      className="panel import-form"
      onSubmit={(event) => {
        event.preventDefault();
        void submit(event.currentTarget);
      }}
    >
      <div className="import-tabs" role="tablist" aria-label="Policy source input">
        <button type="button" role="tab" aria-selected={mode === "file"} onClick={() => setMode("file")}>
          <Upload size={16} /> Upload a file
        </button>
        <button type="button" role="tab" aria-selected={mode === "text"} onClick={() => setMode("text")}>
          <FileText size={16} /> Paste policy text
        </button>
      </div>

      <div className="import-fields">
        {mode === "file" ? (
          <label
            className="file-drop"
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => {
              event.preventDefault();
              const file = event.dataTransfer.files[0];
              if (file) {
                setDroppedFile(file);
                setError(validatePolicySourceFile(file) ?? "");
              }
            }}
          >
            <Upload size={24} />
            <strong>Select PDF, DOCX, or TXT</strong>
            <span>Maximum 25 MiB. The source is retained as immutable evidence.</span>
            <input
              ref={fileInput}
              name="file"
              type="file"
              accept=".pdf,.docx,.txt,application/pdf,text/plain,application/vnd.openxmlformats-officedocument.wordprocessingml.document"
              onChange={(event) => {
                const file = event.target.files?.[0] ?? null;
                setDroppedFile(file);
                setError(file ? validatePolicySourceFile(file) ?? "" : "");
              }}
            />
            {droppedFile && <span className="selected-file">Selected: {droppedFile.name}</span>}
          </label>
        ) : (
          <label className="field field-wide">
            <span>Policy text</span>
            <textarea name="text" rows={12} required placeholder="Paste the complete internal policy, official guidance, or standard…" />
          </label>
        )}

        <label className="field">
          <span>Source title</span>
          <input name="title" required maxLength={240} placeholder="Customer refund approval policy" />
        </label>
        <label className="field">
          <span>Source type</span>
          <select name="source_type" defaultValue="company_policy" required>
            <option value="company_policy">Company policy</option>
            <option value="primary_law">Primary law</option>
            <option value="official_guidance">Official guidance</option>
            <option value="standard">Standard</option>
            <option value="expert_interpretation">Expert interpretation</option>
          </select>
        </label>
        <label className="field">
          <span>Jurisdiction</span>
          <input name="jurisdiction" required maxLength={120} placeholder="Internal, US, EU…" />
        </label>
        <label className="field">
          <span>Effective from <small>optional</small></span>
          <input name="effective_from" type="datetime-local" />
        </label>
        <label className="field field-wide">
          <span>Canonical source URL <small>optional</small></span>
          <input name="source_url" type="url" placeholder="https://intranet.example/policies/refunds" />
        </label>
      </div>

      <div className="form-footer">
        <p>Extraction creates review candidates only. Nothing becomes executable until a human verifies the source and approves every candidate.</p>
        <button className="primary-button" type="submit" disabled={busy}>
          {busy ? <LoaderCircle className="spin" size={16} /> : <Upload size={16} />}
          Import and extract
        </button>
      </div>
      {error && <div className="form-error" role="alert">{error}</div>}
    </form>
  );
}
