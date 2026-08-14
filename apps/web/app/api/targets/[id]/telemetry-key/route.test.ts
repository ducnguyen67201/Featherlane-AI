import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();
vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

describe("target telemetry key BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("rejects an unauthenticated key request", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const response = await POST(new Request("http://localhost", { method: "POST", body: "{}" }), { params: Promise.resolve({ id: "target-1" }) });
    expect(response.status).toBe(401);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("encodes the target id and preserves the one-time response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response('{"plaintext":"flt_secret"}', { status: 201 }));
    const request = new Request("http://localhost", { method: "POST", body: "{}" });
    const response = await POST(request, { params: Promise.resolve({ id: "target/1" }) });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/targets/target%2F1/telemetry-key/rotate",
      expect.objectContaining({ method: "POST", body: request.body }),
    );
    expect(response.status).toBe(201);
    expect(await response.json()).toEqual({ plaintext: "flt_secret" });
  });

  it("returns 503 when the upstream is offline", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));
    const response = await POST(new Request("http://localhost", { method: "POST", body: "{}" }), { params: Promise.resolve({ id: "target-1" }) });
    expect(response.status).toBe(503);
  });
});
