export type Verdict = "PASS" | "FAIL" | "INCONCLUSIVE";
export type TraceQuality = "complete" | "degraded" | "insufficient";

export interface ActivityPoint {
  day: string;
  passed: number;
  failed: number;
  inconclusive: number;
}

export interface RunListItem {
  id: string;
  target: string;
  policy_pack: string;
  verdict: Verdict;
  passed: number;
  failed: number;
  inconclusive: number;
  duration_ms: number;
  created_at: string;
}

export interface DashboardSnapshot {
  active_agents: number;
  policy_packs: number;
  evaluations_30d: number;
  pass_rate: number;
  open_findings: number;
  trace_coverage: number;
  recent_runs: RunListItem[];
  daily_activity: ActivityPoint[];
}

export interface Finding {
  rule_id: string;
  severity: string;
  status: string;
  message: string;
}

export interface TimelineItem {
  sequence: number;
  event_type: string;
  name: string;
  actor: string;
  outcome: string;
}

export interface Evaluation {
  id: string;
  target: string;
  target_version: string;
  policy_pack: string;
  verdict: Verdict;
  passed: number;
  failed: number;
  inconclusive: number;
  duration_ms: number;
  cost_usd: number;
  created_at: string;
  trace_quality: TraceQuality;
  findings: Finding[];
  timeline: TimelineItem[];
}

export interface PolicyPack {
  id: string;
  key: string;
  title: string;
  version: number;
  status: string;
  rules: number;
  source_count: number;
  reviewer: string;
}

export interface PolicyRule {
  id: string;
  version: number;
  obligation_key: string;
  severity: string;
  assertions: Array<{ kind: string }>;
}

export interface PolicyPackDetail {
  id: string;
  organization_id: string;
  key: string;
  version: number;
  title: string;
  status: string;
  content_sha256: string;
  published_at: string | null;
  rules: PolicyRule[];
}

export interface AgentTarget {
  id: string;
  name: string;
  version: string;
  driver: string;
  environment: string;
  status: string;
  trace_coverage: number;
  last_evaluated: string;
}

export interface Jurisdiction {
  code: string;
  corpus_type: string;
  sections: number;
  confidence: string;
  status: string;
}

export interface Corpus {
  set_name: string;
  dataset: string;
  snapshot: string;
  snapshot_date: string;
  files: number;
  total_bytes: number;
  license: string;
  imported_jurisdictions: Jurisdiction[];
  attribution: string;
}
