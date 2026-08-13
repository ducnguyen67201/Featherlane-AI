import { describe, expect, it } from "vitest";
import { importProgress, isActiveImportStatus, validatePolicySourceFile } from "./policy-import";
import type { PolicyImportStatus } from "./types";

describe("policy source import", () => {
  it("accepts supported files within the limit", () => {
    expect(validatePolicySourceFile({ name: "refund-policy.DOCX", size: 1024 })).toBeNull();
  });

  it("rejects unsupported and oversized files", () => {
    expect(validatePolicySourceFile({ name: "empty.txt", size: 0 })).toBe("The selected file is empty.");
    expect(validatePolicySourceFile({ name: "workflow.json", size: 100 })).toContain("PDF");
    expect(validatePolicySourceFile({ name: "law.pdf", size: 26 * 1024 * 1024 })).toContain("25 MiB");
  });

  it("polls only while server-side work is active", () => {
    expect(isActiveImportStatus("extracting")).toBe(true);
    expect(isActiveImportStatus("review_required")).toBe(false);
  });

  it("maps every import status to a bounded progress value", () => {
    const statuses: PolicyImportStatus[] = [
      "uploading", "queued", "parsing", "extracting", "review_required",
      "ready_to_compile", "compiled", "needs_ocr", "failed_retryable", "failed_terminal",
    ];
    expect(statuses.map(importProgress)).toEqual([8, 20, 42, 72, 100, 100, 100, 100, 100, 100]);
  });
});
