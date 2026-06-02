export function SitemapEmptyState() {
  return (
    <div className="border border-border bg-card px-8 py-10 text-center">
      <div className="rule-ornament mb-4">
        <span className="text-muted-foreground text-xs">&#x2767;</span>
      </div>
      <h2 className="font-display text-xl font-semibold italic">
        No sitemap yet
      </h2>
      <p className="mx-auto mt-3 max-w-md text-sm text-muted-foreground leading-relaxed">
        The Cartographer hasn&apos;t bootstrapped this city&apos;s sitemap yet.
        The sitemap is the project&apos;s central artifact — every investigation
        runs against it.
      </p>
      <pre className="mx-auto mt-4 max-w-md overflow-auto border border-border bg-secondary p-3 text-left font-mono text-xs text-foreground/80">
        centinel role investigator -p "bootstrap: build sitemap for tampa.gov"
      </pre>
      <p className="mt-3 text-xs text-muted-foreground italic">
        Or use the setup wizard at <a href="/setup" className="text-primary underline">/setup</a>.
      </p>
      <div className="rule-ornament mt-4">
        <span className="text-muted-foreground text-xs">&#x2767;</span>
      </div>
    </div>
  );
}
