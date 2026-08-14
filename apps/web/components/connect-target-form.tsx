"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { LoaderCircle, PlugZap } from "lucide-react";
import type { AgentTargetDetail, CreateTargetInput } from "@/lib/types";

type PolicyOption = { id: string; title: string };

function optional(value: FormDataEntryValue | null): string | null {
  const text = String(value ?? "").trim();
  return text.length ? text : null;
}

export function serializeTargetForm(data: FormData): CreateTargetInput {
  const autoEvaluation = data.get("auto_evaluation") === "on";
  return {
    name: String(data.get("name") ?? "").trim(),
    key: String(data.get("key") ?? "").trim(),
    version: String(data.get("version") ?? "").trim(),
    environment: String(data.get("environment")) as CreateTargetInput["environment"],
    driver_type: String(data.get("driver_type")) as CreateTargetInput["driver_type"],
    endpoint: String(data.get("endpoint") ?? "").trim(),
    reset_endpoint: optional(data.get("reset_endpoint")),
    auth_secret_ref: optional(data.get("auth_secret_ref")),
    timeout_seconds: Number(data.get("timeout_seconds") ?? 30),
    otlp_required: autoEvaluation,
    telemetry_boundary: {
      boundary_kind: String(data.get("boundary_kind") ?? "workflow_execution") as CreateTargetInput["telemetry_boundary"]["boundary_kind"],
      external_id_attributes: parseAttributeList(String(data.get("external_id_attributes") ?? "featherlane.external_run.id")),
      terminal_attribute: optional(data.get("terminal_attribute")),
      default_policy_pack_id: autoEvaluation ? optional(data.get("default_policy_pack_id")) : null,
      settle_seconds: Number(data.get("settle_seconds") ?? 10),
      idle_timeout_seconds: optionalNumber(data.get("idle_timeout_seconds")),
      max_duration_seconds: optionalNumber(data.get("max_duration_seconds")),
      conversation_id_is_task_boundary: data.get("conversation_id_is_task_boundary") === "on",
    },
  };
}

export function parseAttributeList(value: string): string[] {
  return [...new Set(value.split(/[\n,]/).map((item) => item.trim()).filter(Boolean))];
}

function optionalNumber(value: FormDataEntryValue | null): number | null {
  const text = optional(value);
  return text === null ? null : Number(text);
}

export function ConnectTargetForm({ policies }: { policies: PolicyOption[] }) {
  const router = useRouter();
  const [autoEvaluation, setAutoEvaluation] = useState(policies.length > 0);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const body = serializeTargetForm(new FormData(event.currentTarget));
      const response = await fetch("/api/targets", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      const result = (await response.json()) as AgentTargetDetail | { detail?: string };
      if (!response.ok) {
        throw new Error("detail" in result ? result.detail : "Target could not be saved.");
      }
      router.push(`/agents/${encodeURIComponent((result as AgentTargetDetail).id)}`);
      router.refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Target could not be saved.");
      setSubmitting(false);
    }
  }

  return (
    <form className="panel target-form" onSubmit={submit}>
      <fieldset disabled={submitting}>
        <legend>Identity</legend>
        <div className="form-grid">
          <label><span>Display name</span><input name="name" required maxLength={80} placeholder="Refund Agent" /></label>
          <label><span>Target key</span><input name="key" required pattern="[a-z0-9][a-z0-9-]{0,62}" placeholder="refund-agent-staging" /><small>Lowercase letters, numbers, and hyphens.</small></label>
          <label><span>Version</span><input name="version" required maxLength={120} placeholder="git:4e6a9c1" /></label>
          <label><span>Environment</span><select name="environment" defaultValue="staging"><option value="staging">Staging</option><option value="preview">Preview</option><option value="sandbox">Sandbox</option></select></label>
        </div>
      </fieldset>
      <fieldset disabled={submitting}>
        <legend>Connection</legend>
        <div className="form-grid">
          <label><span>Adapter</span><select name="driver_type" defaultValue="http_text"><option value="http_text">HTTP text</option><option value="webhook">Webhook</option></select></label>
          <label className="span-two"><span>Endpoint</span><input name="endpoint" type="url" required placeholder="http://refund-agent:8091/v1/messages" /><small>The Rust API service calls this URL; the browser never does.</small></label>
          <label className="span-two"><span>Reset endpoint <em>optional</em></span><input name="reset_endpoint" type="url" placeholder="http://sandbox:8090/v1/reset" /></label>
        </div>
      </fieldset>
      <fieldset disabled={submitting}>
        <legend>Security and runtime</legend>
        <div className="form-grid">
          <label className="span-two"><span>Bearer token environment-variable name <em>optional</em></span><input name="auth_secret_ref" pattern="[A-Z][A-Z0-9_]{0,127}" placeholder="REFUND_AGENT_TOKEN" /><small>Enter a server environment-variable name, never a secret value.</small></label>
          <label><span>Timeout (seconds)</span><input name="timeout_seconds" type="number" min={1} max={120} defaultValue={30} required /></label>
        </div>
      </fieldset>
      <fieldset disabled={submitting}>
        <legend>Automatic trace evaluation</legend>
        <label className="toggle-field">
          <input
            name="auto_evaluation"
            type="checkbox"
            checked={autoEvaluation}
            disabled={submitting || policies.length === 0}
            onChange={(event) => setAutoEvaluation(event.target.checked)}
          />
          <span><strong>Evaluate when a traced session ends</strong><small>The ingest key identifies this agent; the session and finished attributes decide when one evaluation is complete.</small></span>
        </label>
        {policies.length === 0 && <p className="form-note">Approve a policy pack before enabling automatic trace evaluation. You can connect the target now and configure traces later.</p>}
        <div className="form-grid" aria-disabled={!autoEvaluation}>
          <label><span>Approved default policy</span><select name="default_policy_pack_id" required={autoEvaluation} disabled={!autoEvaluation || submitting}><option value="">Select a policy</option>{policies.map((policy) => <option key={policy.id} value={policy.id}>{policy.title}</option>)}</select></label>
          <label><span>Session boundary</span><select name="boundary_kind" defaultValue="workflow_execution" disabled={!autoEvaluation || submitting}><option value="workflow_execution">Workflow execution</option><option value="agent_task">Agent task</option><option value="voice_call">Voice call</option></select></label>
          <label className="span-two"><span>Session ID attributes</span><textarea name="external_id_attributes" defaultValue="featherlane.external_run.id" required={autoEvaluation} disabled={!autoEvaluation || submitting} /><small>Ordered comma- or newline-separated span/resource attributes. The first non-empty value identifies the session.</small></label>
          <label className="span-two"><span>Finished boolean attribute</span><input name="terminal_attribute" defaultValue="featherlane.run.terminal" required={autoEvaluation} disabled={!autoEvaluation || submitting} /><small>Emit this attribute as true only after the complete agent task or workflow has ended.</small></label>
          <label><span>Settle window (seconds)</span><input name="settle_seconds" type="number" min={0} max={300} defaultValue={10} required={autoEvaluation} disabled={!autoEvaluation || submitting} /></label>
          <label><span>Idle timeout (seconds)</span><input name="idle_timeout_seconds" type="number" min={1} max={86400} defaultValue={300} disabled={!autoEvaluation || submitting} /></label>
          <label><span>Maximum duration (seconds)</span><input name="max_duration_seconds" type="number" min={1} max={86400} defaultValue={3600} disabled={!autoEvaluation || submitting} /></label>
          <label className="toggle-field compact"><input name="conversation_id_is_task_boundary" type="checkbox" disabled={!autoEvaluation || submitting} /><span><strong>Treat conversation ID as a task boundary</strong><small>Enable only when one conversation always equals one evaluation.</small></span></label>
        </div>
      </fieldset>
      <div className="form-submit">
        <button className="primary-button" type="submit" disabled={submitting}>
          {submitting ? <LoaderCircle className="spin" size={16} /> : <PlugZap size={16} />}
          {submitting ? "Saving and checking…" : "Save and test connection"}
        </button>
        <span role={error ? "alert" : "status"}>{error ?? "Unreachable targets are saved as degraded so you can inspect the configuration."}</span>
      </div>
    </form>
  );
}
