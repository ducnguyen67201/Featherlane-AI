import { beforeEach, describe, expect, it, vi } from "vitest";

const proxyGovernanceRequest = vi.hoisted(() => vi.fn());
vi.mock("@/lib/governance-proxy", () => ({ proxyGovernanceRequest }));

import { GET } from "./route";

describe("source connection callback", () => {
  beforeEach(() => proxyGovernanceRequest.mockReset());

  it("follows only local policy redirects and disables caching", async () => {
    proxyGovernanceRequest.mockResolvedValue(new Response(JSON.stringify({ redirect: "/policies/collections/collection-1" }), { status: 200 }));
    const response = await GET(new Request("http://localhost/api/source-connections/google_drive/callback?code=a&state=b"), { params: Promise.resolve({ path: ["google_drive", "callback"] }) });
    expect(response.status).toBe(307);
    expect(response.headers.get("location")).toBe("http://localhost/policies/collections/collection-1");
    expect(response.headers.get("cache-control")).toBe("no-store");
  });

  it("rejects an upstream open redirect", async () => {
    proxyGovernanceRequest.mockResolvedValue(new Response(JSON.stringify({ redirect: "//attacker.example" }), { status: 200 }));
    const response = await GET(new Request("http://localhost/api/source-connections/notion/callback"), { params: Promise.resolve({ path: ["notion", "callback"] }) });
    expect(response.headers.get("location")).toBe("http://localhost/policies?connector_error=invalid_redirect");
  });
});
