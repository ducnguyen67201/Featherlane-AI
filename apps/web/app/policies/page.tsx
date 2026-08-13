import Link from "next/link";
import { ArrowRight, CheckCircle2, Database, FileCheck2, GitBranch, ShieldCheck } from "lucide-react";
import { ImportPolicyButton } from "@/components/import-policy-button";
import { PolicyPackActions } from "@/components/policy-pack-actions";
import { getPolicies, getPolicyImports, getPolicyPack } from "@/lib/api";
import { PageHeader, StateBadge } from "@/components/ui";

export default async function PoliciesPage() {
  const [policies, imports] = await Promise.all([getPolicies(), getPolicyImports()]);
  const packs = policies.data ?? [];
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

      <section className="panel">
        <div className="section-header"><div><h2>Recent source imports</h2><p>Live ingestion and human-review state from the governance database.</p></div></div>
        {imports.error ? (
          <div className="inline-api-error">{imports.error} Import state is never replaced with demonstration data.</div>
        ) : imports.data?.length ? (
          <div className="import-list">
            {imports.data.map((policyImport) => (
              <Link key={policyImport.id} href={`/policies/imports/${policyImport.id}`}>
                <div><strong>{policyImport.title}</strong><span>{policyImport.source_type.replaceAll("_", " ")} · revision {policyImport.revision} · {policyImport.candidate_count} candidates</span></div>
                <StateBadge state={policyImport.status} />
                <ArrowRight size={14} />
              </Link>
            ))}
          </div>
        ) : (
          <div className="inline-empty">No source imports yet. Upload a policy source to begin grounded extraction.</div>
        )}
      </section>

      {policies.error ? (
        <div className="inline-api-error">{policies.error} Policy packs are never replaced with demonstration data.</div>
      ) : null}

      {!policies.error && packs.length === 0 ? (
        <section className="panel policy-empty">
          <Database size={27} />
          <h2>No policy packs in PostgreSQL</h2>
          <p>Import a policy source, verify it, and approve grounded candidates to compile the first draft pack. Featherlane never loads executable policies from repository files or frontend seed data.</p>
        </section>
      ) : packs.map((pack, index) => {
        const detail = details[index];
        const sourceImport = imports.data?.find((policyImport) => policyImport.compiled_policy_pack_id === pack.id);
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
            <PolicyPackActions
              packId={pack.id}
              status={pack.status}
              sourceImportId={sourceImport?.id}
            />
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
