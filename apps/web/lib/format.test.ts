import { describe, expect, it } from "vitest";
import { formatDuration, verdictLabel } from "./format";

describe("console formatters", () => {
  it("preserves inconclusive as a first-class verdict", () => {
    expect(verdictLabel("INCONCLUSIVE")).toBe("Inconclusive");
  });

  it("renders evaluation duration compactly", () => {
    expect(formatDuration(2_418)).toBe("2.4 s");
  });
});
