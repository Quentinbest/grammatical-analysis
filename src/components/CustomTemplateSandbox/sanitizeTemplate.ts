import DOMPurify from "dompurify";

const FORBIDDEN_TAGS = ["script", "iframe", "object", "embed", "form", "style"];
const FORBIDDEN_ATTRS = [
  "style",
  "onerror",
  "onload",
  "onclick",
  "onmouseover",
  "onfocus",
  "onblur",
  "onmouseenter",
  "onmouseleave",
];

const MAX_CSS_SANITIZE_PASSES = 16;

function decodeCssEscapes(css: string): string {
  return css
    .replace(/\\(?:\r\n|[\r\n\f])/g, "")
    .replace(/\\([0-9a-fA-F]{1,6})[ \t\r\n\f]?/g, (_match, hex: string) => {
      const codePoint = Number.parseInt(hex, 16);
      if (codePoint === 0 || codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
        return "";
      }
      return String.fromCodePoint(codePoint);
    })
    .replace(/\\([^\r\n\f0-9a-fA-F])/g, "$1")
    .replace(/\\$/g, "");
}

function sanitizeCssPass(css: string): string {
  const withoutCommentsAndEscapes = decodeCssEscapes(css.replace(/\/\*[\s\S]*?(?:\*\/|$)/g, ""));
  return withoutCommentsAndEscapes
    .replace(/<\/\s*style\b[^>]*>?/gi, "")
    .replace(/<\/\s*style/gi, "")
    .replace(/@import\b(?:"[^"]*(?:"|$)|'[^']*(?:'|$)|[^;"'])*;?/gi, "")
    .replace(/javascript\s*:/gi, "")
    .replace(/expression\s*\(/gi, "")
    .replace(/-moz-binding\b[^;{}]*;?/gi, "")
    .replace(/behavior\s*:[^;{}]*;?/gi, "")
    .replace(/[^{};]*:host(?:-context)?\b[^{}]*\{[^{}]*\}/gi, "")
    .replace(/:host(?:-context)?\b(?:\([^)]*\))?/gi, "")
    .replace(/position\s*:\s*fixed(?:\s*!\s*important)?\s*;?/gi, "");
}

export function sanitizeHtmlTemplate(html: string): string {
  return DOMPurify.sanitize(html, {
    FORBID_TAGS: FORBIDDEN_TAGS,
    FORBID_ATTR: FORBIDDEN_ATTRS,
    ALLOW_UNKNOWN_PROTOCOLS: false,
  });
}

export function sanitizeCssTemplate(css: string): string {
  let current = css;
  for (let pass = 0; pass < MAX_CSS_SANITIZE_PASSES; pass += 1) {
    const next = sanitizeCssPass(current);
    if (next === current) {
      return next;
    }
    current = next;
  }
  return current;
}
