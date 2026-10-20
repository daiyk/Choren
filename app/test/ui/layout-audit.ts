/**
 * Finds layout defects a person notices at a glance: text cut off without an ellipsis, text that
 * spills out of its box, button labels that wrap, things that sit on top of each other, panels
 * that scroll sideways, and labels that are not centred on the control beside them.
 *
 * It runs inside the page (pass it to `page.evaluate`), so it must stay self-contained.
 * Graph art, the code editor and canvases have their own geometry and zoom, so they are skipped.
 */
export function auditLayout(scope = "body"): string[] {
  const SKIP =
    ".react-flow__viewport, .react-flow__minimap, .react-flow__attribution, .cm-editor, canvas, svg, option, script, style, [aria-hidden='true']";
  const SIDEWAYS_OK = ".breadcrumbs, .dv-tabs-container, .dv-tabs-and-actions-container, .timeline-viewport, .dope-sheet, .react-flow, .cm-scroller, .motion-code, .topic-editor, textarea";
  const CONTROL = "button, input, select, textarea, [role='tab']";
  const issues = new Set<string>();
  const css = (el: Element) => getComputedStyle(el);
  const label = (el: Element) => {
    const cls = [...el.classList].slice(0, 2).map((c) => `.${c}`).join("");
    const text = (el.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 48);
    return `${el.tagName.toLowerCase()}${cls}${text ? ` "${text}"` : ""}`;
  };
  const shown = (el: Element) => {
    const r = el.getBoundingClientRect();
    return r.width > 2 && r.height > 2 && css(el).visibility !== "hidden" && !el.closest("[hidden], details:not([open]) > :not(summary)");
  };
  const isInline = (el: Element) => css(el).display === "inline" || css(el).display === "contents";
  const boxOf = (el: Element) => {
    let box = el;
    while (isInline(box) && box.parentElement) box = box.parentElement;
    return box;
  };
  const clips = (el: Element) => css(el).overflowX !== "visible" || css(el).overflowY !== "visible";
  const scrolls = (el: Element) => /auto|scroll/.test(css(el).overflowX + css(el).overflowY);
  // An ellipsis excuses text cut at the side, a line clamp text cut at the bottom; nothing else.
  const truncatesX = (el: Element) => css(el).textOverflow === "ellipsis";
  const truncatesY = (el: Element) => css(el).webkitLineClamp !== "none";
  /** The part of a rect still visible after every clipping ancestor (scrolled away = gone). */
  const visiblePart = (r: DOMRect, from: Node) => {
    let { left, top, right, bottom } = r;
    for (let a = from.parentElement; a; a = a.parentElement) {
      if (!clips(a) || a === document.body) continue;
      const c = a.getBoundingClientRect();
      left = Math.max(left, c.left);
      top = Math.max(top, c.top);
      right = Math.min(right, c.right);
      bottom = Math.min(bottom, c.bottom);
    }
    return right - left > 1 && bottom - top > 1 ? { left, top, right, bottom } : undefined;
  };
  const T = 1.5;
  const outside = (r: { left: number; top: number; right: number; bottom: number }, b: DOMRect) =>
    ({ x: r.left < b.left - T || r.right > b.right + T, y: r.top < b.top - T || r.bottom > b.bottom + T });

  type Box = { left: number; top: number; right: number; bottom: number };
  type Leaf = { el: Element; rect: Box; full: Box; text: boolean };
  const leaves: Leaf[] = [];

  for (const root of document.querySelectorAll(scope)) {
    if (!shown(root)) continue;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const parent = node.parentElement;
      if (!parent || !node.textContent?.trim() || parent.closest(SKIP) || !shown(parent)) continue;
      if (parent.closest("select, textarea")) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const rects = [...range.getClientRects()].filter((r) => r.width > 0.5);
      if (!rects.length) continue;
      const box = boxOf(parent);
      const b = box.getBoundingClientRect();
      const control = parent.closest(CONTROL);

      // Text drawn outside the box that lays it out, unless that box truncates or scrolls on purpose.
      if (!scrolls(box)) {
        // pre-wrap keeps the space at the end of a line, which may hang past the edge unseen.
        const hang = /pre-wrap|break-spaces/.test(css(parent).whiteSpace) ? parseFloat(css(parent).fontSize) / 2 : 0;
        const inner = new DOMRect(b.left - hang, b.top, b.width + 2 * hang, b.height);
        const out = rects.map((r) => outside(r, inner)).reduce((a, o) => ({ x: a.x || o.x, y: a.y || o.y }), { x: false, y: false });
        if (out.x && !truncatesX(box)) issues.add(`text overflows sideways: ${label(box)}`);
        if (out.y && !truncatesY(box)) issues.add(`text overflows vertically: ${label(box)}`);
      }
      // Text cut by a clipping ancestor (not a scroller, which the user can scroll).
      for (let a = box.parentElement; a && a !== document.body; a = a.parentElement) {
        if (!clips(a)) continue;
        if (scrolls(a)) break;
        const o = rects.map((r) => outside(r, a.getBoundingClientRect()));
        const cut = o.some((o) => o.x && !truncatesX(box) && !truncatesX(a)) || o.some((o) => o.y && !truncatesY(box) && !truncatesY(a));
        if (cut) issues.add(`text clipped by ${label(a).split(" ")[0]}: ${label(box)}`);
        break;
      }
      // A field's label never splits around the field (label text flowing like words around an input).
      const field = parent.querySelector(":scope > :is(input:not([type=checkbox], [type=radio]), select, textarea)");
      if (field && new Set(rects.map((r) => Math.round(r.top / 4))).size > 1) issues.add(`label breaks around its field: ${label(parent)}`);
      if (parent.closest(".app-menu, .main-toolbar, .status-bar") && new Set(rects.map((r) => Math.round(r.top / 4))).size > 1)
        issues.add(`text wraps in a one-line bar: ${label(box)}`);
      // A control's label (its own text, or a slot of its flex/grid row) stays on one line;
      // block-level descriptions inside a control may wrap.
      if (control) {
        const slot = box === control || /flex|grid/.test(css(box.parentElement!).display);
        const lines = new Set(rects.map((r) => Math.round(r.top / 4)));
        if (slot && lines.size > 1) issues.add(`control label wraps: ${label(control)}`);
      }
      if (!control) {
        for (const r of rects) {
          const v = visiblePart(r, node);
          if (v) leaves.push({ el: parent, rect: v, full: r, text: true });
        }
      }
    }
    for (const el of root.querySelectorAll(CONTROL)) {
      if (el.closest(SKIP) || !shown(el) || (el as HTMLElement).closest(".dv-tab")) continue;
      const full = el.getBoundingClientRect();
      const v = visiblePart(full, el);
      if (v) leaves.push({ el, rect: v, full, text: false });
    }
    // A dropdown shows its whole value; a placeholder is readable in full.
    for (const el of root.querySelectorAll<HTMLSelectElement>("select")) {
      if (el.closest(SKIP) || !shown(el) || el.multiple || el.size > 1 || !el.selectedOptions[0]) continue;
      const probe = el.cloneNode(false) as HTMLSelectElement;
      probe.append(el.selectedOptions[0].cloneNode(true));
      probe.style.cssText = "position:absolute;visibility:hidden;width:auto;min-width:0;max-width:none;flex:none";
      el.parentElement!.append(probe);
      const need = probe.getBoundingClientRect().width;
      probe.remove();
      if (need > el.getBoundingClientRect().width + 1) issues.add(`dropdown cuts its value (${Math.round(need - el.getBoundingClientRect().width)}px): ${label(el.selectedOptions[0])} in ${label(el).split(" ")[0]}`);
    }
    const ctx = document.createElement("canvas").getContext("2d")!;
    for (const el of root.querySelectorAll<HTMLInputElement>("input[placeholder]:placeholder-shown")) {
      if (el.closest(SKIP) || !shown(el) || css(el).textOverflow === "ellipsis") continue;
      const s = css(el);
      ctx.font = `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
      const room = el.clientWidth - parseFloat(s.paddingLeft) - parseFloat(s.paddingRight);
      if (ctx.measureText(el.placeholder).width > room + 1) issues.add(`placeholder cut off: input "${el.placeholder}"`);
    }
    // Panels and dialogs must not scroll sideways.
    for (const el of [root, ...root.querySelectorAll("*")]) {
      if (el.closest(SKIP) || el.closest(SIDEWAYS_OK) || !shown(el)) continue;
      if (/auto|scroll/.test(css(el).overflowX) && el.scrollWidth > el.clientWidth + 1)
        issues.add(`scrolls sideways (${el.scrollWidth - el.clientWidth}px): ${label(el).slice(0, 60)}`);
    }
  }

  // Nothing may sit on top of anything else (an element and what it contains are one thing).
  const positioned = (el: Element) => {
    for (let a: Element | null = el; a; a = a.parentElement) if (/absolute|fixed|sticky/.test(css(a).position)) return a;
    return undefined;
  };
  for (let i = 0; i < leaves.length; i++) {
    for (let j = i + 1; j < leaves.length; j++) {
      const a = leaves[i], b = leaves[j];
      if (a.el === b.el || a.el.contains(b.el) || b.el.contains(a.el)) continue;
      const w = Math.min(a.rect.right, b.rect.right) - Math.max(a.rect.left, b.rect.left);
      const h = Math.min(a.rect.bottom, b.rect.bottom) - Math.max(a.rect.top, b.rect.top);
      if (w <= 2 || h <= 2) continue;
      if (positioned(a.el) !== positioned(b.el)) continue; // overlays (menus, popovers) cover content by design
      issues.add(`overlap: ${label(a.el)} ⟷ ${label(b.el)}`);
    }
  }

  // Controls side by side in a row share their vertical centre.
  const controls = leaves.filter((l) => !l.text && !l.el.matches("textarea"));
  for (const a of controls)
    for (const b of controls) {
      const gap = b.full.left - a.full.right;
      if (a === b || gap < -1 || gap > 24 || positioned(a.el) !== positioned(b.el)) continue;
      if (Math.min(a.full.bottom, b.full.bottom) - Math.max(a.full.top, b.full.top) <= 0) continue;
      const dy = Math.abs((a.full.top + a.full.bottom) / 2 - (b.full.top + b.full.bottom) / 2);
      if (dy > 2) issues.add(`controls not aligned (${dy.toFixed(1)}px): ${label(a.el)} ↔ ${label(b.el)}`);
    }
  // A label beside a one-line control shares its vertical centre. The label is the row's slot that
  // holds the text, so a title with a description underneath is centred as one block.
  for (const t of leaves.filter((l) => l.text)) {
    for (const c of leaves.filter((l) => !l.text)) {
      const ch = c.full.bottom - c.full.top;
      if (ch > 44 || c.el.matches("textarea") || positioned(t.el) !== positioned(c.el)) continue;
      const gap = c.full.left - t.full.right;
      if (gap < -1 || gap > 240) continue;
      if (Math.min(t.full.bottom, c.full.bottom) - Math.max(t.full.top, c.full.top) <= 0) continue;
      let slot: Element = t.el;
      while (slot.parentElement && !slot.parentElement.contains(c.el)) slot = slot.parentElement;
      const row = slot.parentElement;
      if (!row || slot.querySelector(CONTROL)) continue;
      const s = slot.getBoundingClientRect();
      if (row.getBoundingClientRect().height > Math.max(s.height, ch) + 14) continue; // not the same row
      const dy = Math.abs((s.top + s.bottom) / 2 - (c.full.top + c.full.bottom) / 2);
      if (dy > 3.5) issues.add(`misaligned label (${dy.toFixed(1)}px): ${label(slot)} ↔ ${label(c.el)}`);
    }
  }
  return [...issues];
}

/**
 * Throws with every layout defect in `scope` at the default and the largest text size, then puts
 * the text size back. Call it wherever a test has a panel or dialog showing realistic content.
 */
export async function expectTidyLayout(page: import("@playwright/test").Page, scope = "body") {
  const root = await page.evaluate(() => document.documentElement.style.getPropertyValue("--text-scale"));
  const found: string[] = [];
  for (const scale of [1, 1.5]) {
    await page.evaluate((s) => document.documentElement.style.setProperty("--text-scale", String(s)), scale);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    for (const issue of await page.evaluate(auditLayout, scope)) found.push(`${scale * 100}% text: ${issue}`);
    // CAPTURE_LAYOUT=<folder> saves what was checked, for a review by eye.
    const dir = process.env.CAPTURE_LAYOUT;
    if (dir) await page.locator(scope).first().screenshot({ path: `${dir}/${new URL(page.url()).hostname}-${Date.now()}-${scope.replace(/\W+/g, "")}-${scale * 100}.png` });
  }
  await page.evaluate((s) => document.documentElement.style.setProperty("--text-scale", s || "1"), root);
  if (found.length) throw new Error(`Layout defects in ${scope}:\n  ${found.join("\n  ")}`);
}
