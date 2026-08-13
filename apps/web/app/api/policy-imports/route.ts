import { proxyGovernanceMultipart, proxyGovernanceRequest } from "@/lib/governance-proxy";

export function GET(request: Request) {
  const query = new URL(request.url).search;
  return proxyGovernanceRequest(request, `/v1/policy-imports${query}`);
}

export function POST(request: Request) {
  return proxyGovernanceMultipart(request, "/v1/policy-imports");
}
