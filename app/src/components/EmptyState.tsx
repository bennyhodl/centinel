import type { ReactNode } from "react";

export function EmptyState({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="border border-border bg-card px-8 py-10 text-center">
      <div className="rule-ornament mb-4">
        <span className="text-muted-foreground text-xs">&#x2767;</span>
      </div>
      <h2 className="font-display text-xl font-semibold italic">{title}</h2>
      <div className="mx-auto mt-3 max-w-md text-sm leading-relaxed text-muted-foreground">
        {children}
      </div>
      <div className="rule-ornament mt-4">
        <span className="text-muted-foreground text-xs">&#x2767;</span>
      </div>
    </div>
  );
}
