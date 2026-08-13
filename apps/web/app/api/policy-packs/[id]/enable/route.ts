import { NextResponse } from "next/server";
import { getCurrentSession } from "@/lib/session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

type RouteContext = { params: Promise<{ id: string }> };

export async function POST(request: Request, context: RouteContext) {
  const session = await getCurrentSession();
  if (!session) {
    return NextResponse.json({ detail: "Authentication required." }, { status: 401 });
  }
  const { id } = await context.params;
  const supplied = (await request.json().catch(() => ({}))) as { notes?: unknown };
  const actorId = session.user.email?.trim() || session.user.id;
  try {
    const response = await fetch(`${API_URL}/v1/policy-packs/${encodeURIComponent(id)}/enable`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        actor_id: actorId,
        notes: typeof supplied.notes === "string" ? supplied.notes : "",
      }),
      cache: "no-store",
      signal: AbortSignal.timeout(30_000),
    });
    return new NextResponse(await response.arrayBuffer(), {
      status: response.status,
      headers: { "content-type": response.headers.get("content-type") ?? "application/json" },
    });
  } catch {
    return NextResponse.json(
      { detail: "The governance API is unavailable; the policy pack was not enabled." },
      { status: 503 },
    );
  }
}
