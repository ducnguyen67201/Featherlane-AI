"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Check, FileCheck2, LoaderCircle, Plus, ShieldCheck, X } from "lucide-react";
import type { PolicyCandidate, PolicyImport, RuleSuggestion } from "@/lib/types";
import { PolicyPackActions } from "./policy-pack-actions";
import { StateBadge } from "./ui";

const DEFAULT_RULE: RuleSuggestion = {
  trigger: { event_type: "final_output", name: null, attribute_equals: {}, numeric_argument: null },
  assertions: [{ kind: "max_count", matcher: { event_type: "final_output", name: null, attribute_equals: {}, numeric_argument: null }, count: 1 }],
  evidence_required: [],
};

export function PolicyReviewWorkspace({ initialImport, initialCandidates, reviewerIdentity, compiledPackStatus, eventTypes }: {
  initialImport: PolicyImport;
  initialCandidates: PolicyCandidate[];
  reviewerIdentity: string;
  compiledPackStatus: string | null;
  eventTypes: string[];
}) {
  const router = useRouter();
  const [policyImport, setPolicyImport] = useState(initialImport);
  const [candidates, setCandidates] = useState(initialCandidates);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [dirtyIds, setDirtyIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (dirtyIds.size > 0) event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirtyIds]);

  async function refreshImport() {
    const response = await fetch(`/api/policy-imports/${policyImport.id}`, { cache: "no-store" });
    if (response.ok) setPolicyImport((await response.json()) as PolicyImport);
  }

  async function verifySource(decision: "verified" | "rejected") {
    setBusy(true);
    const result = await mutate<PolicyImport>(`/api/policy-imports/${policyImport.id}/verify-source`, {
      decision,
      notes: decision === "verified" ? "Verified against the displayed source metadata and excerpts." : "Source provenance could not be verified.",
    });
    setBusy(false);
    if (result.data) setPolicyImport(result.data);
    setMessage(result.error ?? `Source marked ${decision}.`);
  }

  async function compile(form: HTMLFormElement) {
    setBusy(true);
    const formData = new FormData(form);
    const result = await mutate<{ id: string }>(`/api/policy-imports/${policyImport.id}/compile`, {
      key: String(formData.get("key") ?? ""),
      version: Number(formData.get("version") ?? 1),
      title: String(formData.get("title") ?? ""),
    }, "POST", { "idempotency-key": crypto.randomUUID() });
    setBusy(false);
    if (result.data) {
      setPolicyImport({ ...policyImport, status: "compiled", compiled_policy_pack_id: result.data.id });
      setMessage("Draft policy pack compiled and persisted.");
      router.refresh();
    } else {
      setMessage(result.error ?? "Compilation failed.");
    }
  }

  async function addManual(form: HTMLFormElement) {
    const formData = new FormData(form);
    const excerpt = String(formData.get("source_excerpt") ?? "").trim();
    const statement = String(formData.get("statement") ?? "").trim();
    if (!excerpt || !statement) return setMessage("A statement and exact source excerpt are required.");
    let suggestedRule: RuleSuggestion;
    try {
      suggestedRule = JSON.parse(String(formData.get("suggested_rule") ?? "")) as RuleSuggestion;
    } catch {
      return setMessage("The deterministic rule must be valid JSON.");
    }
    setBusy(true);
    const result = await mutate<PolicyCandidate>(`/api/policy-imports/${policyImport.id}/candidates`, {
      statement,
      source_excerpt: excerpt,
      locator: {
        page: nullableNumber(formData.get("page")),
        page_end: nullableNumber(formData.get("page")),
        section: nullableString(formData.get("section")),
        paragraph_start: null,
        paragraph_end: null,
        source_url: policyImport.source_url,
        excerpt_sha256: await sha256(excerpt),
      },
      applicability: {},
      exceptions: [],
      required_evidence: csv(formData.get("required_evidence")),
      suggested_severity: String(formData.get("severity") ?? "medium"),
      suggested_rule: suggestedRule,
    });
    setBusy(false);
    if (result.data) {
      setCandidates((current) => [...current, result.data!]);
      void refreshImport();
      setMessage("Manual candidate added as an approved, audited review decision.");
      form.reset();
      router.refresh();
    } else {
      setMessage(result.error ?? "Candidate could not be added.");
    }
  }

  const disposed = candidates.filter((candidate) => candidate.status !== "pending").length;
  const ready = policyImport.status === "ready_to_compile";
  const compiled = policyImport.status === "compiled";

  return (
    <>
      <section className={`review-toolbar panel${compiled ? " compiled" : ""}`}>
        <div><span>Signed-in reviewer</span><input value={reviewerIdentity} readOnly aria-label="Reviewer identity" /></div>
        <div><span>Source verification</span><StateBadge state={policyImport.verification_status} /></div>
        {!compiled && <button className="secondary-button" type="button" onClick={() => void verifySource("rejected")} disabled={busy}><X size={14} /> Reject source</button>}
        {!compiled && <button className="primary-button" type="button" onClick={() => void verifySource("verified")} disabled={busy}><FileCheck2 size={14} /> Verify source</button>}
      </section>

      <section className="review-summary">
        <article><span>Coverage</span><strong>{policyImport.coverage.processed_chunks}/{policyImport.coverage.total_chunks}</strong><small>{policyImport.coverage.failed_chunks.length ? "Failed chunks block compilation" : "All extracted chunks accounted for"}</small></article>
        <article><span>Candidate decisions</span><strong>{disposed}/{candidates.length}</strong><small>Every candidate must be disposed</small></article>
        <article><span>Compile gate</span><strong>{compiled ? "Compiled" : ready ? "Ready" : "Blocked"}</strong><small>{compiled ? "Candidate decisions are frozen" : ready ? "All evidence gates passed" : "Verify source and complete review"}</small></article>
      </section>

      {compiled && policyImport.compiled_policy_pack_id && (
        <section className="panel compiled-policy-action">
          <div><ShieldCheck size={20} /><div><strong>Candidate review is complete and immutable</strong><p>The resulting policy pack is {compiledPackStatus ?? "loading"}. Upload a new source version to propose policy changes without overwriting this evidence.</p></div></div>
          {compiledPackStatus ? (
            <PolicyPackActions
              packId={policyImport.compiled_policy_pack_id}
              status={compiledPackStatus}
              sourceImportId={policyImport.id}
              showSourceReview={false}
            />
          ) : (
            <span className="policy-action-message" role="status">Policy pack status is unavailable.</span>
          )}
        </section>
      )}

      {policyImport.coverage.warnings.length > 0 && (
        <div className="boundary-callout"><ShieldCheck size={18} /><div><strong>Extraction disclosure</strong><p>{policyImport.coverage.warnings.join(" ")}</p></div></div>
      )}

      <div className="candidate-stack">
        {candidates.map((candidate) => (
          <CandidateCard
            key={candidate.id}
            candidate={candidate}
            eventTypes={eventTypes}
            disabled={busy}
            readOnly={compiled}
            onBusy={setBusy}
            onDirty={() => setDirtyIds((current) => new Set(current).add(candidate.id))}
            onMessage={setMessage}
            onSaved={(saved) => {
              setCandidates((current) => current.map((item) => item.id === saved.id ? saved : item));
              setDirtyIds((current) => {
                const next = new Set(current);
                next.delete(saved.id);
                return next;
              });
              void refreshImport();
            }}
          />
        ))}
      </div>

      {!compiled && <details className="panel manual-candidate">
        <summary><Plus size={15} /> Add a policy candidate missed by extraction</summary>
        <form onSubmit={(event) => { event.preventDefault(); void addManual(event.currentTarget); }}>
          <label className="field field-wide"><span>Policy statement</span><textarea name="statement" rows={3} required /></label>
          <label className="field field-wide"><span>Exact source excerpt</span><textarea name="source_excerpt" rows={4} required /></label>
          <label className="field"><span>Page</span><input name="page" type="number" min="1" /></label>
          <label className="field"><span>Section</span><input name="section" /></label>
          <label className="field"><span>Severity</span><SeveritySelect /></label>
          <label className="field"><span>Required evidence</span><input name="required_evidence" placeholder="approval_decision, final_output" /></label>
          <label className="field field-wide"><span>Deterministic rule JSON</span><textarea className="mono" name="suggested_rule" rows={9} defaultValue={JSON.stringify(DEFAULT_RULE, null, 2)} required /></label>
          <button className="secondary-button" type="submit" disabled={busy}><Plus size={14} /> Add approved candidate</button>
        </form>
      </details>}

      {!compiled && <form className="panel compile-panel" onSubmit={(event) => { event.preventDefault(); void compile(event.currentTarget); }}>
        <div><ShieldCheck size={21} /><div><h2>Compile approved candidates</h2><p>Creates a database-backed draft pack. Publishing remains a separate policy-owner action.</p></div></div>
        <label className="field"><span>Pack key</span><input name="key" required defaultValue={slug(policyImport.title)} /></label>
        <label className="field"><span>Version</span><input name="version" type="number" min="1" required defaultValue={policyImport.revision} /></label>
        <label className="field"><span>Pack title</span><input name="title" required defaultValue={policyImport.title} /></label>
        <button className="primary-button" type="submit" disabled={!ready || busy || policyImport.status === "compiled"}>
          {busy ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />}
          {policyImport.status === "compiled" ? "Compiled" : "Compile draft pack"}
        </button>
      </form>}

      <div className="boundary-callout review-boundary"><ShieldCheck size={18} /><div><strong>Suggestions are not published until human review and pack approval.</strong><p>Compilation creates a draft only. A policy owner must still use the existing pack approval step before any evaluation can select it.</p></div></div>

      {message && <div className="review-message" role="status">{message}</div>}
    </>
  );
}

function CandidateCard({ candidate, eventTypes, disabled, readOnly, onBusy, onDirty, onMessage, onSaved }: {
  candidate: PolicyCandidate;
  eventTypes: string[];
  disabled: boolean;
  readOnly: boolean;
  onBusy: (busy: boolean) => void;
  onDirty: () => void;
  onMessage: (message: string) => void;
  onSaved: (candidate: PolicyCandidate) => void;
}) {
  const [statement, setStatement] = useState(candidate.statement);
  const [severity, setSeverity] = useState(candidate.suggested_severity);
  const [mappingStatus, setMappingStatus] = useState(candidate.mapping_status);
  const [evidence, setEvidence] = useState(candidate.required_evidence.join(", "));
  const [rule, setRule] = useState(JSON.stringify(candidate.suggested_rule, null, 2));
  const [notes, setNotes] = useState(candidate.review?.notes ?? "");
  const [triggerEvent, setTriggerEvent] = useState(String(candidate.suggested_rule?.trigger.event_type ?? "final_output"));
  const [assertionKind, setAssertionKind] = useState(String(candidate.suggested_rule?.assertions[0]?.kind ?? "max_count"));
  const [assertionEvent, setAssertionEvent] = useState(String(candidate.suggested_rule?.assertions[0]?.matcher && typeof candidate.suggested_rule.assertions[0].matcher === "object" ? (candidate.suggested_rule.assertions[0].matcher as Record<string, unknown>).event_type ?? "human_approval_decision" : "human_approval_decision"));
  const [sourceContext, setSourceContext] = useState("");
  const [contextError, setContextError] = useState("");

  async function loadSourceContext() {
    if (sourceContext) return;
    const response = await fetch(`/api/policy-imports/${candidate.policy_import_id}/source-context?candidate_id=${encodeURIComponent(candidate.id)}`, { cache: "no-store" });
    const payload = (await response.json().catch(() => null)) as { context?: string; detail?: string } | null;
    if (response.ok && payload?.context) setSourceContext(payload.context);
    else setContextError(payload?.detail ?? "Source context is unavailable.");
  }

  function applyTypedRule(nextTrigger: string, nextKind: string, nextAssertionEvent: string) {
    setTriggerEvent(nextTrigger);
    setAssertionKind(nextKind);
    setAssertionEvent(nextAssertionEvent);
    const matcher = { event_type: nextAssertionEvent, name: null, attribute_equals: {}, numeric_argument: null };
    const assertion = nextKind === "terminal_state"
      ? { kind: "terminal_state", state: "completed" }
      : nextKind === "max_count"
        ? { kind: "max_count", matcher, count: 1 }
        : { kind: nextKind, matcher };
    setRule(JSON.stringify({
      trigger: { event_type: nextTrigger, name: null, attribute_equals: {}, numeric_argument: null },
      assertions: [assertion],
      evidence_required: csv(evidence),
    }, null, 2));
    onDirty();
  }

  async function decide(decision: "approved" | "rejected") {
    let parsedRule: RuleSuggestion | null = null;
    if (rule.trim() && rule.trim() !== "null") {
      try {
        parsedRule = JSON.parse(rule) as RuleSuggestion;
      } catch {
        return onMessage(`Rule JSON for ${candidate.key} is invalid.`);
      }
    }
    onBusy(true);
    const result = await mutate<PolicyCandidate>(`/api/policy-imports/${candidate.policy_import_id}/candidates/${candidate.id}`, {
      decision,
      notes,
      expected_updated_at: candidate.updated_at,
      candidate: {
        statement,
        applicability: candidate.applicability,
        exceptions: candidate.exceptions,
        required_evidence: csv(evidence),
        suggested_severity: severity,
        suggested_rule: parsedRule,
        mapping_status: mappingStatus,
      },
    }, "PATCH");
    onBusy(false);
    if (result.data) {
      onSaved(result.data);
      onMessage(`${candidate.key} marked ${decision}.`);
    } else {
      onMessage(result.error ?? "Review decision failed.");
    }
  }

  return (
    <article className={`panel candidate-card${readOnly ? " read-only" : ""}`}>
      <header>
        <div><span className="eyebrow">{candidate.key} · {candidate.origin}</span><StateBadge state={candidate.status} /></div>
        <span>{candidate.model_confidence === null ? "Human-added" : `${Math.round(candidate.model_confidence * 100)}% extraction confidence`}</span>
      </header>
      <blockquote>{candidate.source_excerpt}</blockquote>
      <div className="candidate-locator">Page {candidate.locator.page ?? "—"} · Section {candidate.locator.section ?? "—"} · excerpt <code>{candidate.locator.excerpt_sha256.slice(0, 12)}…</code></div>
      <details className="source-context" onToggle={(event) => { if (event.currentTarget.open) void loadSourceContext(); }}>
        <summary>Show bounded source context</summary>
        {sourceContext ? <pre>{sourceContext}</pre> : <p>{contextError || "Loading source context…"}</p>}
      </details>
      <div className="candidate-fields">
        <label className="field field-wide"><span>Normalized policy statement</span><textarea rows={3} value={statement} readOnly={readOnly} onChange={(event) => { setStatement(event.target.value); onDirty(); }} /></label>
        <label className="field"><span>Severity</span><select value={severity} disabled={readOnly} onChange={(event) => { setSeverity(event.target.value as typeof severity); onDirty(); }}><option value="critical">Critical</option><option value="high">High</option><option value="medium">Medium</option><option value="advisory">Advisory</option></select></label>
        <label className="field"><span>Rule mapping</span><select value={mappingStatus} disabled={readOnly} onChange={(event) => { setMappingStatus(event.target.value as typeof mappingStatus); onDirty(); }}><option value="ready">Ready</option><option value="manual_required">Manual required</option><option value="unsupported">Unsupported</option></select></label>
        <label className="field field-wide"><span>Required evidence</span><input value={evidence} readOnly={readOnly} onChange={(event) => { setEvidence(event.target.value); onDirty(); }} /></label>
        <label className="field"><span>Trigger event</span><select value={triggerEvent} disabled={readOnly} onChange={(event) => applyTypedRule(event.target.value, assertionKind, assertionEvent)}>{eventTypes.map((eventType) => <option key={eventType} value={eventType}>{eventType.replaceAll("_", " ")}</option>)}</select></label>
        <label className="field"><span>Assertion</span><select value={assertionKind} disabled={readOnly} onChange={(event) => applyTypedRule(triggerEvent, event.target.value, assertionEvent)}><option value="exists_before">Exists before</option><option value="absent">Absent</option><option value="max_count">Maximum count: 1</option><option value="terminal_state">Terminal state: completed</option></select></label>
        {assertionKind !== "terminal_state" && <label className="field field-wide"><span>Assertion event</span><select value={assertionEvent} disabled={readOnly} onChange={(event) => applyTypedRule(triggerEvent, assertionKind, event.target.value)}>{eventTypes.map((eventType) => <option key={eventType} value={eventType}>{eventType.replaceAll("_", " ")}</option>)}</select></label>}
        <details className="field field-wide rule-json"><summary>Advanced deterministic rule JSON</summary><textarea className="mono" rows={8} value={rule} readOnly={readOnly} onChange={(event) => { setRule(event.target.value); onDirty(); }} /></details>
        <label className="field field-wide"><span>Review notes</span><input value={notes} readOnly={readOnly} onChange={(event) => { setNotes(event.target.value); onDirty(); }} placeholder="Why this decision is supportable" /></label>
      </div>
      <footer>
        <span className="candidate-save-state" role="status">
          {candidate.review
            ? `Saved as ${candidate.status} by ${candidate.review.reviewer_id}`
            : "Not reviewed yet"}
        </span>
        {!readOnly && <button className="secondary-button" type="button" disabled={disabled} onClick={() => void decide("rejected")}><X size={14} /> Reject</button>}
        {!readOnly && <button className="primary-button" type="button" disabled={disabled} onClick={() => void decide("approved")}><Check size={14} /> Approve</button>}
      </footer>
    </article>
  );
}

function SeveritySelect() {
  return <select name="severity" defaultValue="medium"><option value="critical">Critical</option><option value="high">High</option><option value="medium">Medium</option><option value="advisory">Advisory</option></select>;
}

async function mutate<T>(path: string, body: unknown, method = "POST", headers: Record<string, string> = {}): Promise<{ data: T | null; error: string | null }> {
  try {
    const response = await fetch(path, { method, headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body) });
    const payload = (await response.json().catch(() => null)) as T | { detail?: string } | null;
    if (!response.ok) return { data: null, error: payload && typeof payload === "object" && "detail" in payload ? payload.detail ?? "Request failed." : `Request failed (${response.status})` };
    return { data: payload as T, error: null };
  } catch {
    return { data: null, error: "The governance API is unavailable." };
  }
}

function csv(value: FormDataEntryValue | string | null) {
  return String(value ?? "").split(",").map((item) => item.trim()).filter(Boolean);
}

function nullableNumber(value: FormDataEntryValue | null) {
  const normalized = String(value ?? "").trim();
  return normalized ? Number(normalized) : null;
}

function nullableString(value: FormDataEntryValue | null) {
  const normalized = String(value ?? "").trim();
  return normalized || null;
}

async function sha256(value: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function slug(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 80) || "policy-pack";
}
