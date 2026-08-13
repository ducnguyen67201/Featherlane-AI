import Link from "next/link";
import { ChevronRight } from "lucide-react";
import { formatDuration } from "@/lib/format";
import type { RunListItem } from "@/lib/types";
import { StateBadge, VerdictBadge } from "./ui";

export function RunTable({ runs }: { runs: RunListItem[] }) {
  return (
    <div className="table-scroll">
      <table className="data-table">
        <thead><tr><th>Target</th><th>Outcome</th><th>Evidence</th><th>Duration</th><th>Started</th><th><span className="sr-only">Open</span></th></tr></thead>
        <tbody>
          {runs.map((run) => (
            <tr key={run.id}>
              <td><Link className="primary-cell" href={`/evaluations/${run.id}`}>{run.target}<small>{run.policy_pack}</small></Link></td>
              <td>{run.verdict ? <VerdictBadge verdict={run.verdict} /> : <StateBadge state={run.state ?? "collecting"} />}</td>
              <td><div className="rule-counts">{run.trace_count === undefined ? <><span className="pass-text">{run.passed} pass</span><span className="fail-text">{run.failed} fail</span><span>{run.inconclusive} inc.</span></> : <><span>{run.trace_count} traces</span><span>{run.event_count} events</span></>}</div></td>
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
