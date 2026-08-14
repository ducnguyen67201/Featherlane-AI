"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { Check, Clipboard, KeyRound, LoaderCircle, Save, X } from "lucide-react";
import { parseAttributeList } from "@/components/connect-target-form";
import type {
  AgentTargetDetail,
  RotatedTelemetryIngestKey,
  TelemetryBoundaryConfig,
} from "@/lib/types";

type PolicyOption = { id: string; title: string };

function optional(value: FormDataEntryValue | null): string | null {
  const text = String(value ?? "").trim();
  return text.length ? text : null;
}

function optionalNumber(value: FormDataEntryValue | null): number | null {
  const text = optional(value);
  return text === null ? null : Number(text);
}

export function serializeTelemetrySetupForm(data: FormData): TelemetryBoundaryConfig {
  const enabled = data.get("auto_evaluation") === "on";
  return {
    boundary_kind: String(data.get("boundary_kind") ?? "workflow_execution") as TelemetryBoundaryConfig["boundary_kind"],
    external_id_attributes: parseAttributeList(String(data.get("external_id_attributes") ?? "featherlane.external_run.id")),
    terminal_attribute: optional(data.get("terminal_attribute")),
    default_policy_pack_id: enabled ? optional(data.get("default_policy_pack_id")) : null,
    settle_seconds: Number(data.get("settle_seconds") ?? 10),
    idle_timeout_seconds: optionalNumber(data.get("idle_timeout_seconds")),
    max_duration_seconds: optionalNumber(data.get("max_duration_seconds")),
    conversation_id_is_task_boundary: data.get("conversation_id_is_task_boundary") === "on",
  };
}

export function telemetryEnvironmentSnippet(
  plaintext: string,
  endpoint = "http://localhost:4318/v1/traces",
): string {
  return [
    `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=${endpoint}`,
    "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=http/protobuf",
    `OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer ${plaintext}`,
  ].join("\n");
}

export function TelemetrySetup({
  target,
  policies,
}: {
  target: AgentTargetDetail;
  policies: PolicyOption[];
}) {
  const router = useRouter();
  const current = target.manifest.telemetry_boundary;
  const [enabled, setEnabled] = useState(current.default_policy_pack_id !== null);
  const [busy, setBusy] = useState<"save" | "key" | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [plaintext, setPlaintext] = useState<string | null>(null);
  const currentPolicyUnavailable = current.default_policy_pack_id !== null
    && !policies.some((policy) => policy.id === current.default_policy_pack_id);
  const publicEndpoint = process.env.NEXT_PUBLIC_GOVERNANCE_OTLP_HTTP_ENDPOINT
    ?? "http://localhost:4318/v1/traces";
  const snippet = plaintext ? telemetryEnvironmentSnippet(plaintext, publicEndpoint) : null;

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy("save");
    setStatus(null);
    try {
      const telemetryBoundary = serializeTelemetrySetupForm(new FormData(event.currentTarget));
      const response = await fetch(`/api/targets/${encodeURIComponent(target.id)}/telemetry-boundary`, {
        method: "PATCH",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ telemetry_boundary: telemetryBoundary }),
      });
      const result = (await response.json()) as AgentTargetDetail | { detail?: string };
      if (!response.ok) {
        throw new Error("detail" in result ? result.detail : "Trace settings could not be saved.");
      }
      setStatus(telemetryBoundary.default_policy_pack_id
        ? "Automatic evaluation settings saved. New sessions will use this policy."
        : "Automatic trace evaluation disabled.");
      router.refresh();
    } catch (cause) {
      setStatus(cause instanceof Error ? cause.message : "Trace settings could not be saved.");
    } finally {
      setBusy(null);
    }
  }

  async function rotateKey() {
    setBusy("key");
    setStatus(null);
    setPlaintext(null);
    try {
      const response = await fetch(`/api/targets/${encodeURIComponent(target.id)}/telemetry-key`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: "{}",
      });
      const result = (await response.json()) as RotatedTelemetryIngestKey | { detail?: string };
      if (!response.ok) {
        throw new Error("detail" in result ? result.detail : "Ingest key could not be generated.");
      }
      setPlaintext((result as RotatedTelemetryIngestKey).plaintext);
      setStatus("New target-scoped key generated. The previous active key is revoked.");
    } catch (cause) {
      setStatus(cause instanceof Error ? cause.message : "Ingest key could not be generated.");
    } finally {
      setBusy(null);
    }
  }

  async function copySnippet() {
    if (!snippet) return;
    try {
      await navigator.clipboard.writeText(snippet);
      setStatus("OTLP environment settings copied.");
    } catch {
      setStatus("Clipboard unavailable. Select the environment settings below.");
    }
  }

  return (
    <section className="panel telemetry-setup">
      <div className="section-header">
        <div><h2>Automatic trace evaluation</h2><p>Bind this agent to one policy, one session identifier, and one finished signal.</p></div>
        <span className={`auto-eval-state ${enabled ? "enabled" : "disabled"}`}>{enabled ? "Enabled" : "Not configured"}</span>
      </div>
      <form onSubmit={save}>
        <label className="toggle-field telemetry-toggle">
          <input name="auto_evaluation" type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} disabled={busy !== null} />
          <span><strong>Evaluate after the session finishes</strong><small>All traces with the same authenticated agent and external session ID become one evaluation.</small></span>
        </label>
        <div className="form-grid" aria-disabled={!enabled}>
          <label><span>Approved default policy</span><select name="default_policy_pack_id" defaultValue={current.default_policy_pack_id ?? ""} required={enabled} disabled={!enabled || busy !== null}><option value="">Select a policy</option>{currentPolicyUnavailable && <option value={current.default_policy_pack_id ?? ""}>Current policy unavailable</option>}{policies.map((policy) => <option key={policy.id} value={policy.id}>{policy.title}</option>)}</select></label>
          <label><span>Session boundary</span><select name="boundary_kind" defaultValue={current.boundary_kind === "explicit_ci" ? "workflow_execution" : current.boundary_kind} disabled={!enabled || busy !== null}><option value="workflow_execution">Workflow execution</option><option value="agent_task">Agent task</option><option value="voice_call">Voice call</option></select></label>
          <label className="span-two"><span>Session ID attributes</span><textarea name="external_id_attributes" defaultValue={current.external_id_attributes.join("\n")} required={enabled} disabled={!enabled || busy !== null} /><small>Ordered, one per line. Add the chosen attribute to every span or resource in the session.</small></label>
          <label className="span-two"><span>Finished boolean attribute</span><input name="terminal_attribute" defaultValue={current.terminal_attribute ?? "featherlane.run.terminal"} required={enabled} disabled={!enabled || busy !== null} /></label>
          <label><span>Settle window</span><input name="settle_seconds" type="number" min={0} max={300} defaultValue={current.settle_seconds} required={enabled} disabled={!enabled || busy !== null} /></label>
          <label><span>Idle timeout</span><input name="idle_timeout_seconds" type="number" min={1} max={86400} defaultValue={current.idle_timeout_seconds ?? 300} disabled={!enabled || busy !== null} /></label>
          <label><span>Maximum duration</span><input name="max_duration_seconds" type="number" min={1} max={86400} defaultValue={current.max_duration_seconds ?? 3600} disabled={!enabled || busy !== null} /></label>
          <label className="toggle-field compact"><input name="conversation_id_is_task_boundary" type="checkbox" defaultChecked={current.conversation_id_is_task_boundary} disabled={!enabled || busy !== null} /><span><strong>Conversation equals one task</strong><small>Opt in only for bounded conversations.</small></span></label>
        </div>
        <div className="telemetry-actions">
          <button className="primary-button" type="submit" disabled={busy !== null}>{busy === "save" ? <LoaderCircle className="spin" size={15} /> : <Save size={15} />}{busy === "save" ? "Saving…" : "Save trace settings"}</button>
          <button className="secondary-button" type="button" onClick={rotateKey} disabled={busy !== null || !target.auto_evaluation_enabled}>{busy === "key" ? <LoaderCircle className="spin" size={15} /> : <KeyRound size={15} />}{busy === "key" ? "Generating…" : "Generate ingest key"}</button>
          {!target.auto_evaluation_enabled && <small>Save enabled settings before generating a key.</small>}
        </div>
      </form>
      {snippet && (
        <div className="one-time-key" role="status">
          <div><KeyRound size={16} /><span><strong>Shown once</strong><small>Store this as a secret. Featherlane cannot display it again.</small></span></div>
          <pre tabIndex={0}>{snippet}</pre>
          <div className="button-row"><button className="secondary-button" type="button" onClick={copySnippet}><Clipboard size={14} />Copy environment settings</button><button className="secondary-button" type="button" onClick={() => setPlaintext(null)}><X size={14} />Clear</button></div>
        </div>
      )}
      {status && <p className="action-status telemetry-status" role="status"><Check size={14} />{status}</p>}
    </section>
  );
}
