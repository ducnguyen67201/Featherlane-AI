"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import type { EvaluationRunState } from "@/lib/types";

const terminal = new Set<EvaluationRunState>(["completed", "cancelled", "failed"]);

export function RunRefresh({ state }: { state: EvaluationRunState }) {
  const router = useRouter();
  useEffect(() => {
    if (terminal.has(state)) return;
    let cancelled = false;
    let delay = 1_000;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = () => {
      timer = setTimeout(() => {
        if (cancelled) return;
        router.refresh();
        delay = Math.min(delay * 2, 10_000);
        refresh();
      }, delay);
    };
    refresh();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [router, state]);
  return null;
}
