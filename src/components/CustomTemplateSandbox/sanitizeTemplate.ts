import DOMPurify from "dompurify";

const FORBIDDEN_TAGS = ["script", "iframe", "object", "embed", "form", "style"];
const FORBIDDEN_ATTRS = [
  "onerror",
  "onload",
  "onclick",
  "onmouseover",
  "onfocus",
  "onblur",
  "onmouseenter",
  "onmouseleave",
];

export function sanitizeHtmlTemplate(html: string): string {
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: FORBIDDEN_TAGS,
    FORBID_ATTR: FORBIDDEN_ATTRS,
    ALLOW_UNKNOWN_PROTOCOLS: false,
  });
}

export function sanitizeCssTemplate(css: string): string {
  return css
    .replace(/<\/style/gi, "")
    .replace(/@import\b[^;]*;?/gi, "")
    .replace(/javascript\s*:/gi, "")
    .replace(/expression\s*\(/gi, "");
}
