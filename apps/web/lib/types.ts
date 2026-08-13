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
  cost_usd: number | null;
  created_at: string;
  trace_quality: TraceQuality;
  findings: Finding[];
  timeline: TimelineItem[];
  summary: EvaluationSummary;
}

export interface EvaluationSummary {
  eval_run_id: string;
  verdict: Verdict;
  results: Array<Finding & { id: string; evidence_event_ids: string[] }>;
  passed: number;
  failed: number;
  inconclusive: number;
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
  issues: string[];
  checked_at: string;
  latest_trace_quality: TraceQuality | null;
  last_evaluated: string | null;
}

export type DriverType = "http_text" | "webhook";
export type TargetEnvironment = "staging" | "preview" | "sandbox";

export interface TargetManifest {
  schema_version: "1.0";
  target_id: string;
  target_version: string;
  driver_type: DriverType;
  endpoint: string;
  reset_endpoint: string | null;
  auth_secret_ref: string | null;
  timeout_seconds: number;
  evidence_mode: "inline";
  production_credentials_allowed: false;
}

export interface AgentTargetDetail extends AgentTarget {
  manifest: TargetManifest;
}

export interface CreateTargetInput {
  name: string;
  key: string;
  version: string;
  environment: TargetEnvironment;
  driver_type: DriverType;
  endpoint: string;
  reset_endpoint: string | null;
  auth_secret_ref: string | null;
  timeout_seconds: number;
}

export interface ScenarioDefinition {
  schema_version: "1.0";
  name: string;
  events: Array<
    | { type: "user_text"; text: string }
    | { type: "webhook"; payload: unknown }
    | { type: "system"; payload: unknown }
  >;
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
