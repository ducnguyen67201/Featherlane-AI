import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();

vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

describe("policy pack BFF", () => {
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

  it("forwards the exact authorized body and preserves the upstream response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response("created", {
      status: 201,
      headers: { "content-type": "text/plain" },
    }));
    const body = "{\"name\":\"policy\"}";

    const response = await POST(new Request("http://localhost/api/policy-packs", {
      method: "POST",
      body,
    }));

    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/policy-packs",
      expect.objectContaining({
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
      }),
    );
    expect(response.status).toBe(201);
    expect(response.headers.get("content-type")).toBe("text/plain");
    await expect(response.text()).resolves.toBe("created");
  });

  it("keeps the existing 503 response when the governance API is unavailable", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));

    const response = await POST(new Request("http://localhost/api/policy-packs", {
      method: "POST",
      body: "{}",
    }));

    expect(response.status).toBe(503);
    await expect(response.json()).resolves.toEqual({
      detail: "The governance API is unavailable; no policy was persisted.",
    });
  });
});
