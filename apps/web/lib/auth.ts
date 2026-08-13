import "server-only";

import { betterAuth } from "better-auth";

const SESSION_LIFETIME_SECONDS = 8 * 60 * 60;

export const auth = betterAuth({
  appName: "Featherlane",
  baseURL: process.env.BETTER_AUTH_URL,
  secret: process.env.BETTER_AUTH_SECRET,
  socialProviders: {
    google: {
      clientId: process.env.GOOGLE_CLIENT_ID ?? "",
      clientSecret: process.env.GOOGLE_CLIENT_SECRET ?? "",
    },
  },
  session: {
    expiresIn: SESSION_LIFETIME_SECONDS,
    disableSessionRefresh: true,
    cookieCache: {
      enabled: true,
      strategy: "jwe",
      maxAge: SESSION_LIFETIME_SECONDS,
      refreshCache: false,
      version: "1",
    },
  },
  account: {
    storeStateStrategy: "cookie",
    storeAccountCookie: true,
  },
});
