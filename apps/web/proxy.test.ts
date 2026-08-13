import { beforeEach, describe, expect, it, vi } from "vitest";
import { NextRequest } from "next/server";

const getSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());

vi.mock("@/lib/auth", () => ({
  auth: { api: { getSession: getSessionMock } },
}));

import { proxy } from "./proxy";

describe("console auth proxy", () => {
  beforeEach(() => {
    getSessionMock.mockReset();
    getSessionMock.mockResolvedValue(null);
  });

  it("redirects a signed-out page and preserves its local callback", async () => {
    const response = await proxy(new NextRequest("http://localhost:3000/evaluations?status=fail"));
    const location = response.headers.get("location");

    expect(response.status).toBe(307);
    expect(location).not.toBeNull();
    const redirect = new URL(location ?? "http://localhost:3000/");
    expect(redirect.pathname).toBe("/login");
    expect(redirect.searchParams.get("callbackUrl")).toBe("/evaluations?status=fail");
  });

  it("returns JSON 401 for a signed-out BFF request", async () => {
    const response = await proxy(new NextRequest("http://localhost:3000/api/policy-packs"));

    expect(response.status).toBe(401);
    await expect(response.json()).resolves.toEqual({ detail: "Authentication required." });
  });

  it("passes OAuth handler traffic without checking a session", async () => {
    const response = await proxy(
      new NextRequest("http://localhost:3000/api/auth/callback/google?code=example"),
    );

    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(getSessionMock).not.toHaveBeenCalled();
  });

  it("passes the login page for a signed-out user", async () => {
    const response = await proxy(new NextRequest("http://localhost:3000/login"));

    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(getSessionMock).toHaveBeenCalledOnce();
  });

  it("redirects a signed-in login visit to its safe callback", async () => {
    getSessionMock.mockResolvedValue({ user: { id: "user-1" } });

    const response = await proxy(
      new NextRequest("http://localhost:3000/login?callbackUrl=%2Fagents"),
    );

    expect(response.headers.get("location")).toBe("http://localhost:3000/agents");
  });

  it("passes protected traffic with a fully validated session", async () => {
    getSessionMock.mockResolvedValue({ user: { id: "user-1" } });

    const response = await proxy(new NextRequest("http://localhost:3000/policies"));

    expect(response.headers.get("x-middleware-next")).toBe("1");
    expect(getSessionMock).toHaveBeenCalledOnce();
  });
});
