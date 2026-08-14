import { NextResponse } from "next/server";
import { proxyGovernanceRequest } from "@/lib/governance-proxy";

type RouteContext = { params: Promise<{ path: string[] }> };

async function upstreamPath(request: Request, context: RouteContext) {
  const { path } = await context.params;
  return `/v1/source-connections/${path.map(encodeURIComponent).join("/")}${new URL(request.url).search}`;
}

export async function GET(request: Request, context: RouteContext) {
  const { path } = await context.params;
  const response = await proxyGovernanceRequest(request, await upstreamPath(request, context));
  if (path.at(-1) !== "callback") return response;
  if (!response.ok) {
    return NextResponse.redirect(new URL("/policies?connector_error=authorization_failed", request.url), { headers: { "cache-control": "no-store" } });
  }
  const payload = await response.json().catch(() => null) as { redirect?: string } | null;
  const redirect = payload?.redirect;
  if (!redirect || !redirect.startsWith("/policies") || redirect.startsWith("//")) {
    return NextResponse.redirect(new URL("/policies?connector_error=invalid_redirect", request.url), { headers: { "cache-control": "no-store" } });
  }
  return NextResponse.redirect(new URL(redirect, request.url), { headers: { "cache-control": "no-store" } });
}

export async function POST(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}

export async function DELETE(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}
