import type { ReactNode } from "react";

export function EmptyState({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-dashed border-white/15 bg-white/[0.02] p-8 text-center">
      <h2 className="text-lg font-semibold">{title}</h2>
      <div className="mx-auto mt-2 max-w-md text-sm opacity-70">{children}</div>
    </div>
  );
}
