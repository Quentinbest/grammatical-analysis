// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  CustomTemplateSandbox,
  sanitizeCssTemplate,
  sanitizeHtmlTemplate,
} from "../CustomTemplateSandbox/index.ts";

describe("CustomTemplateSandbox", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("strips script tags, inline event handlers, and javascript URLs via DOMPurify", () => {
    const dirty = [
      `<div class="card">`,
      `<script>window.__xss = 1;</script>`,
      `<img src="x" onerror="window.__xss = 2" alt="safe" />`,
      `<a href="javascript:alert(1)" onclick="alert(2)">Link</a>`,
      `<iframe src="https://evil.example"></iframe>`,
      `<p>Clean content</p>`,
      `</div>`,
    ].join("");

    const clean = sanitizeHtmlTemplate(dirty);
    expect(clean).not.toContain("<script");
    expect(clean).not.toContain("onerror");
    expect(clean).not.toContain("onclick");
    expect(clean).not.toContain("javascript:");
    expect(clean).not.toContain("<iframe");
    expect(clean).toContain("Clean content");
  });

  it("strips dangerous directives from custom CSS templates", () => {
    const dirtyCss = `@import url("https://evil.example/a.css"); </style><script>alert(1)</script> .x { color: red; background: expression(alert(1)); }`;
    const cleanCss = sanitizeCssTemplate(dirtyCss);
    expect(cleanCss).not.toContain("@import");
    expect(cleanCss).not.toContain("</style");
    expect(cleanCss).not.toContain("expression(");
    expect(cleanCss).toContain("color: red");
  });

  it("mounts sanitized HTML and scoped CSS inside an isolated Shadow Root", () => {
    const payload = `<section class="custom"><script>window.__pwned = true;</script><img src="x" onerror="alert(1)" /><strong>Parsed OK</strong></section>`;
    const customCss = `.custom { padding: 12px; }`;

    act(() => {
      root.render(
        createElement(CustomTemplateSandbox, {
          html: payload,
          css: customCss,
          theme: "warm-paper",
        }),
      );
    });

    const host = container.querySelector('[data-sandbox="custom-template"]');
    expect(host).not.toBeNull();
    expect(host?.shadowRoot).not.toBeNull();

    const shadow = host?.shadowRoot;
    expect(shadow?.querySelector("script")).toBeNull();
    const img = shadow?.querySelector("img");
    expect(img?.getAttribute("onerror")).toBeNull();
    expect(shadow?.querySelector("strong")?.textContent).toBe("Parsed OK");
    expect(shadow?.querySelector("style")?.textContent).toContain(".custom { padding: 12px; }");
    expect(container.querySelector("strong")).toBeNull();
  });
});
