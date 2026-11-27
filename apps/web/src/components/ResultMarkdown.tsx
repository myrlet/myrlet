/**
 * Read-only rendering of the immutable Harness-written Markdown result.
 *
 * Text only: headings, lists, tables, fenced code, and links render through
 * `react-markdown` + `remark-gfm`. Raw HTML is never rendered, images are not
 * fetched (markdown image syntax shows its alt text instead), and every link
 * opens in a new tab without leaking the referrer. The text is a fact from the
 * Hub, never a verdict.
 */

import { ExternalLink } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

export interface ResultMarkdownProps {
  readonly markdown: string;
}

export default function ResultMarkdown({ markdown }: ResultMarkdownProps) {
  return (
    <div className="result-markdown">
      <Markdown
        skipHtml
        remarkPlugins={[remarkGfm]}
        components={{
          // Wide tables scroll inside their own container instead of pushing
          // the whole reader (or the narrow Overview pane) sideways.
          table({ children }) {
            return (
              <div className="result-table-scroll">
                <table>{children}</table>
              </div>
            );
          },
          a({ href, children }) {
            const target = typeof href === "string" ? href : undefined;
            if (target === undefined || !isSafeHref(target)) {
              return <span>{children}</span>;
            }
            return (
              <a href={target} target="_blank" rel="noreferrer noopener">
                {children}
                <ExternalLink size={11} aria-hidden="true" />
              </a>
            );
          },
          img({ alt }) {
            return <span className="result-markdown-image-alt">[image: {alt ?? "untitled"}]</span>;
          },
        }}
      >
        {markdown}
      </Markdown>
    </div>
  );
}

/** Only absolute http(s) and mailto links are followed; everything else is text. */
function isSafeHref(href: string): boolean {
  return /^(https?:\/\/|mailto:)/i.test(href.trim());
}
