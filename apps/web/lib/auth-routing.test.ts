import { describe, expect, it } from "vitest";
import {
  isPublicAuthPath,
  loginErrorMessage,
  safeCallbackPath,
  userInitials,
} from "./auth-routing";

describe("auth routing policy", () => {
  it("only classifies the login and Better Auth handler paths as public", () => {
    expect(isPublicAuthPath("/login")).toBe(true);
    expect(isPublicAuthPath("/api/auth")).toBe(true);
    expect(isPublicAuthPath("/api/auth/callback/google")).toBe(true);
    expect(isPublicAuthPath("/")).toBe(false);
    expect(isPublicAuthPath("/api/authentication")).toBe(false);
  });

  it("preserves safe local callbacks with their query", () => {
    expect(safeCallbackPath("/evaluations/run-1?tab=evidence")).toBe(
      "/evaluations/run-1?tab=evidence",
    );
    expect(safeCallbackPath("/")).toBe("/");
  });

  it("collapses unsafe or malformed callbacks to the console root", () => {
    expect(safeCallbackPath("https://evil.example/steal")).toBe("/");
    expect(safeCallbackPath("//evil.example/steal")).toBe("/");
    expect(safeCallbackPath("/\\evil.example/steal")).toBe("/");
    expect(safeCallbackPath("")).toBe("/");
    expect(safeCallbackPath(undefined)).toBe("/");
    expect(safeCallbackPath(["/agents"])).toBe("/");
  });

  it("maps OAuth failures to a generic message without echoing input", () => {
    const rawError = "provider_secret_failure";
    const message = loginErrorMessage(rawError);

    expect(message).toBe("Google sign-in could not be completed. Please try again.");
    expect(message).not.toContain(rawError);
    expect(loginErrorMessage(undefined)).toBeNull();
  });

  it("derives deterministic initials from names and email fallbacks", () => {
    expect(userInitials("Duc Nguyen", "duc@example.com")).toBe("DN");
    expect(userInitials("  Nguyễn   An  ", "an@example.com")).toBe("NA");
    expect(userInitials(null, "duc@example.com")).toBe("DU");
    expect(userInitials("", "@example.com")).toBe("FL");
  });
});
