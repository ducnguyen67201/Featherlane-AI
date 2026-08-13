import { redirect } from "next/navigation";
import Image from "next/image";
import { GoogleSignInButton } from "@/components/google-sign-in-button";
import { loginErrorMessage, safeCallbackPath } from "@/lib/auth-routing";
import { getCurrentSession } from "@/lib/session";

type LoginPageProps = {
  searchParams: Promise<{
    callbackUrl?: string | string[];
    error?: string | string[];
  }>;
};

export default async function LoginPage({ searchParams }: LoginPageProps) {
  const params = await searchParams;
  const callbackPath = safeCallbackPath(params.callbackUrl);
  const session = await getCurrentSession();

  if (session) redirect(callbackPath);

  const providerError = loginErrorMessage(params.error);

  return (
    <main className="login-page">
      <section className="login-card" aria-labelledby="login-title">
        <div className="login-brand" aria-label="Featherlane">
          <Image
            className="brand-mark"
            src="/brand/featherlane-mark.png"
            alt=""
            width={32}
            height={30}
            aria-hidden="true"
          />
          <span>featherlane</span>
        </div>
        <p className="eyebrow">Governance console</p>
        <h1 id="login-title">Sign in to Featherlane</h1>
        <p className="login-copy">
          Review agent evaluations, evidence, and approved policy controls in one place.
        </p>
        {providerError && <p className="login-error" role="alert">{providerError}</p>}
        <GoogleSignInButton callbackPath={callbackPath} />
        <p className="login-footnote">
          Featherlane uses only your basic Google identity—name and email—to sign you in.
        </p>
      </section>
    </main>
  );
}
