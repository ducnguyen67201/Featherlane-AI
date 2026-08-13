import "server-only";

import { NextResponse } from "next/server";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function proxyGovernanceRequest(request: Request, path: string, method = request.method) {
  try {
    const headers = new Headers();
    const contentType = request.headers.get("content-type");
    const idempotencyKey = request.headers.get("idempotency-key");
    if (contentType) headers.set("content-type", contentType);
    if (idempotencyKey) headers.set("idempotency-key", idempotencyKey);
    const body = method === "GET" || method === "HEAD" ? undefined : await request.arrayBuffer();
    const response = await fetch(`${API_URL}${path}`, {
      method,
      headers,
      body,
      cache: "no-store",
      signal: AbortSignal.timeout(30_000),
    });
    const responseHeaders = new Headers();
    responseHeaders.set("content-type", response.headers.get("content-type") ?? "application/json");
    const location = response.headers.get("location");
    if (location) responseHeaders.set("location", location);
    return new NextResponse(await response.arrayBuffer(), {
      status: response.status,
      headers: responseHeaders,
    });
  } catch {
    return NextResponse.json(
      { detail: "The governance API is unavailable; no policy data was changed." },
      { status: 503 },
    );
  }
}

export async function proxyGovernanceMultipart(request: Request, path: string) {
  try {
    const headers = new Headers();
    const idempotencyKey = request.headers.get("idempotency-key");
    if (idempotencyKey) headers.set("idempotency-key", idempotencyKey);
    const response = await fetch(`${API_URL}${path}`, {
      method: "POST",
      headers,
      body: await request.formData(),
      cache: "no-store",
      signal: AbortSignal.timeout(30_000),
    });
    const responseHeaders = new Headers();
    responseHeaders.set("content-type", response.headers.get("content-type") ?? "application/json");
    const location = response.headers.get("location");
    if (location) responseHeaders.set("location", location);
    return new NextResponse(await response.arrayBuffer(), {
      status: response.status,
      headers: responseHeaders,
    });
  } catch {
    return NextResponse.json(
      { detail: "The governance API is unavailable; no policy data was changed." },
      { status: 503 },
    );
  }
}
