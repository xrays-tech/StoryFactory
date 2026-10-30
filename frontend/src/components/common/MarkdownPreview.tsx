import { useMemo } from "react";
import { renderMarkdown } from "../../lib/markdown";
import { cn } from "../../lib/cn";

export type MarkdownPreviewProps = {
  source?: string | null;
  className?: string;
  empty?: string;
};

export function MarkdownPreview({ source, className, empty = "暂无内容" }: MarkdownPreviewProps) {
  const html = useMemo(() => renderMarkdown(source), [source]);

  if (!html) {
    return <p className={cn("text-sm text-faint", className)}>{empty}</p>;
  }

  return (
    <div
      className={cn("prose-story max-w-none", className)}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
