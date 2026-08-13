"use client";

import { useState, type ReactNode } from "react";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  Bell,
  Bot,
  Braces,
  Command,
  FlaskConical,
  LayoutDashboard,
  Library,
  LogOut,
  ScrollText,
  Search,
  Settings,
  ShieldCheck,
} from "lucide-react";
import { authClient } from "@/lib/auth-client";
import { userInitials } from "@/lib/auth-routing";

const primaryNav = [
  { href: "/", label: "Overview", icon: LayoutDashboard },
  { href: "/evaluations", label: "Evaluations", icon: FlaskConical },
  { href: "/policies", label: "Policy packs", icon: ScrollText },
  { href: "/agents", label: "Agents", icon: Bot },
  { href: "/corpus", label: "Source corpus", icon: Library },
];

type AppShellProps = {
  children: ReactNode;
  user: {
    name: string;
    email: string;
  };
};

export function AppShell({ children, user }: AppShellProps) {
  const pathname = usePathname();
  const [signingOut, setSigningOut] = useState(false);
  const [signOutError, setSignOutError] = useState<string | null>(null);

  async function signOut() {
    setSigningOut(true);
    setSignOutError(null);

    try {
      const result = await authClient.signOut();
      if (result.error) {
        setSignOutError("Sign out failed. Please try again.");
        setSigningOut(false);
        return;
      }
      window.location.assign(new URL("/login", window.location.origin).toString());
    } catch {
      setSignOutError("Sign out failed. Please try again.");
      setSigningOut(false);
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <Link className="brand" href="/" aria-label="Featherlane home">
          <Image
            className="brand-mark"
            src="/brand/featherlane-mark.png"
            alt=""
            width={32}
            height={30}
            aria-hidden="true"
          />
          <span>featherlane</span>
        </Link>

        <label className="global-search">
          <Search size={17} aria-hidden="true" />
          <span className="sr-only">Search evaluations, agents, and policies</span>
          <input placeholder="Search evaluations, agents, policies" />
          <kbd><Command size={12} /> K</kbd>
        </label>

        <nav className="top-links" aria-label="Product links">
          <Link href="/evaluations">Runs</Link>
          <Link href="/policies">Controls</Link>
          <Link href="/corpus">Sources</Link>
          <Link href="/agents">Integrations</Link>
        </nav>

        <div className="top-actions">
          <button className="icon-button" aria-label="Notifications"><Bell size={17} /></button>
          <div className="user-control">
            <span className="avatar" aria-hidden="true">{userInitials(user.name, user.email)}</span>
            <span className="user-identity">
              <strong>{user.name || user.email}</strong>
              <small>{user.email}</small>
            </span>
            <button
              className="logout-button"
              type="button"
              onClick={signOut}
              disabled={signingOut}
              aria-label={signingOut ? "Signing out" : "Sign out"}
            >
              <LogOut size={15} aria-hidden="true" />
              <span>{signingOut ? "Signing out…" : "Sign out"}</span>
            </button>
          </div>
          {signOutError && <span className="sign-out-error" role="alert">{signOutError}</span>}
        </div>
      </header>

      <aside className="sidebar">
        <div className="environment-label">
          <ShieldCheck size={16} /> Governance console
        </div>
        <nav className="side-nav" aria-label="Governance navigation">
          {primaryNav.map((item) => {
            const active = item.href === "/"
              ? pathname === "/"
              : pathname.startsWith(item.href);
            const Icon = item.icon;
            return (
              <Link key={item.href} href={item.href} className={active ? "active" : undefined}>
                <Icon size={17} />
                <span>{item.label}</span>
                {item.label === "Evaluations" && <span className="nav-count">184</span>}
              </Link>
            );
          })}
        </nav>
        <div className="sidebar-section-label">Developer</div>
        <nav className="side-nav" aria-label="Developer navigation">
          <a href="http://localhost:8080/health"><Braces size={17} /><span>API status</span></a>
          <a href="#configuration"><Settings size={17} /><span>Configuration</span></a>
        </nav>
        <div className="sidebar-note">
          <span className="live-dot" />
          Trace gateway online
          <small>94.2% evidence coverage</small>
        </div>
      </aside>

      <main className="main-content">{children}</main>
    </div>
  );
}
