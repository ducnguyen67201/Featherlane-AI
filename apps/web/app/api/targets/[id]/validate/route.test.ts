import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();
vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

describe("target validation BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("rejects an unauthenticated request", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const response = await POST(new Request("http://localhost"), { params: Promise.resolve({ id: "target/1" }) });
    expect(response.status).toBe(401);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("encodes the target id and preserves the response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response('{"status":"healthy"}', { status: 200 }));
    const response = await POST(new Request("http://localhost"), { params: Promise.resolve({ id: "target/1" }) });
    expect(fetchMock).toHaveBeenCalledWith("http://127.0.0.1:8080/v1/targets/target%2F1/validate", expect.objectContaining({ method: "POST" }));
    expect(response.status).toBe(200);
  });

  it("returns 503 when the upstream is offline", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));
    const response = await POST(new Request("http://localhost"), { params: Promise.resolve({ id: "target-1" }) });
    expect(response.status).toBe(503);
  });
});
