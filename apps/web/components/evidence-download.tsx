"use client";

import { Braces } from "lucide-react";
import type { EvidenceBundle } from "@/lib/types";

export function EvidenceDownload({ evidence, runId }: { evidence: EvidenceBundle; runId: string }) {
  function download() {
    const url = URL.createObjectURL(new Blob([JSON.stringify(evidence, null, 2)], {
      type: "application/json",
    }));
    const link = document.createElement("a");
    link.href = url;
    link.download = `featherlane-evidence-${runId}.json`;
    link.click();
    URL.revokeObjectURL(url);
  }

  return <button className="secondary-button" type="button" onClick={download}><Braces size={15} />Download JSON</button>;
}
