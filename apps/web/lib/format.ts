import type { Verdict } from "./types";

export function formatDuration(milliseconds: number): string {
  return milliseconds < 1_000
    ? `${milliseconds} ms`
    : `${(milliseconds / 1_000).toFixed(1)} s`;
}

export function verdictLabel(verdict: Verdict): string {
  if (verdict === "INCONCLUSIVE") return "Inconclusive";
  return verdict[0] + verdict.slice(1).toLowerCase();
}

export function formatBytes(bytes: number): string {
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}
