import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import Link from "next/link";
import { resolveWikilink } from "@/lib/wiki";

/**
 * Replace [[wikilinks]] with markdown links to the resolved internal route.
 * Done as a pre-processing pass over the source so react-markdown's link
 * renderer handles them naturally.
 */
function rewriteWikilinks(src: string): string {
  return src.replace(/\[\[([^\]\n]+?)\]\]/g, (_m, inner: string) => {
    // Support [[target|label]]
    const [target, label] = inner.split("|").map((s) => s.trim());
    const href = resolveWikilink(target);
    const text = label ?? target;
    return `[${text}](${href})`;
  });
}

export interface MarkdownViewProps {
  source: string;
  className?: string;
}

export default function MarkdownView({ source, className }: MarkdownViewProps) {
  const rewritten = rewriteWikilinks(source);
  return (
    <div className={className ?? "prose-broadsheet prose-wide"}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children, ...rest }) => {
            const h = href ?? "#";
            if (h.startsWith("/")) {
              return (
                <Link href={h} {...rest}>
                  {children}
                </Link>
              );
            }
            return (
              <a href={h} target="_blank" rel="noreferrer noopener" {...rest}>
                {children}
              </a>
            );
          },
        }}
      >
        {rewritten}
      </ReactMarkdown>
    </div>
  );
}
