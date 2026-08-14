import { proxyGovernanceRequest } from "@/lib/governance-proxy";

export async function PATCH(
  request: Request,
  context: { params: Promise<{ id: string }> },
) {
  const { id } = await context.params;
  return proxyGovernanceRequest(
    request,
    `/v1/targets/${encodeURIComponent(id)}/telemetry-boundary`,
    "PATCH",
  );
}
