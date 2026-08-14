"use client";

import { useMemo, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { Check, Clipboard, FlaskConical, LoaderCircle, RotateCcw } from "lucide-react";
import type { AgentTargetDetail, DriverType, Evaluation, ScenarioDefinition } from "@/lib/types";

type PolicyOption = { id: string; title: string };

export function buildQuickTestScenario(driverType: DriverType, input: string): ScenarioDefinition {
  if (driverType === "webhook") {
    let payload: unknown;
    try {
      payload = JSON.parse(input) as unknown;
    } catch {
      throw new Error("Webhook input must be valid JSON.");
    }
    return {
      schema_version: "1.0",
      name: "console quick test",
      events: [{ type: "webhook", payload }],
    };
  }
  return {
    schema_version: "1.0",
    name: "console quick test",
    events: [{ type: "user_text", text: input }],
  };
}

export function TargetActions({ target, policies }: { target: AgentTargetDetail; policies: PolicyOption[] }) {
  const router = useRouter();
  const isWebhook = target.manifest.driver_type === "webhook";
  const [input, setInput] = useState(() => isWebhook
    ? '{\n  "event": "test",\n  "message": "Run the connected workflow"\n}'
    : "Refund order test-456 for $700");
  const [policyId, setPolicyId] = useState(policies[0]?.id ?? "");
  const [busy, setBusy] = useState<"validate" | "run" | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const scenario = useMemo(() => {
    try {
      return buildQuickTestScenario(target.manifest.driver_type, input);
    } catch {
      return null;
    }
  }, [input, target.manifest.driver_type]);
  const scenarioPath = `.featherlane/${target.manifest.target_id}.scenario.json`;
  const command = `gov-eval run --target-id ${target.id} --policy-pack-id ${policyId || "<approved-policy-id>"} --scenario ${scenarioPath} --format junit --fail-on-inconclusive`;

  async function recheck() {
    setBusy("validate");
    setStatus(null);
    try {
      const response = await fetch(`/api/targets/${encodeURIComponent(target.id)}/validate`, { method: "POST" });
      const result = (await response.json()) as { detail?: string };
      if (!response.ok) throw new Error(result.detail ?? "Connection check failed.");
      setStatus("Connection report updated.");
      router.refresh();
    } catch (cause) {
      setStatus(cause instanceof Error ? cause.message : "Connection check failed.");
    } finally {
      setBusy(null);
    }
  }

  async function copy(value: string, label: string) {
    try {
      await navigator.clipboard.writeText(value);
      setStatus(`${label} copied.`);
    } catch {
      setStatus(`Clipboard unavailable. Select the ${label.toLowerCase()} text below.`);
    }
  }

  async function run(event: FormEvent) {
    event.preventDefault();
    if (!policyId) {
      setStatus("Approve a policy pack before running a test.");
      return;
    }
    if (!scenario) {
      setStatus("Webhook input must be valid JSON.");
      return;
    }
    setBusy("run");
    setStatus(null);
    try {
      const response = await fetch("/api/evaluations", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ target_id: target.id, policy_pack_id: policyId, scenario }),
      });
      const result = (await response.json()) as Evaluation | { detail?: string };
      if (!response.ok) throw new Error("detail" in result ? result.detail : "Evaluation failed to start.");
      router.push(`/evaluations/${(result as Evaluation).id}`);
    } catch (cause) {
      setStatus(cause instanceof Error ? cause.message : "Evaluation failed to start.");
      setBusy(null);
    }
  }

  return (
    <div className="target-action-grid">
      <section className="panel action-panel">
        <div className="section-header"><div><h2>Connection actions</h2><p>Probe from the Rust API or copy the safe manifest.</p></div></div>
        <div className="button-row">
          <button className="secondary-button" onClick={recheck} disabled={busy !== null}>{busy === "validate" ? <LoaderCircle className="spin" size={15} /> : <RotateCcw size={15} />}Recheck connection</button>
          <button className="secondary-button" onClick={() => copy(JSON.stringify(target.manifest, null, 2), "Manifest")}><Clipboard size={15} />Copy manifest</button>
        </div>
      </section>

      <form className="panel action-panel" onSubmit={run}>
        <div className="section-header"><div><h2>Quick test</h2><p>Run one {isWebhook ? "JSON webhook" : "text"} event against an approved policy.</p></div></div>
        <div className="action-fields">
          <label><span>Approved policy pack</span><select value={policyId} onChange={(event) => setPolicyId(event.target.value)} disabled={busy !== null}><option value="">Select a policy</option>{policies.map((policy) => <option key={policy.id} value={policy.id}>{policy.title}</option>)}</select></label>
          <label><span>{isWebhook ? "Webhook JSON" : "Message"}</span><textarea value={input} onChange={(event) => setInput(event.target.value)} maxLength={isWebhook ? 262144 : 32768} required disabled={busy !== null} /></label>
          <button className="primary-button" type="submit" disabled={busy !== null || !policyId}>{busy === "run" ? <LoaderCircle className="spin" size={15} /> : <FlaskConical size={15} />}{busy === "run" ? "Running…" : "Run test"}</button>
        </div>
      </form>

      <section className="panel action-panel ci-panel">
        <div className="section-header"><div><h2>CI setup</h2><p>Save this scenario as {scenarioPath}, commit it, and run the strict command.</p></div></div>
        <pre tabIndex={0}>{scenario ? JSON.stringify(scenario, null, 2) : "Enter valid webhook JSON to generate the scenario."}</pre>
        <pre tabIndex={0}>{command}</pre>
        <button className="secondary-button" onClick={() => copy(command, "CI command")}><Clipboard size={15} />Copy command</button>
      </section>
      {status && <p className="action-status" role="status"><Check size={14} />{status}</p>}
    </div>
  );
}
