import { NextResponse } from "next/server";
import { getCurrentSession } from "@/lib/session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function POST(request: Request) {
  const session = await getCurrentSession();
  if (!session) {
    return NextResponse.json({ detail: "Authentication required." }, { status: 401 });
  }

  const body = await request.text();
  try {
    const response = await fetch(`${API_URL}/v1/policy-packs`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body,
      cache: "no-store",
    });
    return new NextResponse(await response.text(), {
      status: response.status,
      headers: { "content-type": response.headers.get("content-type") ?? "application/json" },
    });
  } catch {
    return NextResponse.json(
      { detail: "The governance API is unavailable; no policy was persisted." },
      { status: 503 },
    );
  }
}
