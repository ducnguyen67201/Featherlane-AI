import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const fetchMock = vi.fn<typeof fetch>();

vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));

import { POST } from "./route";

const context = { params: Promise.resolve({ id: "pack-1" }) };

describe("policy pack approval BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("does not publish a pack without an authenticated session", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const response = await POST(new Request("http://localhost/api/policy-packs/pack-1/approve", {
      method: "POST",
      body: "{}",
    }), context);

    expect(response.status).toBe(401);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("attributes publication to the signed-in account", async () => {
    getCurrentSessionMock.mockResolvedValue({
      user: { id: "user-1", email: "owner@example.com" },
    });
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ status: "approved" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }));

    const response = await POST(new Request("http://localhost/api/policy-packs/pack-1/approve", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ reviewer_id: "spoofed@example.com", notes: "Reviewed" }),
    }), context);

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/v1/policy-packs/pack-1/approve",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ reviewer_id: "owner@example.com", notes: "Reviewed" }),
      }),
    );
  });
});
