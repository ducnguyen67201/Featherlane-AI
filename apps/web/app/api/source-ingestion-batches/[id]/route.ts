import { proxyGovernanceRequest } from "@/lib/governance-proxy";

type RouteContext = { params: Promise<{ id: string }> };

export async function GET(request: Request, context: RouteContext) {
  const { id } = await context.params;
  return proxyGovernanceRequest(
    request,
    `/v1/source-ingestion-batches/${encodeURIComponent(id)}`,
  );
}
