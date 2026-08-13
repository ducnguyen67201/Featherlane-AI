import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();

vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

describe("evaluation BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("rejects an unauthorized request before reading or forwarding its body", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const text = vi.fn(async () => "sensitive body");

    const response = await POST({ text } as unknown as Request);

    expect(response.status).toBe(401);
    await expect(response.json()).resolves.toEqual({ detail: "Authentication required." });
    expect(text).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("forwards an authorized evaluation and preserves the upstream response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response("{\"id\":\"run-1\"}", {
      status: 202,
      headers: { "content-type": "application/json; charset=utf-8" },
    }));
    const body = "{\"target\":\"refund-agent-staging\"}";

    const response = await POST(new Request("http://localhost/api/evaluations", {
      method: "POST",
      body,
    }));

    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/evaluations",
      expect.objectContaining({
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
      }),
    );
    expect(response.status).toBe(202);
    expect(response.headers.get("content-type")).toBe("application/json; charset=utf-8");
    await expect(response.json()).resolves.toEqual({ id: "run-1" });
  });

  it("returns evaluation-specific JSON when the governance API is unavailable", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));

    const response = await POST(new Request("http://localhost/api/evaluations", {
      method: "POST",
      body: "{}",
    }));

    expect(response.status).toBe(503);
    await expect(response.json()).resolves.toEqual({
      detail: "The governance API is unavailable; no evaluation was created.",
    });
  });
});
