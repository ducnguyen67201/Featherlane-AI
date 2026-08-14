import { proxyGovernanceRequest } from "@/lib/governance-proxy";

export async function GET(request: Request) {
  return proxyGovernanceRequest(request, `/v1/policy-collections${new URL(request.url).search}`);
}

export async function POST(request: Request) {
  return proxyGovernanceRequest(request, "/v1/policy-collections");
}
