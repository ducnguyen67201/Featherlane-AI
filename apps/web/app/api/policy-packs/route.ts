import { proxyGovernancePost } from "@/lib/governance-proxy";

export async function POST(request: Request) {
  return proxyGovernancePost(
    "/v1/policy-packs",
    "The governance API is unavailable; no policy was persisted.",
    request,
  );
}
