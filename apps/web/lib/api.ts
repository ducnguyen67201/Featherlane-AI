import type {
  AgentTarget,
  AgentTargetDetail,
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

async function getJsonRequired<T>(path: string): Promise<T> {
  await requireSession();
  const response = await fetch(`${API_URL}${path}`, {
    cache: "no-store",
    signal: AbortSignal.timeout(3_000),
  });
  if (!response.ok) {
    throw new Error(`Governance API request failed with status ${response.status}.`);
  }
  return (await response.json()) as T;
}

export function getOverview(): Promise<DashboardSnapshot> {
  return getJson("/v1/overview", seed.overview);
}

export function getEvaluations(): Promise<Evaluation[]> {
  return getJsonRequired("/v1/evaluations");
}

export async function getEvaluation(id: string): Promise<Evaluation | null> {
  await requireSession();
  const response = await fetch(`${API_URL}/v1/evaluations/${encodeURIComponent(id)}`, {
    cache: "no-store",
    signal: AbortSignal.timeout(3_000),
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Governance API request failed with status ${response.status}.`);
  return (await response.json()) as Evaluation;
}

export function getPolicies(): Promise<PolicyPack[]> {
  return getJson("/v1/policy-packs", seed.policies);
}

export function getPolicyPack(id: string): Promise<PolicyPackDetail | null> {
  return getJson(`/v1/policy-packs/${encodeURIComponent(id)}`, null);
}

export function getAgents(): Promise<AgentTarget[]> {
  return getJsonRequired("/v1/targets");
}

export async function getAgent(id: string): Promise<AgentTargetDetail | null> {
  await requireSession();
  const response = await fetch(`${API_URL}/v1/targets/${encodeURIComponent(id)}`, {
    cache: "no-store",
    signal: AbortSignal.timeout(3_000),
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Governance API request failed with status ${response.status}.`);
  return (await response.json()) as AgentTargetDetail;
}

export function getCorpus(setName: string): Promise<Corpus> {
  return getJson(`/v1/corpora/${encodeURIComponent(setName)}`, seed.corpus);
}
