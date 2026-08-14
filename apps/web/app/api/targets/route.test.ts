import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();
vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

describe("target create BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("authorizes before reading the body", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const text = vi.fn(async () => "secret");
    const response = await POST({ text } as unknown as Request);
    expect(response.status).toBe(401);
    expect(text).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("forwards the exact body and upstream response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response('{"id":"target-1"}', { status: 201 }));
    const body = '{"name":"Refund Agent"}';
    const request = new Request("http://localhost/api/targets", { method: "POST", body });
    const response = await POST(request);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/targets",
      expect.objectContaining({ body: request.body }),
    );
    expect(response.status).toBe(201);
  });

  it("returns a target-specific offline response", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));
    const response = await POST(new Request("http://localhost/api/targets", { method: "POST", body: "{}" }));
    expect(response.status).toBe(503);
  });
});
