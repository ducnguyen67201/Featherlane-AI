import {
  displayVerdict,
  type AgentTarget,
  type Corpus,
  type DashboardSnapshot,
  type EvaluationRun,
  type EvaluationRunDetail,
  type RunListItem,
  type PolicyPack,
  type PolicyPackDetail,
  type PolicyCandidate,
  type PolicyImport,
} from "./types";
import * as seed from "./seed";
import { requireSession } from "./session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

async function getJson<T>(path: string, fallback: T): Promise<T> {
  const result = await getLiveJson<T>(path);
  return result.data ?? fallback;
}

export function getOverview(): Promise<DashboardSnapshot> {
  return getJson("/v1/overview", seed.overview);
}

export async function getEvaluations(): Promise<RunListItem[]> {
  const runs = await getJson<EvaluationRun[]>("/v1/evaluations", []);
  return runs.map((run) => ({
    id: run.id,
    target: run.target_id,
    policy_pack: `${run.policy_pack_key} v${run.policy_pack_version}`,
    verdict: run.verdict ? displayVerdict(run.verdict) : null,
    state: run.state,
    passed: 0,
    failed: 0,
    inconclusive: 0,
    duration_ms: Math.max(0, new Date(run.completed_at ?? run.updated_at).getTime() - new Date(run.created_at).getTime()),
    created_at: run.created_at,
    trace_count: run.trace_count,
    event_count: run.event_count,
  }));
}

export function getEvaluation(id: string): Promise<EvaluationRunDetail | null> {
  return getJson(`/v1/evaluations/${encodeURIComponent(id)}`, null);
}

export function getEventTypes(): Promise<string[]> {
  return getJson("/v1/contracts/event-types", []);
}

export function getPolicies(): Promise<{ data: PolicyPack[] | null; error: string | null }> {
  return getLiveJson("/v1/policy-packs");
}

export function getPolicyPack(id: string): Promise<PolicyPackDetail | null> {
  return getJson(`/v1/policy-packs/${encodeURIComponent(id)}`, null);
}

export function getAgents(): Promise<AgentTarget[]> {
  return getJson("/v1/targets", seed.agents);
}

export function getCorpus(setName: string): Promise<Corpus> {
  return getJson(`/v1/corpora/${encodeURIComponent(setName)}`, seed.corpus);
}

async function getLiveJson<T>(path: string): Promise<{ data: T | null; error: string | null }> {
  await requireSession();
  try {
    const response = await fetch(`${API_URL}${path}`, {
      cache: "no-store",
      signal: AbortSignal.timeout(3_000),
    });
    if (!response.ok) {
      return { data: null, error: `Governance API returned ${response.status}` };
    }
    return { data: (await response.json()) as T, error: null };
  } catch {
    return { data: null, error: "The governance API is unavailable." };
  }
}

export function getPolicyImports(): Promise<{ data: PolicyImport[] | null; error: string | null }> {
  return getLiveJson("/v1/policy-imports?limit=25");
}

export function getPolicyImport(id: string): Promise<{ data: PolicyImport | null; error: string | null }> {
  return getLiveJson(`/v1/policy-imports/${encodeURIComponent(id)}`);
}

export function getPolicyCandidates(id: string): Promise<{ data: PolicyCandidate[] | null; error: string | null }> {
  return getLiveJson(`/v1/policy-imports/${encodeURIComponent(id)}/candidates`);
}
