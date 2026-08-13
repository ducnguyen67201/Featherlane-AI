"use client";

import { useState } from "react";
import { authClient } from "@/lib/auth-client";
import { safeCallbackPath } from "@/lib/auth-routing";

const SIGN_IN_ERROR = "Google sign-in could not be completed. Please try again.";

export function GoogleSignInButton({ callbackPath }: { callbackPath: string }) {
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function signIn() {
    setConnecting(true);
    setError(null);

    try {
      const result = await authClient.signIn.social({
        provider: "google",
        callbackURL: safeCallbackPath(callbackPath),
        errorCallbackURL: "/login?error=oauth",
      });
      if (result.error) {
        setError(SIGN_IN_ERROR);
        setConnecting(false);
      }
    } catch {
      setError(SIGN_IN_ERROR);
      setConnecting(false);
    }
  }

  return (
    <div className="login-action">
      <button className="google-sign-in" type="button" onClick={signIn} disabled={connecting}>
        <span className="google-mark" aria-hidden="true">G</span>
        <span>{connecting ? "Connecting…" : "Continue with Google"}</span>
      </button>
      {error && <p className="login-error" role="alert">{error}</p>}
    </div>
  );
}
