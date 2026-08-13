import { proxyGovernancePost } from "@/lib/governance-proxy";

export async function POST(request: Request) {
  return proxyGovernancePost(
    "/v1/targets",
    "The governance API is unavailable; no target was persisted.",
    request,
  );
}
