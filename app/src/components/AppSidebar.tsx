"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const SECTIONS = [
  {
    heading: "Records",
    items: [
      { href: "/sitemap", label: "The Sitemap", roman: "I" },
      { href: "/investigations", label: "Investigations", roman: "II" },
      { href: "/entities", label: "Entities & Persons", roman: "III" },
      { href: "/findings", label: "Findings", roman: "IV" },
    ],
  },
  {
    heading: "Operations",
    items: [
      { href: "/operator-queue", label: "Operator Queue", roman: "V" },
      { href: "/status", label: "Status Board", roman: "VI" },
      { href: "/runs", label: "Agent Runs", roman: "VII" },
      { href: "/briefings", label: "Weekly Briefings", roman: "VIII" },
    ],
  },
  {
    heading: "Tools",
    items: [
      { href: "/db", label: "The Database", roman: "IX" },
      { href: "/chat", label: "Editor's Desk", roman: "X" },
    ],
  },
];

export function AppSidebar({ className }: { className?: string }) {
  const pathname = usePathname();
  const today = new Date();
  const dateStr = today.toLocaleDateString("en-US", {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  return (
    <aside className={`w-64 shrink-0 flex-col border-r-2 border-foreground/20 bg-sidebar ${className ?? ""}`}>
      {/* Masthead */}
      <div className="px-5 pt-6 pb-3 text-center">
        <Link href="/" className="block group">
          <div className="border-y-2 border-foreground/80 py-2">
            <h1 className="masthead text-[1.75rem] text-foreground tracking-tight">
              CENTINEL
            </h1>
          </div>
          <div className="mt-1.5 font-smallcaps text-[0.6rem] tracking-[0.2em] text-muted-foreground uppercase">
            Civic Transparency Gazette
          </div>
        </Link>
        <div className="mt-2 text-[0.6rem] text-muted-foreground italic">
          {dateStr}
        </div>
        <hr className="rule-double mt-3" />
      </div>

      {/* Table of Contents */}
      <nav className="flex-1 overflow-y-auto px-5 pb-4">
        {SECTIONS.map((section, si) => (
          <div key={section.heading} className={si > 0 ? "mt-4" : ""}>
            <div className="section-header">{section.heading}</div>
            <ul className="space-y-0.5">
              {section.items.map((item) => {
                const isActive =
                  pathname === item.href ||
                  pathname.startsWith(item.href + "/");
                return (
                  <li key={item.href}>
                    <Link
                      href={item.href}
                      className={`flex items-baseline gap-2 rounded-sm px-2 py-1.5 text-[0.8rem] leading-snug transition-colors ${
                        isActive
                          ? "bg-accent text-primary font-semibold"
                          : "text-foreground/80 hover:bg-accent hover:text-foreground"
                      }`}
                    >
                      <span className="font-mono text-[0.6rem] text-muted-foreground w-5 shrink-0 text-right tabular-nums">
                        {item.roman}.
                      </span>
                      <span>{item.label}</span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </nav>

      {/* Colophon */}
      <div className="border-t border-sidebar-border px-5 py-3">
        <div className="text-center text-[0.55rem] leading-relaxed text-muted-foreground">
          <span className="italic">Published by the</span>
          <br />
          <span className="font-smallcaps tracking-[0.15em] text-[0.6rem]">
            Hermes Intelligence Engine
          </span>
          <br />
          <span className="italic">Est. MMXXV &middot; Vol. I</span>
        </div>
      </div>
    </aside>
  );
}
