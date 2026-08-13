import { proxyGovernancePost } from "@/lib/governance-proxy";

export async function POST(request: Request) {
  return proxyGovernancePost(
    "/v1/evaluations",
    "The governance API is unavailable; no evaluation was created.",
    request,
  );
}
