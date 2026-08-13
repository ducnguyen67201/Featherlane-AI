import Link from "next/link";
import { ChevronRight } from "lucide-react";
import { formatDuration } from "@/lib/format";
import type { RunListItem } from "@/lib/types";
import { VerdictBadge } from "./ui";

export function RunTable({ runs }: { runs: RunListItem[] }) {
  return (
    <div className="table-scroll">
      <table className="data-table">
        <thead><tr><th>Target</th><th>Verdict</th><th>Rule results</th><th>Duration</th><th>Started</th><th><span className="sr-only">Open</span></th></tr></thead>
        <tbody>
          {runs.map((run) => (
            <tr key={run.id}>
              <td><Link className="primary-cell" href={`/evaluations/${run.id}`}>{run.target}<small>{run.policy_pack}</small></Link></td>
              <td><VerdictBadge verdict={run.verdict} /></td>
              <td><div className="rule-counts"><span className="pass-text">{run.passed} pass</span><span className="fail-text">{run.failed} fail</span><span>{run.inconclusive} inc.</span></div></td>
              <td className="mono">{formatDuration(run.duration_ms)}</td>
              <td className="muted-cell">{new Date(run.created_at).toLocaleString("en", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" })}</td>
              <td><Link className="row-link" href={`/evaluations/${run.id}`} aria-label={`Open ${run.target} evaluation`}><ChevronRight size={16} /></Link></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
