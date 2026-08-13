import { NextResponse } from "next/server";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function POST(request: Request) {
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
