const LOCAL_CALLBACK_ORIGIN = "http://featherlane.local";
const LOGIN_RETRY_MESSAGE = "Google sign-in could not be completed. Please try again.";

export function isPublicAuthPath(pathname: string): boolean {
  return pathname === "/login" || pathname === "/api/auth" || pathname.startsWith("/api/auth/");
}

export function safeCallbackPath(value: unknown): string {
  if (
    typeof value !== "string"
    || !value.startsWith("/")
    || value.startsWith("//")
    || value.includes("\\")
    || /[\u0000-\u001f\u007f]/u.test(value)
  ) {
    return "/";
  }

  try {
    const callback = new URL(value, LOCAL_CALLBACK_ORIGIN);
    if (callback.origin !== LOCAL_CALLBACK_ORIGIN) return "/";
    return `${callback.pathname}${callback.search}`;
  } catch {
    return "/";
  }
}

export function loginErrorMessage(error: unknown): string | null {
  return typeof error === "string" && error.length > 0 ? LOGIN_RETRY_MESSAGE : null;
}

export function userInitials(name: string | null | undefined, email: string): string {
  const nameParts = name?.trim().split(/\s+/u).filter(Boolean) ?? [];
  if (nameParts.length > 1) {
    return `${Array.from(nameParts[0])[0]}${Array.from(nameParts.at(-1) ?? "")[0]}`.toUpperCase();
  }

  const source = nameParts[0] ?? email.split("@")[0] ?? "";
  const characters = Array.from(source.replace(/[^\p{L}\p{N}]/gu, "")).slice(0, 2).join("");
  return (characters || "FL").toUpperCase();
}
