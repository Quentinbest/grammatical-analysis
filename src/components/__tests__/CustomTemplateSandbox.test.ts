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

  it("strips inline style attributes from HTML templates to prevent CSS sanitization bypass", () => {
    const dirty = `<div style="position:fixed;inset:0;background:url(javascript:alert(1));width:expression(alert(1))"><p style="color:red">Safe text</p></div>`;
    const clean = sanitizeHtmlTemplate(dirty);

    expect(clean).not.toContain("style=");
    expect(clean).not.toContain("position:fixed");
    expect(clean).not.toContain("javascript:");
    expect(clean).not.toContain("expression(");
    expect(clean).toContain("Safe text");
  });

  it("defeats nested reconstruction, comments, escapes, quoted @import semicolons, :host breakout, and position:fixed in CSS", () => {
    const hostileCss = [
      `</sty</style>le><script>alert(1)</script>`,
      `@imp@import url("https://evil.example/a;b.css");ort url("https://evil.example/c;d.css");`,
      `@import "https://evil.example/semi;colon.css" screen;`,
      `.a { width: exp/*bypass*/ression(alert(1)); background: java/*x*/script:alert(1); }`,
      `.b { width: ex\\70ression(alert(1)); -moz-bind\\69ng: url("https://evil.example/x.xml"); behav\\69or: url(xss.htc); }`,
      `:host { position: fixed; inset: 0; z-index: 99999; }`,
      `:host-context(body) { display: none; }`,
      `:\\68ost(.evil) { width: 100vw; }`,
      `.overlay { position: fixed !important; pos\\69tion: f\\69xed; color: teal; }`,
    ].join("\n");

    const cleanCss = sanitizeCssTemplate(hostileCss);

    expect(cleanCss.toLowerCase()).not.toContain("</style");
    expect(cleanCss.toLowerCase()).not.toContain("@import");
    expect(cleanCss.toLowerCase()).not.toContain("evil.example");
    expect(cleanCss.toLowerCase()).not.toContain("expression(");
    expect(cleanCss.toLowerCase()).not.toContain("javascript:");
    expect(cleanCss.toLowerCase()).not.toContain("-moz-binding");
    expect(cleanCss.toLowerCase()).not.toContain("behavior:");
    expect(cleanCss.toLowerCase()).not.toContain(":host");
    expect(cleanCss.toLowerCase()).not.toMatch(/position\s*:\s*fixed/);
    expect(cleanCss).toContain("color: teal");
  });

  it("enforces layout/style containment on Shadow Root :host base style", () => {
    act(() => {
      root.render(
        createElement(CustomTemplateSandbox, {
          html: `<div style="position:fixed">Contained</div>`,
          css: `:host { position: fixed; } .inner { position: fixed; padding: 4px; }`,
          theme: "dark-ink",
        }),
      );
    });

    const shadow = container.querySelector('[data-sandbox="custom-template"]')?.shadowRoot;
    const styleText = shadow?.querySelector("style")?.textContent ?? "";
    expect(styleText).toContain(":host { display: block; contain: layout style; position: relative;");
    expect(styleText).not.toMatch(/position\s*:\s*fixed/);
    expect(styleText).toContain("padding: 4px");
    expect(shadow?.querySelector("div div")?.getAttribute("style")).toBeNull();
  });
});
