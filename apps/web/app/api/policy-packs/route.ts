import { proxyGovernanceRequest } from "@/lib/governance-proxy";

export function POST(request: Request) {
  return proxyGovernanceRequest(request, "/v1/policy-packs");
}
