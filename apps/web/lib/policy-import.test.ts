import { describe, expect, it } from "vitest";
import { isActiveImportStatus, validatePolicySourceFile } from "./policy-import";

describe("policy source import", () => {
  it("accepts supported files within the limit", () => {
    expect(validatePolicySourceFile({ name: "refund-policy.DOCX", size: 1024 })).toBeNull();
  });

  it("rejects unsupported and oversized files", () => {
    expect(validatePolicySourceFile({ name: "workflow.json", size: 100 })).toContain("PDF");
    expect(validatePolicySourceFile({ name: "law.pdf", size: 26 * 1024 * 1024 })).toContain("25 MiB");
  });

  it("polls only while server-side work is active", () => {
    expect(isActiveImportStatus("extracting")).toBe(true);
    expect(isActiveImportStatus("review_required")).toBe(false);
  });
});
