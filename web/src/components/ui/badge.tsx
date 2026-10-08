import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

const badgeVariants = cva('ui-badge', { variants: { variant: { default: 'ui-badge--default', muted: 'ui-badge--muted', warning: 'ui-badge--warning', success: 'ui-badge--success' } }, defaultVariants: { variant: 'default' } })

export function Badge({ className, variant, ...props }: React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />
}
