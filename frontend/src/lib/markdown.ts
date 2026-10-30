import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({
  gfm: true,
  breaks: true,
});

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function renderMarkdown(source?: string | null): string {
  if (!source || !source.trim()) return "";
  try {
    const html = marked.parse(source, { async: false }) as string;
    return DOMPurify.sanitize(html, {
      FORBID_TAGS: ["style", "iframe", "form", "input", "button", "object", "embed"],
      FORBID_ATTR: ["style", "onerror", "onclick"],
      ADD_ATTR: ["target", "rel"],
    });
  } catch {
    return `<p>${escapeHtml(source).replaceAll("\n", "<br />")}</p>`;
  }
}

export function plainPreview(source?: string | null, limit = 140): string {
  if (!source) return "";
  const text = source
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/[#>*_`~\-]/g, " ")
    .replace(/\[(.*?)\]\(.*?\)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > limit ? `${text.slice(0, limit)}…` : text;
}
