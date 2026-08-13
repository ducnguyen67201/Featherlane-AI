import type {
  Corpus,
  DashboardSnapshot,
} from "./types";

export const overview: DashboardSnapshot = {
  active_agents: 0,
  policy_packs: 0,
  evaluations_30d: 0,
  pass_rate: 0,
  open_findings: 0,
  trace_coverage: 0,
  recent_runs: [],
  daily_activity: [],
};

export const corpus: Corpus = {
  set_name: "open-us-law",
  dataset: "Open US Law",
  snapshot: "v2026.07",
  snapshot_date: "2026-07-21",
  files: 105,
  total_bytes: 1_169_714_039,
  license: "CC BY 4.0",
  imported_jurisdictions: [
    { code: "US-CA", corpus_type: "statutes", sections: 98_664, confidence: "snapshot_official_provenance", status: "verification_required" },
    { code: "US-FED", corpus_type: "statutes", sections: 54_853, confidence: "official_verified", status: "verified" },
    { code: "US-GA", corpus_type: "statutes", sections: 28_154, confidence: "quarantined", status: "blocked" },
  ],
  attribution: "Structured US primary-law data from the Open US Law corpus by Vaquill AI, used under CC BY 4.0.",
};
