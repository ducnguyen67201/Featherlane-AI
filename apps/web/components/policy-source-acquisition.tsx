"use client";

import { useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { Cloud, FileText, Link2, LoaderCircle, Upload } from "lucide-react";
import { validatePolicySourceBatch, validateProviderSelectionIds, validatePublicPolicyUrls } from "@/lib/policy-source-batch";
import type { SourceConnection, SourceIngestionBatch } from "@/lib/types";
import { IngestionBatchProgress } from "./ingestion-batch-progress";

type Mode = "upload" | "paste" | "url" | "cloud";

export function PolicySourceAcquisition({ collectionId, connections }: { collectionId: string; connections: SourceConnection[] }) {
  const router = useRouter();
  const fileInput = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<Mode>("upload");
  const [files, setFiles] = useState<File[]>([]);
  const [batch, setBatch] = useState<SourceIngestionBatch | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  function selectFiles(selected: File[]) {
    const invalid = validatePolicySourceBatch(selected);
    if (invalid) return setError(invalid);
    setFiles(selected); setError("");
  }

  async function uploadBatch(sourceFiles: File[]) {
    if (!sourceFiles.length) throw new Error("Choose at least one policy source.");
    const form = new FormData();
    const items = sourceFiles.map((file, index) => ({ client_item_key: crypto.randomUUID(), title: file.name, source_url: null, index }));
    form.set("manifest", JSON.stringify({
      source_type: "company_policy",
      jurisdiction: "internal",
      items: items.map((item) => ({
        client_item_key: item.client_item_key,
        title: item.title,
        source_url: item.source_url,
      })),
    }));
    items.forEach((item) => form.set(`file:${item.client_item_key}`, sourceFiles[item.index]));
    const response = await fetch(`/api/policy-collections/${collectionId}/uploads`, { method: "POST", body: form });
    const payload = await response.json().catch(() => null) as SourceIngestionBatch & { detail?: string };
    if (!response.ok || !payload.id) throw new Error(payload.detail ?? `Upload failed (${response.status})`);
    setBatch(payload);
  }

  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError("");
    const data = new FormData(event.currentTarget);
    try {
      if (mode === "upload") await uploadBatch(files);
      if (mode === "paste") {
        const text = String(data.get("text") ?? "").trim();
        const title = String(data.get("title") ?? "").trim();
        if (text.length < 12 || !title) throw new Error("Provide a title and complete policy text.");
        const response = await fetch(`/api/policy-collections/${collectionId}/pastes`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ source_type: "company_policy", jurisdiction: "internal", items: [{ client_item_key: crypto.randomUUID(), title, text, source_url: null }] }),
        });
        const payload = await response.json().catch(() => null) as SourceIngestionBatch & { detail?: string };
        if (!response.ok || !payload.id) throw new Error(payload.detail ?? `Paste failed (${response.status})`);
        setBatch(payload);
      }
      if (mode === "url") {
        const { urls, error } = validatePublicPolicyUrls(String(data.get("urls") ?? ""));
        if (error) throw new Error(error);
        const response = await fetch(`/api/policy-collections/${collectionId}/urls`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ items: urls.map((url) => ({ client_item_key: crypto.randomUUID(), url })) }) });
        const payload = await response.json().catch(() => null) as SourceIngestionBatch & { detail?: string };
        if (!response.ok || !payload.id) throw new Error(payload.detail ?? `URL batch failed (${response.status})`);
        setBatch(payload);
      }
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Source ingestion failed."); }
    finally { setBusy(false); }
  }

  async function connect(provider: SourceConnection["provider"]) {
    setError("");
    const response = await fetch(`/api/source-connections/${provider}/authorize`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ collection_id: collectionId }) });
    const payload = await response.json().catch(() => null) as { authorization_url?: string; detail?: string } | null;
    if (!response.ok || !payload?.authorization_url) return setError(payload?.detail ?? "Provider authorization is unavailable.");
    window.location.assign(payload.authorization_url);
  }

  async function selectRemote(connection: SourceConnection, value: string) {
    const { ids, error } = validateProviderSelectionIds(connection.provider, value);
    if (error) return setError(error);
    setBusy(true); setError("");
    try {
      const response = await fetch(`/api/policy-collections/${collectionId}/selections`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          connection_id: connection.id,
          items: ids.map((external_item_id) => ({ client_item_key: crypto.randomUUID(), external_item_id })),
        }),
      });
      const payload = await response.json().catch(() => null) as SourceIngestionBatch & { detail?: string };
      if (!response.ok || !payload.id) throw new Error(payload.detail ?? `Selection failed (${response.status})`);
      setBatch(payload);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Provider selection failed.");
    } finally { setBusy(false); }
  }

  async function sync(connection: SourceConnection) {
    setBusy(true); setError("");
    try {
      const response = await fetch(`/api/source-connections/${connection.id}/sync`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ collection_id: collectionId }),
      });
      const payload = await response.json().catch(() => null) as SourceIngestionBatch & { detail?: string };
      if (!response.ok || !payload.id) throw new Error(payload.detail ?? `Sync failed (${response.status})`);
      setBatch(payload);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Provider sync failed.");
    } finally { setBusy(false); }
  }

  async function disconnect(connection: SourceConnection) {
    setBusy(true); setError("");
    try {
      const response = await fetch(`/api/source-connections/${connection.id}`, { method: "DELETE" });
      if (!response.ok) {
        const payload = await response.json().catch(() => null) as { detail?: string } | null;
        throw new Error(payload?.detail ?? `Disconnect failed (${response.status})`);
      }
      router.refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Provider disconnect failed.");
    } finally { setBusy(false); }
  }

  return (
    <section className="panel acquisition-panel">
      <div className="section-header"><div><h2>Add sources</h2><p>Every accepted item becomes immutable evidence and a separate review.</p></div></div>
      <div className="import-tabs" role="tablist" aria-label="Source acquisition method">
        {([ ["upload", Upload, "Files"], ["paste", FileText, "Paste"], ["url", Link2, "Public URLs"], ["cloud", Cloud, "Cloud"] ] as const).map(([value, Icon, label]) => <button key={value} type="button" role="tab" aria-selected={mode === value} onClick={() => setMode(value)}><Icon size={15} />{label}</button>)}
      </div>
      {mode !== "cloud" ? <form onSubmit={submit}>
        <div className="import-fields">
          {mode === "upload" && <label className="file-drop"><Upload size={24} /><strong>Select up to 25 PDF, DOCX, or TXT files</strong><span>25 MiB per file, 100 MiB total.</span><input ref={fileInput} type="file" multiple accept=".pdf,.docx,.txt" onChange={(event) => selectFiles(Array.from(event.target.files ?? []))} />{files.length > 0 && <span>{files.length} files · {(files.reduce((sum, file) => sum + file.size, 0) / 1024 / 1024).toFixed(1)} MiB</span>}</label>}
          {mode === "paste" && <><label className="field field-wide"><span>Source title</span><input name="title" maxLength={240} required /></label><label className="field field-wide"><span>Policy text</span><textarea name="text" rows={10} required /></label></>}
          {mode === "url" && <label className="field field-wide"><span>Public HTTPS URLs</span><textarea name="urls" rows={7} placeholder="https://example.com/policy.pdf" required /></label>}
        </div>
        <div className="form-footer"><p>URL requests block private networks, unsafe redirects, unsupported media, and oversized bodies.</p><button className="primary-button" disabled={busy}>{busy ? <LoaderCircle className="spin" size={16} /> : <Upload size={16} />} Start ingestion</button></div>
      </form> : <div className="connection-grid">
        {(["google_drive", "microsoft_graph", "notion"] as const).map((provider) => { const connection = connections.find((item) => item.provider === provider && item.status !== "disconnected"); return <article key={provider}><Cloud size={18} /><div><strong>{provider.replaceAll("_", " ")}</strong><span>{connection ? `${connection.display_label} · ${connection.status}` : "Explicit file/page selection only"}</span>{connection?.status === "active" && <RemoteSelection connection={connection} busy={busy} onSelect={selectRemote} onSync={sync} onDisconnect={disconnect} onError={setError} />}</div><button className="secondary-button" type="button" disabled={busy} onClick={() => void connect(provider)}>{connection ? "Reconnect" : "Connect"}</button></article>; })}
      </div>}
      {error && <div className="form-error" role="alert">{error}</div>}
      {batch && <IngestionBatchProgress initialBatch={batch} />}
    </section>
  );
}

function RemoteSelection({ connection, busy, onSelect, onSync, onDisconnect, onError }: { connection: SourceConnection; busy: boolean; onSelect: (connection: SourceConnection, ids: string) => Promise<void>; onSync: (connection: SourceConnection) => Promise<void>; onDisconnect: (connection: SourceConnection) => Promise<void>; onError: (message: string) => void }) {
  const [ids, setIds] = useState("");
  const [remoteItems, setRemoteItems] = useState<RemoteBrowseItem[]>([]);
  const [browsing, setBrowsing] = useState(false);
  const hint = connection.provider === "microsoft_graph" ? "drive-id:item-id" : connection.provider === "notion" ? "Notion page ID" : "Google Drive file ID";
  async function browse(driveId?: string, parentId?: string) {
    setBrowsing(true); onError("");
    try {
      const parameters = new URLSearchParams();
      if (driveId) parameters.set("drive_id", driveId);
      if (parentId) parameters.set("parent_id", parentId);
      const response = await fetch(`/api/source-connections/${connection.id}/browse?${parameters}`, { cache: "no-store" });
      const payload = await response.json().catch(() => null) as RemoteBrowseResponse & { detail?: string };
      if (!response.ok || !payload.items) onError(payload.detail ?? "Provider browser failed.");
      else setRemoteItems(payload.items);
    } catch {
      onError("Provider browser is unavailable.");
    } finally { setBrowsing(false); }
  }
  function addRemoteId(id: string) {
    const values = new Set(ids.split(/\r?\n/).map((value) => value.trim()).filter(Boolean));
    values.add(id); setIds(Array.from(values).join("\n"));
  }
  return <div className="remote-selection"><label><span>Selected item IDs</span><textarea aria-label={`${connection.provider} selected item IDs`} value={ids} onChange={(event) => setIds(event.target.value)} rows={3} placeholder={hint} /></label><div>{connection.provider === "google_drive" ? <button type="button" disabled={busy} onClick={() => void openGooglePicker(connection, onSelect).catch((cause) => onError(cause instanceof Error ? cause.message : "Google Picker failed."))}>Choose with Google Picker</button> : <button type="button" disabled={busy || browsing} onClick={() => void browse()}>{browsing ? "Loading…" : connection.provider === "notion" ? "Browse shared pages" : "Browse OneDrive"}</button>}<button type="button" disabled={busy || !ids.trim()} onClick={() => void onSelect(connection, ids)}>Import selected</button><button type="button" disabled={busy} onClick={() => void onSync(connection)}>Sync now</button><button type="button" disabled={busy} onClick={() => void onDisconnect(connection)}>Disconnect</button></div>{remoteItems.length > 0 && <ul>{remoteItems.map((item) => <li key={`${item.kind}:${item.id}`}><span>{item.name}</span>{item.kind === "file" ? <button type="button" onClick={() => addRemoteId(item.id)}>Add</button> : <button type="button" onClick={() => void browse(item.drive_id ?? item.id, item.kind === "folder" ? item.id : undefined)}>Open</button>}</li>)}</ul>}</div>;
}

type RemoteBrowseItem = { id: string; name: string; kind: "drive" | "folder" | "file"; drive_id: string | null };
type RemoteBrowseResponse = { items: RemoteBrowseItem[]; next_cursor: string | null };

async function openGooglePicker(connection: SourceConnection, onSelect: (connection: SourceConnection, ids: string) => Promise<void>) {
  const developerKey = process.env.NEXT_PUBLIC_GOOGLE_PICKER_API_KEY;
  const appId = process.env.NEXT_PUBLIC_GOOGLE_PICKER_APP_ID;
  if (!developerKey || !appId) throw new Error("Google Picker public configuration is unavailable.");
  const tokenResponse = await fetch(`/api/source-connections/${connection.id}/picker-token`, { cache: "no-store" });
  const token = await tokenResponse.json().catch(() => null) as { access_token?: string; detail?: string } | null;
  const accessToken = token?.access_token;
  if (!tokenResponse.ok || !accessToken) throw new Error(token?.detail ?? "Google Picker authorization is unavailable.");
  await loadGooglePickerScript();
  await new Promise<void>((resolve, reject) => {
    window.gapi?.load("picker", {
      callback: () => {
        const picker = window.google?.picker;
        if (!picker) return reject(new Error("Google Picker failed to initialize."));
        const view = new picker.DocsView(picker.ViewId.DOCS).setMimeTypes([
          "application/vnd.google-apps.document",
          "application/pdf",
          "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
          "text/plain",
        ].join(","));
        new picker.PickerBuilder()
          .enableFeature(picker.Feature.MULTISELECT_ENABLED)
          .setOAuthToken(accessToken)
          .setDeveloperKey(developerKey)
          .setAppId(appId)
          .addView(view)
          .setCallback((data) => {
            if (data.action === picker.Action.PICKED) {
              const ids = (data.docs ?? []).map((document) => document.id).filter(Boolean);
              void onSelect(connection, ids.join("\n")).finally(resolve);
            } else {
              resolve();
            }
          })
          .build()
          .setVisible(true);
      },
      onerror: () => reject(new Error("Google Picker failed to load.")),
    });
  });
}

function loadGooglePickerScript() {
  if (window.gapi) return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    const existing = document.getElementById("google-picker-api");
    if (existing) {
      existing.addEventListener("load", () => resolve(), { once: true });
      existing.addEventListener("error", () => reject(new Error("Google Picker failed to load.")), { once: true });
      return;
    }
    const script = document.createElement("script");
    script.id = "google-picker-api";
    script.src = "https://apis.google.com/js/api.js";
    script.async = true;
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("Google Picker failed to load."));
    document.head.append(script);
  });
}

type GooglePickerData = { action: string; docs?: Array<{ id: string }> };
type GooglePickerView = { setMimeTypes: (mimeTypes: string) => GooglePickerView };
type GooglePickerBuilder = {
  enableFeature: (feature: string) => GooglePickerBuilder;
  setOAuthToken: (token: string) => GooglePickerBuilder;
  setDeveloperKey: (key: string) => GooglePickerBuilder;
  setAppId: (appId: string) => GooglePickerBuilder;
  addView: (view: GooglePickerView) => GooglePickerBuilder;
  setCallback: (callback: (data: GooglePickerData) => void) => GooglePickerBuilder;
  build: () => { setVisible: (visible: boolean) => void };
};

declare global {
  interface Window {
    gapi?: { load: (name: string, options: { callback: () => void; onerror: () => void }) => void };
    google?: { picker: { Action: { PICKED: string }; Feature: { MULTISELECT_ENABLED: string }; ViewId: { DOCS: string }; DocsView: new (viewId: string) => GooglePickerView; PickerBuilder: new () => GooglePickerBuilder } };
  }
}
