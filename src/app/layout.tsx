import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "Tampa-DOGE",
  description: "Civic transparency viewer + control panel",
};

const NAV: { href: string; label: string }[] = [
  { href: "/sitemap", label: "Sitemap" },
  { href: "/investigations", label: "Investigations" },
  { href: "/entities", label: "Entities" },
  { href: "/findings", label: "Findings" },
  { href: "/operator-queue", label: "Queue" },
  { href: "/status", label: "Status" },
  { href: "/briefings", label: "Briefings" },
  { href: "/db", label: "DB" },
  { href: "/chat", label: "Chat" },
];

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className="h-full antialiased">
      <body className="min-h-full flex flex-col">
        <header className="border-b border-black/10 dark:border-white/10">
          <nav className="mx-auto max-w-6xl flex flex-wrap items-center gap-x-5 gap-y-2 px-4 py-3">
            <Link href="/" className="font-semibold text-tampa-cyan tracking-tight">
              Tampa-DOGE
            </Link>
            <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
              {NAV.map((n) => (
                <li key={n.href}>
                  <Link href={n.href} className="hover:text-tampa-cyan">
                    {n.label}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
        </header>
        <main className="flex-1 mx-auto w-full max-w-6xl px-4 py-6">{children}</main>
        <footer className="border-t border-black/10 dark:border-white/10 text-xs text-black/50 dark:text-white/50">
          <div className="mx-auto max-w-6xl px-4 py-3">
            Tampa-DOGE · Hermes plugin · v0.1
          </div>
        </footer>
      </body>
    </html>
  );
}
