import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();
vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { PATCH } from "./route";

describe("target telemetry boundary BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("authorizes before reading the request body", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const text = vi.fn(async () => "secret");
    const response = await PATCH({ method: "PATCH", text } as unknown as Request, { params: Promise.resolve({ id: "target-1" }) });
    expect(response.status).toBe(401);
    expect(text).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("encodes the target id and forwards PATCH", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockResolvedValue(new Response('{"id":"target-1"}', { status: 200 }));
    const request = new Request("http://localhost", { method: "PATCH", body: "{}" });
    const response = await PATCH(request, { params: Promise.resolve({ id: "target/1" }) });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/targets/target%2F1/telemetry-boundary",
      expect.objectContaining({ method: "PATCH", body: request.body }),
    );
    expect(response.status).toBe(200);
  });

  it("returns 503 when the upstream is offline", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1" } });
    fetchMock.mockRejectedValue(new Error("offline"));
    const response = await PATCH(new Request("http://localhost", { method: "PATCH", body: "{}" }), { params: Promise.resolve({ id: "target-1" }) });
    expect(response.status).toBe(503);
  });
});
