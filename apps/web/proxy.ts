import { type NextRequest, NextResponse } from "next/server";
import { auth } from "@/lib/auth";
import { isPublicAuthPath, safeCallbackPath } from "@/lib/auth-routing";

const AUTH_REQUIRED_DETAIL = "Authentication required.";

export async function proxy(request: NextRequest) {
  const { pathname, search } = request.nextUrl;

  if (isPublicAuthPath(pathname) && pathname !== "/login") {
    return NextResponse.next();
  }

  const session = await auth.api.getSession({ headers: request.headers });

  if (pathname === "/login") {
    if (!session) return NextResponse.next();
    const callbackPath = safeCallbackPath(request.nextUrl.searchParams.get("callbackUrl"));
    return NextResponse.redirect(new URL(callbackPath, request.url));
  }

  if (session) return NextResponse.next();

  if (pathname.startsWith("/api/")) {
    return NextResponse.json({ detail: AUTH_REQUIRED_DETAIL }, { status: 401 });
  }

  const loginUrl = new URL("/login", request.url);
  loginUrl.searchParams.set("callbackUrl", safeCallbackPath(`${pathname}${search}`));
  return NextResponse.redirect(loginUrl);
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|.*\\.[^/]+$).*)"],
};
