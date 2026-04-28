import FindingsListView, {
  isTab,
  type TabKey,
} from "./_components/FindingsListView";

export const dynamic = "force-dynamic";

export default async function FindingsPage({
  searchParams,
}: {
  searchParams: Promise<{ stack?: string }>;
}) {
  const sp = await searchParams;
  const activeTab: TabKey = isTab(sp?.stack) ? sp.stack : "all";
  return <FindingsListView activeTab={activeTab} />;
}
