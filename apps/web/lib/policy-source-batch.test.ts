import { describe, expect, it } from "vitest";
import {
  MAX_POLICY_SOURCE_BATCH_BYTES,
  validatePolicySourceBatch,
  validateProviderSelectionIds,
  validatePublicPolicyUrls,
} from "./policy-source-batch";

describe("policy source batch validation", () => {
  it("rejects the 26th file", () => {
    const files = Array.from({ length: 26 }, (_, index) => ({ name: `${index}.txt`, size: 1 }));
    expect(validatePolicySourceBatch(files)).toContain("at most 25");
  });

  it("accepts only provider-specific explicit IDs", () => {
    expect(validateProviderSelectionIds("google_drive", "1AbCdEfGhIjKlMnOp").error).toBeNull();
    expect(validateProviderSelectionIds("google_drive", "folder/id").error).toContain("invalid");
    expect(validateProviderSelectionIds("microsoft_graph", "drive-1:item-1").error).toBeNull();
    expect(validateProviderSelectionIds("microsoft_graph", "item-only").error).toContain("invalid");
    expect(validateProviderSelectionIds("notion", "01234567-89ab-cdef-0123-456789abcdef").error).toBeNull();
  });

  it("rejects aggregate bytes even when every file is individually valid", () => {
    const files = Array.from({ length: 5 }, (_, index) => ({
      name: `${index}.pdf`,
      size: MAX_POLICY_SOURCE_BATCH_BYTES / 4,
    }));
    expect(validatePolicySourceBatch(files)).toContain("100 MiB");
  });

  it("accepts unique HTTPS URLs and rejects duplicates or credentials", () => {
    expect(validatePublicPolicyUrls("https://example.com/a\nhttps://example.com/b").error).toBeNull();
    expect(validatePublicPolicyUrls("https://example.com/a\nhttps://example.com/a").error).toContain("only once");
    expect(validatePublicPolicyUrls("https://user@example.com/a").error).toContain("without credentials");
  });
});
