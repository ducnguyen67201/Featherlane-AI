import type { ActivityPoint } from "@/lib/types";

export function ActivityChart({ data }: { data: ActivityPoint[] }) {
  if (data.length === 0) {
    return <div className="policy-empty"><p>No evaluation activity has been persisted yet.</p></div>;
  }
  const max = Math.max(...data.map((point) => point.passed + point.failed + point.inconclusive), 1);
  return (
    <div className="chart-wrap">
      <div className="chart-legend" aria-label="Chart legend">
        <span><i className="pass-key" />Pass</span>
        <span><i className="fail-key" />Fail</span>
        <span><i className="inconclusive-key" />Inconclusive</span>
      </div>
      <div className="bar-chart" role="img" aria-label="Evaluation outcomes over the last seven days">
        {data.map((point) => {
          const total = point.passed + point.failed + point.inconclusive;
          const height = Math.max((total / max) * 100, 6);
          return (
            <div className="bar-column" key={point.day}>
              <div className="bar-value">{total}</div>
              <div className="bar-stack" style={{ height: `${height}%` }}>
                <span className="bar-pass" style={{ flex: point.passed }} />
                <span className="bar-fail" style={{ flex: point.failed || 0.08 }} />
                <span className="bar-inconclusive" style={{ flex: point.inconclusive || 0.08 }} />
              </div>
              <span className="bar-label">{point.day.replace("Aug ", "")}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
