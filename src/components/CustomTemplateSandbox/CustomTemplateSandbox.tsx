import { useEffect, useRef } from "react";
import type { CardTheme } from "../../domain/types.ts";
import { sanitizeCssTemplate, sanitizeHtmlTemplate } from "./sanitizeTemplate.ts";

export interface CustomTemplateSandboxProps {
  readonly html: string;
  readonly css?: string | null;
  readonly theme?: CardTheme;
  readonly className?: string;
}

const BASE_SHADOW_CSS = `:host { display: block; contain: layout style; position: relative; color: inherit; font-family: inherit; }`;

export function CustomTemplateSandbox({
  html,
  css = null,
  theme = "dark-ink",
  className,
}: CustomTemplateSandboxProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const shadowRef = useRef<ShadowRoot | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      return;
    }

    const shadow = shadowRef.current ?? host.shadowRoot ?? host.attachShadow({ mode: "open" });
    shadowRef.current = shadow;

    const cleanHtml = sanitizeHtmlTemplate(html);
    const cleanCss = css ? sanitizeCssTemplate(css) : "";

    while (shadow.firstChild) {
      shadow.removeChild(shadow.firstChild);
    }

    const styleEl = document.createElement("style");
    styleEl.textContent = cleanCss.length > 0 ? `${BASE_SHADOW_CSS}\n${cleanCss}` : BASE_SHADOW_CSS;
    shadow.appendChild(styleEl);

    const container = document.createElement("div");
    container.setAttribute("data-card-theme", theme);
    container.innerHTML = cleanHtml;
    shadow.appendChild(container);
  }, [html, css, theme]);

  return (
    <div
      ref={hostRef}
      className={className}
      data-card-theme={theme}
      data-sandbox="custom-template"
    />
  );
}
