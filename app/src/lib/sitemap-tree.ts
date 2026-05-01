import type { SitemapDoc, SitemapEntry } from "./sitemap";

/**
 * URL tree built from sitemap entries.
 *
 * The tree groups entries by `host` then by URL path segments. A node is
 * simultaneously a *branch* (has children) and/or a *leaf* (has its own
 * crawled entry). Branch-only nodes (no entry) are synthesized as section
 * indexes during render.
 */

export interface TreeNode {
  /** segment label at this depth ("" for the host root, e.g. "page" or "rfp-2024-12") */
  segment: string;
  /** full path from host root, joined by "/" — e.g. "page/about". "" at root. */
  path: string;
  /** the sitemap entry for this exact URL, if present */
  entry?: SitemapEntry;
  /** child nodes keyed by next path segment */
  children: Map<string, TreeNode>;
  /** descendant counts (entries below this node, including self) */
  descendantCount: number;
}

export interface HostTree {
  host: string;
  /** the host's root node ("" segment) — its `entry` is the homepage entry if crawled */
  root: TreeNode;
  /** total entries for this host (=== root.descendantCount) */
  total: number;
}

function parseUrl(u: string): { host: string; segments: string[] } | null {
  try {
    const parsed = new URL(u);
    const segs = parsed.pathname
      .split("/")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
    return { host: parsed.host, segments: segs };
  } catch {
    return null;
  }
}

function emptyNode(segment: string, fullPath: string): TreeNode {
  return {
    segment,
    path: fullPath,
    children: new Map(),
    descendantCount: 0,
  };
}

/**
 * Build a host->tree map from sitemap entries. Entries with unparseable URLs
 * are silently dropped (caller can filter beforehand if it needs to surface them).
 */
export function buildHostTrees(
  doc: SitemapDoc,
): Map<string, HostTree> {
  const hosts = new Map<string, HostTree>();

  for (const entry of doc.entries) {
    const parsed = parseUrl(entry.url);
    if (!parsed) continue;
    const { host, segments } = parsed;

    let tree = hosts.get(host);
    if (!tree) {
      tree = { host, root: emptyNode("", ""), total: 0 };
      hosts.set(host, tree);
    }

    // Walk down the tree, creating nodes as needed
    let node = tree.root;
    node.descendantCount += 1;
    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i];
      const childPath = segments.slice(0, i + 1).join("/");
      let child = node.children.get(seg);
      if (!child) {
        child = emptyNode(seg, childPath);
        node.children.set(seg, child);
      }
      child.descendantCount += 1;
      node = child;
    }
    // node is now the leaf for this URL
    node.entry = entry;
    tree.total += 1;
  }

  return hosts;
}

/**
 * Pinned-host ordering. The host matching CENTINEL_HOST_DOMAIN env var (or
 * inferred from CENTINEL_WIKI_PATH) comes first; everything else alphabetical.
 */
export function orderHosts(
  hosts: string[],
  pinned?: string,
): string[] {
  const sorted = [...hosts].sort((a, b) => a.localeCompare(b));
  if (!pinned) return sorted;
  const idx = sorted.indexOf(pinned);
  if (idx < 0) return sorted;
  return [pinned, ...sorted.filter((h) => h !== pinned)];
}

/**
 * Resolve a node by host + path segments. Returns null if not found.
 */
export function resolveNode(
  tree: HostTree,
  segments: string[],
): TreeNode | null {
  let node: TreeNode | undefined = tree.root;
  for (const seg of segments) {
    if (!node) return null;
    node = node.children.get(seg);
  }
  return node ?? null;
}

/**
 * Sorted children for a node — entries first (alphabetic), then branch-only
 * directories (alphabetic).
 */
export function sortedChildren(node: TreeNode): TreeNode[] {
  const arr = [...node.children.values()];
  arr.sort((a, b) => {
    const aHasEntry = a.entry ? 0 : 1;
    const bHasEntry = b.entry ? 0 : 1;
    if (aHasEntry !== bHasEntry) return aHasEntry - bHasEntry;
    return a.segment.localeCompare(b.segment);
  });
  return arr;
}
