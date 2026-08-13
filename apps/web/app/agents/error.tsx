"use client";

import { AlertTriangle, RotateCcw } from "lucide-react";

export default function AgentsError({ reset }: { error: Error; reset: () => void }) {
  return (
    <div className="page">
      <section className="panel policy-empty" role="alert">
        <AlertTriangle size={27} />
        <h1>Targets could not be loaded</h1>
        <p>The governance API is unavailable. This is different from an empty target registry.</p>
        <button className="primary-button" onClick={reset}><RotateCcw size={16} />Try again</button>
      </section>
    </div>
  );
}
