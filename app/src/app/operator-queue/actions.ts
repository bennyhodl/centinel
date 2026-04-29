"use server";

import { revalidatePath } from "next/cache";
import { z } from "zod";
import {
  resolveQueueItem,
  QUEUE_BUCKETS,
  type Decision,
} from "@/lib/operator-queue";

const Schema = z.object({
  bucket: z.enum(QUEUE_BUCKETS as unknown as [string, ...string[]]),
  slug: z
    .string()
    .trim()
    .min(1)
    .regex(/^[A-Za-z0-9._-]+$/, "invalid slug"),
  decision: z.enum(["approve", "reject", "dismiss", "acknowledge", "snooze"]),
  reason: z.string().trim().max(2000).optional(),
  snoozeUntil: z
    .string()
    .regex(/^\d{4}-\d{2}-\d{2}$/)
    .optional(),
});

export interface ResolveActionResult {
  ok: true;
  status: string;
  needsAgent: boolean;
  inboxPath: string | null;
}

export async function resolveItemAction(
  formData: FormData,
): Promise<ResolveActionResult> {
  const parsed = Schema.parse({
    bucket: formData.get("bucket"),
    slug: formData.get("slug"),
    decision: formData.get("decision"),
    reason: formData.get("reason") || undefined,
    snoozeUntil: formData.get("snoozeUntil") || undefined,
  });

  const result = await resolveQueueItem(
    parsed.bucket as Parameters<typeof resolveQueueItem>[0],
    parsed.slug,
    {
      decision: parsed.decision as Decision,
      reason: parsed.reason,
      snoozeUntil: parsed.snoozeUntil,
    },
  );

  revalidatePath("/operator-queue");
  revalidatePath("/status"); // outbox/inbox count surfaces here too

  return {
    ok: true,
    status: result.status,
    needsAgent: result.needsAgent,
    inboxPath: result.inboxPath,
  };
}
