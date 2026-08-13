import { Database, Download, FileCheck2, LockKeyhole, RefreshCw } from "lucide-react";
import { getCorpus } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { MetricCard, PageHeader, StateBadge } from "@/components/ui";

export default async function CorpusPage() {
  const corpus = await getCorpus();
  return (
    <div className="page">
      <PageHeader
        eyebrow="Source lane / Regulatory corpus"
        title="Open US Law corpus"
        description="Pinned primary-law discovery data with checksum verification, provenance labels, and quarantine before rules can be published."
        action={<button className="primary-button"><RefreshCw size={16} />Sync manifest</button>}
      />
      <section className="metric-grid">
        <MetricCard label="Pinned snapshot" value={corpus.snapshot} detail={corpus.snapshot_date} />
        <MetricCard label="Parquet files" value={corpus.files.toString()} detail="Manifest entries" />
        <MetricCard label="Corpus size" value={formatBytes(corpus.total_bytes)} detail="Checksummed download" />
        <MetricCard label="Data license" value={corpus.license} detail="Attribution required" />
      </section>
      <section className="panel">
        <div className="section-header"><div><h2>Imported jurisdictions</h2><p>Discovery status is separate from source verification and policy approval</p></div><button className="secondary-button"><Download size={15} />Manifest</button></div>
        <div className="table-scroll">
          <table className="data-table">
            <thead><tr><th>Jurisdiction</th><th>Corpus type</th><th>Sections</th><th>Provenance confidence</th><th>Rule eligibility</th></tr></thead>
            <tbody>{corpus.imported_jurisdictions.map((item) => (
              <tr key={`${item.code}-${item.corpus_type}`}>
                <td><span className="jurisdiction"><Database size={15} />{item.code}</span></td>
                <td>{item.corpus_type}</td>
                <td className="mono">{item.sections.toLocaleString()}</td>
                <td><StateBadge state={item.confidence} /></td>
                <td><StateBadge state={item.status} /></td>
              </tr>
            ))}</tbody>
          </table>
        </div>
      </section>
      <section className="source-guardrails">
        <article><Download size={18} /><div><strong>Acquire</strong><p>Fetch only the pinned manifest and declared file URL.</p></div></article>
        <article><FileCheck2 size={18} /><div><strong>Verify</strong><p>Enforce byte length, SHA-256, and Parquet magic bytes.</p></div></article>
        <article><LockKeyhole size={18} /><div><strong>Quarantine</strong><p>Block known-noisy or unverified records from rule publication.</p></div></article>
      </section>
      <p className="certification-note">{corpus.attribution} Corpus records support discovery; reviewers must verify controlling sources before approving an executable rule.</p>
    </div>
  );
}
