"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import type { SourceIngestionBatch, SourceIngestionItem } from "@/lib/types";
import { StateBadge } from "./ui";

export function IngestionBatchProgress({ initialBatch }: { initialBatch: SourceIngestionBatch }) {
  const [batch, setBatch] = useState(initialBatch);
  const [items, setItems] = useState<SourceIngestionItem[]>([]);
  useEffect(() => {
    let timer: number | undefined;
    let disposed = false;
    async function poll() {
      const response = await fetch(`/api/source-ingestion-batches/${batch.id}`, { cache: "no-store" });
      if (!response.ok || disposed) return;
      const payload = await response.json() as [SourceIngestionBatch, SourceIngestionItem[]];
      setBatch(payload[0]); setItems(payload[1]);
      if (!["complete", "partial", "failed"].includes(payload[0].status)) {
        timer = window.setTimeout(() => void poll(), 1800);
      }
    }
    void poll();
    return () => { disposed = true; if (timer) window.clearTimeout(timer); };
  }, [batch.id]);
  return <div className="batch-progress"><header><strong>Batch progress</strong><StateBadge state={batch.status} /><span>{batch.succeeded_count + batch.failed_count + batch.unchanged_count}/{batch.total_count}</span></header>{items.map((item) => <div key={item.id}><span>{item.client_item_key.slice(0, 12)}</span><StateBadge state={item.status} />{item.policy_import_id ? <Link href={`/policies/imports/${item.policy_import_id}${batch.policy_collection_id ? `/review?collection=${batch.policy_collection_id}` : ""}`}>Review source</Link> : <small>{item.failure_code?.replaceAll("_", " ")}</small>}</div>)}</div>;
}
