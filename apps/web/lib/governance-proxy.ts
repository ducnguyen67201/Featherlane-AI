import { NextResponse } from "next/server";
import { getCurrentSession } from "@/lib/session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function proxyGovernancePost(
  path: string,
  unavailableDetail: string,
  request?: Request,
) {
  if (!await getCurrentSession()) {
    return NextResponse.json({ detail: "Authentication required." }, { status: 401 });
  }

  const body = request ? await request.text() : undefined;
  try {
    const response = await fetch(`${API_URL}${path}`, {
      method: "POST",
      ...(body === undefined ? {} : {
        headers: { "content-type": "application/json" },
        body,
      }),
      cache: "no-store",
    });
    return new NextResponse(await response.text(), {
      status: response.status,
      headers: { "content-type": response.headers.get("content-type") ?? "application/json" },
    });
  } catch {
    return NextResponse.json({ detail: unavailableDetail }, { status: 503 });
  }
}
