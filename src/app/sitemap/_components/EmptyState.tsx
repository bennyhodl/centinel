export function SitemapEmptyState() {
  return (
    <div className="rounded-lg border border-dashed border-white/15 bg-white/[0.02] p-8 text-center">
      <h2 className="text-lg font-semibold">No sitemap yet</h2>
      <p className="mx-auto mt-2 max-w-md text-sm opacity-70">
        The Cartographer hasn&apos;t bootstrapped this city&apos;s sitemap yet.
        The sitemap is the project&apos;s central artifact — every investigation
        runs against it.
      </p>
      <pre className="mx-auto mt-4 max-w-md overflow-auto rounded bg-black/40 p-3 text-left font-mono text-xs text-tampa-cyan">
        hermes session run sitemap-builder --mode bootstrap --target tampa.gov
      </pre>
      <p className="mt-3 text-xs opacity-50">
        Or use the setup wizard at <a href="/setup" className="underline">/setup</a>.
      </p>
    </div>
  );
}
