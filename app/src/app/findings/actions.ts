"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import { promoteDraftToPublished } from "@/lib/findings";

export async function promoteFindingAction(formData: FormData) {
  const slug = String(formData.get("slug") ?? "").trim();
  if (!slug) throw new Error("slug is required");

  await promoteDraftToPublished(slug);

  // Bust caches for the lists, the old draft URL, and the new published URL.
  revalidatePath("/findings");
  revalidatePath("/findings/draft");
  revalidatePath(`/findings/${slug}`);
  redirect(`/findings/${slug}`);
}
