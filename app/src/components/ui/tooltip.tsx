"use client";

import * as React from "react";
import { cn } from "@/lib/utils";

/* Minimal tooltip — no radix dependency needed */

interface TooltipProviderProps {
  children: React.ReactNode;
  delayDuration?: number;
}

const TooltipContext = React.createContext({ delayDuration: 200 });

function TooltipProvider({ children, delayDuration = 200 }: TooltipProviderProps) {
  return (
    <TooltipContext.Provider value={{ delayDuration }}>
      {children}
    </TooltipContext.Provider>
  );
}

function Tooltip({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

const TooltipTrigger = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement> & { asChild?: boolean }
>(({ children, ...props }, ref) => {
  if (props.asChild) {
    return <>{children}</>;
  }
  return (
    <button ref={ref} {...props}>
      {children}
    </button>
  );
});
TooltipTrigger.displayName = "TooltipTrigger";

function TooltipContent({
  children,
  side: _side,
  className,
}: {
  children: React.ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  className?: string;
}) {
  // Tooltip content is a no-op in this minimal implementation
  // The sidebar handles its own collapsed tooltips
  return (
    <span className={cn("sr-only", className)}>{children}</span>
  );
}

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
