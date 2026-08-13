import { expect, it, vi } from "vitest";

const proxyGovernanceRequest = vi.hoisted(() => vi.fn());

vi.mock("@/lib/governance-proxy", () => ({ proxyGovernanceRequest }));

import { POST } from "./route";

it("delegates policy creation to the authenticated governance proxy", async () => {
  const request = new Request("http://localhost/api/policy-packs", { method: "POST", body: "{}" });
  const response = new Response("{}", { status: 201 });
  proxyGovernanceRequest.mockResolvedValue(response);

  await expect(POST(request)).resolves.toBe(response);
  expect(proxyGovernanceRequest).toHaveBeenCalledWith(request, "/v1/policy-packs");
});
