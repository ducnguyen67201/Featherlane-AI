import { proxyGovernancePost } from "@/lib/governance-proxy";

export async function POST(
  _request: Request,
  context: { params: Promise<{ id: string }> },
) {
  const { id } = await context.params;
  return proxyGovernancePost(
    `/v1/targets/${encodeURIComponent(id)}/validate`,
    "The governance API is unavailable; the target was not revalidated.",
  );
}
