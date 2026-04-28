"use client";

import { type ReactNode, useState, useCallback, useEffect } from "react";
import { usePathname } from "next/navigation";
import { AppSidebar } from "@/components/AppSidebar";
import { Menu, X } from "lucide-react";

export function SidebarLayout({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();

  // Close drawer on navigation
  useEffect(() => {
    setOpen(false);
  }, [pathname]);

  const toggle = useCallback(() => setOpen((v) => !v), []);

  return (
    <div className="flex min-h-svh w-full">
      {/* Desktop sidebar */}
      <AppSidebar className="hidden md:flex" />

      {/* Mobile drawer overlay */}
      {open && (
        <div
          className="fixed inset-0 z-40 bg-foreground/30 md:hidden"
          onClick={() => setOpen(false)}
        />
      )}

      {/* Mobile drawer */}
      <div
        className={`fixed inset-y-0 left-0 z-50 w-72 transform transition-transform duration-250 ease-in-out md:hidden ${
          open ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        <AppSidebar className="flex h-full" />
        <button
          onClick={() => setOpen(false)}
          className="absolute top-3 right-3 p-1 text-muted-foreground hover:text-foreground"
          aria-label="Close menu"
        >
          <X className="size-5" />
        </button>
      </div>

      <main className="flex-1 flex flex-col min-w-0">
        {/* Mobile header with menu button */}
        <div className="flex items-center gap-3 border-b-2 border-foreground/15 px-4 py-3 md:hidden">
          <button
            onClick={toggle}
            className="p-1 text-foreground hover:text-primary"
            aria-label="Open menu"
          >
            <Menu className="size-5" />
          </button>
          <span className="masthead text-lg text-foreground">CENTINEL</span>
        </div>
        <div className="flex-1 px-4 py-6 md:px-8 md:py-8">
          <div className="mx-auto max-w-4xl">{children}</div>
        </div>
        {/* Footer colophon */}
        <footer className="border-t border-border px-4 py-4 md:px-8">
          <div className="mx-auto max-w-4xl flex items-center justify-between text-[0.65rem] text-muted-foreground">
            <span className="italic">
              Printed for the publick Benefit
            </span>
            <span className="font-smallcaps tracking-[0.12em]">
              Centinel &middot; Vol. I &middot; Hermes Engine
            </span>
          </div>
        </footer>
      </main>
    </div>
  );
}
