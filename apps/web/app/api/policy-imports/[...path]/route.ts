import { NextResponse } from "next/server";
import { proxyGovernanceRequest } from "@/lib/governance-proxy";
import { getCurrentSession } from "@/lib/session";

type RouteContext = { params: Promise<{ path: string[] }> };

async function upstreamPath(request: Request, context: RouteContext) {
  const { path } = await context.params;
  const encoded = path.map(encodeURIComponent).join("/");
  return `/v1/policy-imports/${encoded}${new URL(request.url).search}`;
}

export async function GET(request: Request, context: RouteContext) {
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}

export async function POST(request: Request, context: RouteContext) {
  if (await isReviewerMutation(context, "POST")) {
    return proxyReviewerMutation(request, context);
  }
  return proxyGovernanceRequest(request, await upstreamPath(request, context));
}

export async function PATCH(request: Request, context: RouteContext) {
  return proxyReviewerMutation(request, context);
}

async function isReviewerMutation(context: RouteContext, method: "POST" | "PATCH") {
  const { path } = await context.params;
  return method === "PATCH"
    ? path.length === 3 && path[1] === "candidates"
    : path.length === 2 && (path[1] === "verify-source" || path[1] === "candidates");
}

async function proxyReviewerMutation(request: Request, context: RouteContext) {
  const session = await getCurrentSession();
  if (!session) {
    return NextResponse.json({ detail: "Authentication required." }, { status: 401 });
  }

  let payload: unknown;
  try {
    payload = await request.json();
  } catch {
    return NextResponse.json({ detail: "A JSON review payload is required." }, { status: 400 });
  }
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return NextResponse.json({ detail: "A JSON review object is required." }, { status: 400 });
  }

  const reviewerId = session.user.email?.trim() || session.user.id;
  const authenticatedRequest = new Request(request.url, {
    method: request.method,
    headers: request.headers,
    body: JSON.stringify({ ...payload, reviewer_id: reviewerId }),
  });
  return proxyGovernanceRequest(
    authenticatedRequest,
    await upstreamPath(authenticatedRequest, context),
  );
}
