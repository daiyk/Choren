import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Markdown } from "../src/workspace/markdown";

const html = (source: string) => renderToStaticMarkup(createElement(Markdown, { source }));

describe("release notes Markdown", () => {
  it("renders changelog bullets with wrapped lines and inline formatting", () => {
    expect(html("- Keep the order in `<project>.chorenproj`, Choren's own\n  file. **Bold** and *italic*.\n- Second entry"))
      .toBe("<div><ul><li>Keep the order in <code>&lt;project&gt;.chorenproj</code>, Choren&#x27;s own file. <strong>Bold</strong> and <em>italic</em>.</li><li>Second entry</li></ul></div>");
  });

  it("renders headings, paragraphs, numbered and nested lists", () => {
    expect(html("## Installation\n\n**macOS:** download\nthe .dmg.\n\n1. One\n   - Nested\n2. Two"))
      .toBe("<div><h3>Installation</h3><p><strong>macOS:</strong> download the .dmg.</p><ol><li><p>One</p><ul><li>Nested</li></ul></li><li>Two</li></ol></div>");
  });

  it("keeps HTML as text and drops comments", () => {
    expect(html("<!-- hidden -->\n<img src=x onerror=alert(1)> [site](https://example.com) snake_case_name"))
      .toBe('<div><p>&lt;img src=x onerror=alert(1)&gt; <span class="markdown-link" title="https://example.com">site</span> snake_case_name</p></div>');
  });

  it("renders fenced code verbatim", () => {
    expect(html("```\nxattr -dr **x**\n```")).toBe("<div><pre><code>xattr -dr **x**</code></pre></div>");
  });
});
