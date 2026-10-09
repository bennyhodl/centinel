import { cn } from "cn"

/**
 * A slot whose data has not arrived. With `mask`, it is that text made transparent inside
 * the real element, so the line keeps its font, size and wrap and nothing moves when the
 * data lands. Without, a block sized by its classes. Only data slots get one; the chrome
 * around them renders for real.
 */
function Skeleton({ className, mask, ...props }: React.ComponentProps<"span"> & { mask?: string }) {
  return (
    <span
      data-slot="skeleton"
      aria-hidden
      className={cn(
        "animate-pulse rounded-sm bg-[#EFE9DC] motion-reduce:animate-none",
        mask === undefined ? "block" : "box-decoration-clone select-none text-transparent [overflow-wrap:anywhere]",
        className,
      )}
      {...props}
    >
      {mask}
    </span>
  )
}

export { Skeleton }
