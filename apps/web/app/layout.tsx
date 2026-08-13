import type { Metadata } from "next";
import type { ReactNode } from "react";
import { AppShell } from "@/components/app-shell";
import { getCurrentSession } from "@/lib/session";
import "./globals.css";

export const metadata: Metadata = {
  title: "Featherlane | Agent governance evidence",
  description: "Test agent trajectories against approved governance policy packs.",
  icons: {
    icon: "/brand/featherlane-mark.png",
  },
};

export default async function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  const session = await getCurrentSession();

  return (
    <html lang="en">
      <body>
        {session ? (
          <AppShell user={{ name: session.user.name, email: session.user.email }}>
            {children}
          </AppShell>
        ) : children}
      </body>
    </html>
  );
}
