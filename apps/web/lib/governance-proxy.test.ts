import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSession = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();

vi.mock("@/lib/session", () => ({ getCurrentSession }));

import { proxyGovernanceRequest } from "./governance-proxy";

describe("governance proxy", () => {
  beforeEach(() => {
    getCurrentSession.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    vi.stubEnv("GOVERNANCE_CONSOLE_API_KEY", "server-console-key");
  });

  it("rejects unauthenticated requests before reading their body", async () => {
    getCurrentSession.mockResolvedValue(null);
    const body = new ReadableStream();

    const response = await proxyGovernanceRequest(
      { method: "POST", headers: new Headers(), body } as unknown as Request,
      "/v1/evaluations",
    );

    expect(response.status).toBe(401);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("preserves request bytes and relevant headers", async () => {
    getCurrentSession.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response("created", {
      status: 201,
      headers: { "content-type": "text/plain", location: "/v1/policy-imports/import-1" },
    }));
    const contentType = "multipart/form-data; boundary=review-boundary";
    const request = new Request("http://localhost/api/policy-imports", {
      method: "POST",
      headers: { "content-type": contentType, "idempotency-key": "review-key" },
      body: "--review-boundary--",
    });

    const response = await proxyGovernanceRequest(request, "/v1/policy-imports");
    const [, init] = fetchMock.mock.calls[0];

    expect(init?.method).toBe("POST");
    expect(new Headers(init?.headers).get("content-type")).toBe(contentType);
    expect(new Headers(init?.headers).get("idempotency-key")).toBe("review-key");
    expect(init?.body).toBe(request.body);
    expect((init as RequestInit & { duplex?: string })?.duplex).toBe("half");
    expect(response.status).toBe(201);
    expect(response.headers.get("location")).toBe("/v1/policy-imports/import-1");
  });

  it("replaces spoofed internal identity and preserves safe response controls", async () => {
    getCurrentSession.mockResolvedValue({ user: { id: "user-1", email: "owner@example.com" } });
    fetchMock.mockResolvedValue(new Response("{}", {
      status: 429,
      headers: { "cache-control": "no-store", "retry-after": "12" },
    }));
    const request = new Request("http://localhost/api/source-connections", {
      headers: {
        "x-featherlane-console-key": "browser-spoof",
        "x-featherlane-actor-id": "attacker@example.com",
      },
    });

    const response = await proxyGovernanceRequest(request, "/v1/source-connections");
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);

    expect(headers.get("x-featherlane-console-key")).toBe("server-console-key");
    expect(headers.get("x-featherlane-actor-id")).toBe("owner@example.com");
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(response.headers.get("retry-after")).toBe("12");
  });

  it("reports an unknown mutation outcome when the upstream is unavailable", async () => {
    getCurrentSession.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));

    const response = await proxyGovernanceRequest(
      new Request("http://localhost/api/evaluations", { method: "POST", body: "{}" }),
      "/v1/evaluations",
    );

    expect(response.status).toBe(503);
    await expect(response.json()).resolves.toEqual({
      detail: "The governance API is unavailable. The request outcome is unknown; refresh before retrying.",
    });
  });

  it("reports that a failed read did not change policy data", async () => {
    getCurrentSession.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));

    const response = await proxyGovernanceRequest(
      new Request("http://localhost/api/policy-packs"),
      "/v1/policy-packs",
    );

    expect(response.status).toBe(503);
    await expect(response.json()).resolves.toEqual({
      detail: "The governance API is unavailable. No policy data was changed.",
    });
  });
});
