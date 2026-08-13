import { proxyGovernanceRequest } from "@/lib/governance-proxy";

type RouteContext = { params: Promise<{ path: string[] }> };

async function upstreamPath(request: Request, context: RouteContext) {
  const { path } = await context.params;
  const encoded = path.map(encodeURIComponent).join("/");
  return `/v1/policy-imports/${encoded}${new URL(request.url).search}`;
}

export async function GET(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}

export async function POST(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}

export async function PATCH(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}
