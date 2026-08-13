import type {
  AgentTarget,
  Corpus,
  DashboardSnapshot,
  Evaluation,
  PolicyPack,
  PolicyPackDetail,
} from "./types";
import * as seed from "./seed";
import { requireSession } from "./session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

async function getJson<T>(path: string, fallback: T): Promise<T> {
  await requireSession();
  try {
    const response = await fetch(`${API_URL}${path}`, {
      cache: "no-store",
      signal: AbortSignal.timeout(1_500),
    });
    if (!response.ok) return fallback;
    return (await response.json()) as T;
  } catch {
    return fallback;
  }
}

export function getOverview(): Promise<DashboardSnapshot> {
  return getJson("/v1/overview", seed.overview);
}

export function getEvaluations(): Promise<Evaluation[]> {
  return getJson("/v1/evaluations", seed.evaluations);
}

export function getEvaluation(id: string): Promise<Evaluation> {
  const fallback = seed.evaluations.find((run) => run.id === id) ?? seed.evaluations[0];
  return getJson(`/v1/evaluations/${encodeURIComponent(id)}`, fallback);
}

export function getPolicies(): Promise<PolicyPack[]> {
  return getJson("/v1/policy-packs", seed.policies);
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
