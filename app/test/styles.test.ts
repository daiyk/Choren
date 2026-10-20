import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/app.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selector, body]) => ({ selector: selector.trim(), body }));

// Graph art has fixed geometry and its own zoom; the welcome headline follows the window.
const fixedType = /^\.box-|\.box-card|\.box-comment|\.react-flow__|^\.welcome h1$/;

describe("stylesheet", () => {
  it("sizes text with the Text size setting outside the graph", () => {
    const unscaled = rules
      .filter(({ selector }) => !fixedType.test(selector))
      .flatMap(({ selector, body }) =>
        [...body.matchAll(/(?:^|;)\s*(font-size|font)\s*:([^;]*)/g)]
          .filter(([, , value]) => /\d+px/.test(value) && !value.includes("var(--text-scale)"))
          .map(([, property, value]) => `${selector} { ${property}:${value} }`),
      );
    expect(unscaled).toEqual([]);
  });

  it("lets bars grow with their text instead of clipping it", () => {
    const bars = rules.filter(({ selector }) => /\.(app-menu|main-toolbar|status-bar|panel-toolbar|breadcrumbs)$/.test(selector));
    expect(bars.length).toBeGreaterThan(4);
    for (const { selector, body } of bars) expect(body, selector).not.toMatch(/(^|[;\s])height\s*:\s*[\d.]+px/);
  });
});
