import { proxyGovernanceRequest } from "@/lib/governance-proxy";

export async function GET(request: Request) {
  return proxyGovernanceRequest(request, "/v1/source-connections");
}
