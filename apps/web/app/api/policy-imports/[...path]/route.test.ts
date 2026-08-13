import { beforeEach, describe, expect, it, vi } from "vitest";

const getCurrentSessionMock = vi.hoisted(() => vi.fn<() => Promise<unknown>>());
const proxyGovernanceRequestMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/session", () => ({ getCurrentSession: getCurrentSessionMock }));
vi.mock("@/lib/governance-proxy", () => ({
  proxyGovernanceRequest: proxyGovernanceRequestMock,
}));

import { PATCH, POST } from "./route";

const context = (path: string[]) => ({ params: Promise.resolve({ path }) });

describe("policy import review BFF", () => {
  beforeEach(() => {
    getCurrentSessionMock.mockReset();
    proxyGovernanceRequestMock.mockReset();
  });

  it("rejects review mutations without an authenticated session", async () => {
    getCurrentSessionMock.mockResolvedValue(null);
    const response = await PATCH(
      new Request("http://localhost/api/policy-imports/import-1/candidates/candidate-1", {
        method: "PATCH",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ decision: "approved" }),
      }),
      context(["import-1", "candidates", "candidate-1"]),
    );

    expect(response.status).toBe(401);
    expect(proxyGovernanceRequestMock).not.toHaveBeenCalled();
  });

  it("overrides a supplied reviewer with the signed-in account", async () => {
    getCurrentSessionMock.mockResolvedValue({
      user: { id: "user-1", email: "reviewer@example.com" },
    });
    proxyGovernanceRequestMock.mockResolvedValue(new Response("{}", {
      status: 200,
      headers: { "content-type": "application/json" },
    }));

    const response = await POST(
      new Request("http://localhost/api/policy-imports/import-1/verify-source", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          decision: "verified",
          reviewer_id: "spoofed@example.com",
        }),
      }),
      context(["import-1", "verify-source"]),
    );

    expect(response.status).toBe(200);
    expect(proxyGovernanceRequestMock).toHaveBeenCalledOnce();
    const [forwardedRequest, path] = proxyGovernanceRequestMock.mock.calls[0] as [Request, string];
    expect(path).toBe("/v1/policy-imports/import-1/verify-source");
    await expect(forwardedRequest.json()).resolves.toMatchObject({
      decision: "verified",
      reviewer_id: "reviewer@example.com",
    });
  });
});
