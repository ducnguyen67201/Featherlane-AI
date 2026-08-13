import { NextResponse } from "next/server";
import { getCurrentSession } from "@/lib/session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function POST(
  _request: Request,
  context: { params: Promise<{ id: string }> },
) {
  const session = await getCurrentSession();
  if (!session) return NextResponse.json({ detail: "Authentication required." }, { status: 401 });

  const { id } = await context.params;
  try {
    const response = await fetch(`${API_URL}/v1/targets/${encodeURIComponent(id)}/validate`, {
      method: "POST",
      cache: "no-store",
    });
    return new NextResponse(await response.text(), {
      status: response.status,
      headers: { "content-type": response.headers.get("content-type") ?? "application/json" },
    });
  } catch {
    return NextResponse.json(
      { detail: "The governance API is unavailable; the target was not revalidated." },
      { status: 503 },
    );
  }
}
