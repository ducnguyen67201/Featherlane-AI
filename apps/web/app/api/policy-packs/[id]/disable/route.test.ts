import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();

vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

const context = { params: Promise.resolve({ id: "pack-1" }) };

describe("policy pack disable BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("requires an authenticated account", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const response = await POST(new Request("http://localhost/api/policy-packs/pack-1/disable", {
      method: "POST",
      body: "{}",
    }), context);
    expect(response.status).toBe(401);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("attributes disabling to the signed-in account", async () => {
    getCurrentSessionMock.mockResolvedValue({ user: { id: "user-1", email: "owner@example.com" } });
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ status: "disabled" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }));
    const response = await POST(new Request("http://localhost/api/policy-packs/pack-1/disable", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ actor_id: "spoofed@example.com", notes: "Pause" }),
    }), context);
    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/policy-packs/pack-1/disable",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ actor_id: "owner@example.com", notes: "Pause" }),
      }),
    );
  });
});
