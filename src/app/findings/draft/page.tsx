import FindingsListView from "../_components/FindingsListView";

export const dynamic = "force-dynamic";

export default async function FindingsDraftPage() {
  return <FindingsListView activeTab="draft" showDraftBanner />;
}
