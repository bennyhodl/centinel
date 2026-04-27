import { redirect } from "next/navigation";
import { isSetupComplete } from "@/lib/setup-state";

export default async function HomePage() {
  if (await isSetupComplete()) {
    redirect("/sitemap");
  }
  redirect("/setup");
}
