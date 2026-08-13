import { CheckCircle2, Database, FileCheck2, GitBranch, ShieldCheck } from "lucide-react";
import { ImportPolicyButton } from "@/components/import-policy-button";
import { getPolicies, getPolicyPack } from "@/lib/api";
import { PageHeader, StateBadge } from "@/components/ui";

export default async function PoliciesPage() {
  const packs = await getPolicies();
  const details = await Promise.all(packs.map((pack) => getPolicyPack(pack.id)));
  const sourceCount = packs.reduce((total, pack) => total + pack.source_count, 0);
  const ruleCount = packs.reduce((total, pack) => total + pack.rules, 0);
  return (
    <div className="page">
      <PageHeader
        eyebrow="Policy lane / Executable controls"
        title="Policy packs"
        description="Review the translation from laws and internal policies into versioned, deterministic agent checks. Approval publishes rules; it does not take over runtime business approval."
        action={<ImportPolicyButton />}
      />

      <section className="policy-flow" aria-label="Policy ingestion lifecycle">
        <div><FileCheck2 size={18} /><span>Persisted sources</span><strong>{sourceCount} linked in PostgreSQL</strong></div>
        <i />
        <div><GitBranch size={18} /><span>Extracted obligations</span><strong>Human review required</strong></div>
        <i />
        <div><ShieldCheck size={18} /><span>Executable packs</span><strong>{ruleCount} database rules</strong></div>
      </section>

      {packs.length === 0 ? (
        <section className="panel policy-empty">
          <Database size={27} />
          <h2>No policy packs in PostgreSQL</h2>
          <p>Import a JSON policy aggregate to create a draft. Featherlane never loads executable policies from repository files or frontend seed data.</p>
        </section>
      ) : packs.map((pack, index) => {
        const detail = details[index];
        return (
          <section className="panel policy-card" key={pack.id}>
            <div className="policy-summary">
              <div className="policy-icon"><ShieldCheck size={23} /></div>
              <div><div className="eyebrow">{pack.key}</div><h2>{pack.title}</h2><p>Version {pack.version} · {pack.source_count} persisted sources · reviewer {pack.reviewer}</p></div>
              <StateBadge state={pack.status} />
            </div>
            <div className="rules-table">
              <div className="rules-head"><span>Rule</span><span>Source obligation</span><span>Deterministic checks</span><span>Severity</span></div>
              {detail?.rules.map((rule) => (
                <div className="rule-row" key={`${rule.id}:${rule.version}`}>
                  <span><CheckCircle2 size={15} />{rule.id.replaceAll("_", " ")}</span>
                  <code>{rule.obligation_key}</code>
                  <span>{rule.assertions.map((assertion) => assertion.kind.replaceAll("_", " ")).join(", ")}</span>
                  <StateBadge state={rule.severity} />
                </div>
              ))}
            </div>
          </section>
        );
      })}

      <div className="boundary-callout">
        <ShieldCheck size={18} />
        <div><strong>What human approval means here</strong><p>A policy owner approves the extracted rule before publication. During evaluation, Featherlane only observes approval events produced by the customer workflow—it does not grant, deny, or block those approvals.</p></div>
      </div>
    </div>
  );
}
