export type WireVerdict = "pass" | "fail" | "inconclusive";
export type Verdict = "PASS" | "FAIL" | "INCONCLUSIVE";

export function displayVerdict(verdict: WireVerdict): Verdict {
  const display: Record<WireVerdict, Verdict> = {
    pass: "PASS",
    fail: "FAIL",
    inconclusive: "INCONCLUSIVE",
  };
  return display[verdict];
}
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
  verdict: Verdict | null;
  state?: EvaluationRunState;
  passed: number;
  failed: number;
  inconclusive: number;
  duration_ms: number;
  created_at: string;
  trace_count?: number;
  event_count?: number;
}

export type EvaluationRunState =
  | "created"
  | "collecting"
  | "settling"
  | "finalizing"
  | "evaluating"
  | "completed"
  | "cancelled"
  | "failed";

export interface EvaluationRun {
  id: string;
  organization_id: string;
  target_id: string;
  target_version: string;
  policy_pack_id: string;
  policy_pack_key: string;
  policy_pack_version: number;
  policy_content_sha256: string;
  scenario_id: string;
  rule_ids: string[];
  boundary_kind: "workflow_execution" | "agent_task" | "voice_call" | "explicit_ci";
  external_run_id: string | null;
  primary_invocation_id: string;
  state: EvaluationRunState;
  completion_reason: string | null;
  terminal_state: string | null;
  verdict: WireVerdict | null;
  trace_quality: TraceQuality | null;
  evidence_sha256: string | null;
  span_count: number;
  trace_count: number;
  event_count: number;
  created_at: string;
  updated_at: string;
  finalized_at: string | null;
  completed_at: string | null;
}

export interface RuleResult {
  rule_id: string;
  severity: string;
  status: string;
  message: string;
  evidence_event_ids: string[];
}

export interface EvaluationSummary {
  verdict: WireVerdict;
  results: RuleResult[];
  passed: number;
  failed: number;
  inconclusive: number;
}

export interface EvidenceEvent {
  id: string;
  sequence: number;
  event_type: string;
  name: string;
  actor: { actor_type: string; id: string };
}

export interface EvidenceBundle {
  trace_quality: TraceQuality;
  trace_defects: Array<{ code: string; message: string; blocking: boolean }>;
  events: EvidenceEvent[];
}

export interface EvaluationRunDetail {
  run: EvaluationRun;
  summary: EvaluationSummary | null;
  evidence: EvidenceBundle | null;
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

export type PolicyImportStatus =
  | "uploading"
  | "queued"
  | "parsing"
  | "extracting"
  | "review_required"
  | "ready_to_compile"
  | "compiled"
  | "needs_ocr"
  | "failed_retryable"
  | "failed_terminal";

export interface PolicyImportCoverage {
  total_chunks: number;
  processed_chunks: number;
  failed_chunks: string[];
  duplicate_candidates: number;
  warnings: string[];
}

export interface PolicyImport {
  id: string;
  policy_source_id: string;
  revision: number;
  supersedes_import_id: string | null;
  status: PolicyImportStatus;
  input_kind: "file" | "pasted_text";
  source_type: "primary_law" | "official_guidance" | "standard" | "company_policy" | "expert_interpretation";
  title: string;
  jurisdiction: string;
  effective_from: string | null;
  source_url: string | null;
  original_filename: string | null;
  detected_mime_type: string;
  byte_length: number;
  content_sha256: string;
  parser_kind: string | null;
  parser_version: string | null;
  model_provider: string | null;
  model_name: string | null;
  prompt_version: string | null;
  page_count: number | null;
  coverage: PolicyImportCoverage;
  candidate_count: number;
  verification_status: "pending" | "verified" | "rejected";
  verified_by: string | null;
  verification_notes: string | null;
  failure_code: string | null;
  failure_detail: string | null;
  compiled_policy_pack_id: string | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
}

export interface SourceLocator {
  page: number | null;
  page_end: number | null;
  section: string | null;
  paragraph_start: number | null;
  paragraph_end: number | null;
  source_url: string | null;
  excerpt_sha256: string;
}

export interface RuleSuggestion {
  trigger: Record<string, unknown>;
  assertions: Array<Record<string, unknown>>;
  evidence_required: string[];
}

export interface PolicyCandidate {
  id: string;
  policy_import_id: string;
  position: number;
  origin: "model" | "human";
  key: string;
  statement: string;
  locator: SourceLocator;
  source_excerpt: string;
  applicability: unknown;
  exceptions: string[];
  required_evidence: string[];
  suggested_severity: "critical" | "high" | "medium" | "advisory";
  suggested_rule: RuleSuggestion | null;
  mapping_status: "ready" | "manual_required" | "unsupported";
  model_confidence: number | null;
  status: "pending" | "approved" | "rejected";
  review: { reviewer_id: string; notes: string; reviewed_at: string } | null;
  updated_at: string;
}
