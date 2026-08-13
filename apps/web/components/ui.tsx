import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowRight, Info } from "lucide-react";
import type { Verdict } from "@/lib/types";
import { verdictLabel } from "@/lib/format";

export function PageHeader({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow?: string;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="page-header">
      <div>
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action && <div className="page-actions">{action}</div>}
    </div>
  );
}

export function VerdictBadge({ verdict }: { verdict: Verdict }) {
  return <span className={`status-badge ${verdict.toLowerCase()}`}>{verdictLabel(verdict)}</span>;
}

export function StateBadge({ state }: { state: string }) {
  const normalized = state.toLowerCase().replaceAll("_", "-");
  return <span className={`state-badge ${normalized}`}><span />{state.replaceAll("_", " ")}</span>;
}

export function MetricCard({
  label,
  value,
  detail,
  trend,
}: {
  label: string;
  value: string;
  detail: string;
  trend?: string;
}) {
  return (
    <article className="metric-card">
      <div className="metric-label">{label}<Info size={13} aria-hidden="true" /></div>
      <div className="metric-value">{value}</div>
      <div className="metric-detail">{detail}{trend && <span>{trend}</span>}</div>
    </article>
  );
}

export function SectionHeader({
  title,
  description,
  href,
  linkLabel,
}: {
  title: string;
  description: string;
  href?: string;
  linkLabel?: string;
}) {
  return (
    <div className="section-header">
      <div><h2>{title}</h2><p>{description}</p></div>
      {href && linkLabel && <Link href={href}>{linkLabel}<ArrowRight size={14} /></Link>}
    </div>
  );
}
