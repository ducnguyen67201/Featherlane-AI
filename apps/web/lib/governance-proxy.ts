import "server-only";

import { NextResponse } from "next/server";
import { getCurrentSession } from "./session";

const API_URL = process.env.GOVERNANCE_API_URL ?? "http://127.0.0.1:8080";

export async function proxyGovernanceRequest(request: Request, path: string, method = request.method) {
  const session = await getCurrentSession();
  if (!session) {
    return NextResponse.json({ detail: "Authentication required." }, { status: 401 });
  }
  try {
    const headers = new Headers();
    const contentType = request.headers.get("content-type");
    const idempotencyKey = request.headers.get("idempotency-key");
    if (contentType) headers.set("content-type", contentType);
    if (idempotencyKey) headers.set("idempotency-key", idempotencyKey);
    const consoleKey = process.env.GOVERNANCE_CONSOLE_API_KEY;
    if (consoleKey) headers.set("x-featherlane-console-key", consoleKey);
    headers.set(
      "x-featherlane-actor-id",
      session.user.email?.trim() || session.user.id,
    );
    const body = method === "GET" || method === "HEAD" ? undefined : request.body;
    const init: RequestInit & { duplex?: "half" } = {
      method,
      headers,
      body,
      cache: "no-store",
      signal: AbortSignal.timeout(30_000),
    };
    if (body) init.duplex = "half";
    const response = await fetch(`${API_URL}${path}`, init);
    const responseHeaders = new Headers();
    responseHeaders.set("content-type", response.headers.get("content-type") ?? "application/json");
    const location = response.headers.get("location");
    if (location) responseHeaders.set("location", location);
    const cacheControl = response.headers.get("cache-control");
    if (cacheControl) responseHeaders.set("cache-control", cacheControl);
    const retryAfter = response.headers.get("retry-after");
    if (retryAfter) responseHeaders.set("retry-after", retryAfter);
    return new NextResponse(await response.arrayBuffer(), {
      status: response.status,
      headers: responseHeaders,
    });
  } catch {
    const detail = method === "GET" || method === "HEAD"
      ? "The governance API is unavailable. No policy data was changed."
      : "The governance API is unavailable. The request outcome is unknown; refresh before retrying.";
    return NextResponse.json(
      { detail },
      { status: 503 },
    );
  }
}
