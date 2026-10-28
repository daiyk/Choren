import { expect, test, type Page } from "@playwright/test";
import { expectTidyLayout } from "./layout-audit";
const node = (page: Page, name: string) =>
  page.locator(".react-flow__node-box").filter({
    has: page.locator(`svg > title`, { hasText: new RegExp(`^${name}$`) }),
  });
async function example(page: Page) {
  await page.goto("/");
  await page
    .getByRole("button", { name: "Explore example", exact: true })
    .click();
  await expect(page.locator(".react-flow__node-box")).toHaveCount(4);
  await expect(page.locator(".react-flow__viewport")).not.toHaveAttribute(
    "style",
    /translate\(0px, 0px\) scale\(1\)/,
  );
}
async function command(page: Page, title: string) {
  await page
    .getByRole("button", { name: "Search commands", exact: false })
    .click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(title);
  await page.getByRole("button", { name: title, exact: false }).last().click();
}
test("read-only context menus expose navigation and explain disabled edits", async ({ page }) => {
  await example(page);
  for (const style of ["choren", "classic"]) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption(style);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await node(page, "Say hello").click({ button: "right" });
    const menu = page.getByRole("menu", { name: "Diagram actions" });
    await expect(menu.getByRole("menuitem", { name: "Open Say hello" })).toBeEnabled();
    await expect(menu.getByRole("menuitem", { name: "Properties", exact: true })).toBeEnabled();
    await expect(menu.getByRole("menuitem", { name: "Delete box" })).toBeDisabled();
    await expect(menu.getByRole("menuitem", { name: "Delete box" })).toHaveAttribute("title", /read-only/);
    await page.keyboard.press("Escape");
    await page.locator(".react-flow__edge").first().focus();
    await page.keyboard.press("Shift+F10");
    await expect(menu.getByRole("menuitem")).toHaveText(["Delete connection"]);
    await expect(menu.getByRole("menuitem")).toBeDisabled();
    await expect(menu).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
  }
});

test("Graph UI preference applies to nested diagrams and recovers from an invalid saved value", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("choren.graph-style", '"obsolete"'));
  await example(page);
  await expect(page.getByTestId("graph")).toHaveAttribute("data-graph-style", "choren");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption("classic");
  await page.getByRole("button", { name: "Close dialog" }).click();
  await node(page, "Sequence").dblclick();
  await expect(node(page, "Nested step").locator(".box-classic")).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption("choren");
  await page.getByRole("button", { name: "Close dialog" }).click();
  await expect(node(page, "Nested step").locator(".box-card")).toBeVisible();
  await page.getByRole("navigation", { name: "Diagram path" }).getByRole("button", { name: "Welcome behavior", exact: true }).click();
  await expect(page.locator(".box-card")).toHaveCount(4);
});
test("read-only graph, XML descriptions and nested viewport restoration", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await example(page);
  await expect(page.locator(".react-flow__handle.connectable")).toHaveCount(0);
  const disabledPort = page.locator('[data-id="rail-left"]').getByRole("button", { name: "Connect from onStart" });
  await expect(disabledPort).toHaveAttribute("aria-disabled", "true");
  await expect(disabledPort).toHaveAttribute("tabindex", "-1");
  await node(page, "Say hello").click();
  await expect(page.locator(".inspector h2")).toHaveText("Say hello");
  await expect(page.locator(".inspector .description")).toHaveText(
    "A simple script box.\nDouble-click to inspect its Python source.",
  );
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Delete");
  await expect(page.locator(".react-flow__node-box")).toHaveCount(4);
  await page.getByRole("button", { name: "Zoom in", exact: true }).click();
  await expect(page.getByLabel("Zoom", { exact: true })).not.toHaveText("100%");
  // The zoom button animates for 120 ms; compare the settled viewport.
  await expect
    .poll(async () => {
      const first = await page.getByLabel("Zoom", { exact: true }).innerText();
      await new Promise((resolve) => setTimeout(resolve, 180));
      return (
        first === (await page.getByLabel("Zoom", { exact: true }).innerText())
      );
    })
    .toBe(true);
  const zoom = await page.getByLabel("Zoom", { exact: true }).innerText();
  const graphBounds = (await page.getByTestId("graph").boundingBox())!;
  const leftRail = (await page.locator('[data-id="rail-left"]').boundingBox())!;
  const rightRail = (await page
    .locator('[data-id="rail-right"]')
    .boundingBox())!;
  expect(Math.abs(leftRail.x - graphBounds.x)).toBeLessThan(1);
  expect(
    Math.abs(rightRail.x + rightRail.width - graphBounds.x - graphBounds.width),
  ).toBeLessThan(1);
  const source = await page
    .locator(".react-flow__edge path.react-flow__edge-path")
    .first()
    .evaluate((path: SVGPathElement) => {
      const point = path
        .getPointAtLength(path.getTotalLength())
        .matrixTransform(path.getScreenCTM()!);
      return { x: point.x, y: point.y };
    });
  expect(Math.abs(source.x - (leftRail.x + 21))).toBeLessThan(1);
  expect(Math.abs(source.y - (leftRail.y + 24))).toBeLessThan(1);
  await node(page, "Sequence").dblclick();
  await expect(node(page, "Nested step")).toBeVisible();
  await expect(page.locator(".inspector h2")).toHaveText("Project overview");
  await node(page, "Nested step").click();
  await expect(page.locator(".inspector h2")).toHaveText("Nested step");
  await page
    .getByRole("navigation", { name: "Diagram path" })
    .getByRole("button", { name: "Welcome behavior", exact: true })
    .click();
  await expect(page.locator(".react-flow__node-box")).toHaveCount(4);
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText(zoom);
  expect(errors).toEqual([]);
});
test("script tabs remain read-only and selection follows tab activation", async ({
  page,
}) => {
  await example(page);
  await node(page, "Say hello").dblclick();
  await expect(page.locator(".cm-content")).toContainText("class Greeting");
  await page.locator(".cm-content").click();
  await page.keyboard.type("MUTATION");
  await expect(page.locator(".cm-content")).not.toContainText("MUTATION");
  await page
    .getByRole("tab", { name: "Welcome behavior", exact: true })
    .click();
  await node(page, "Sequence").click();
  await expect(page.locator(".inspector h2")).toHaveText("Sequence");
  await page.getByRole("tab", { name: "Say hello.py", exact: true }).click();
  await expect(page.locator(".inspector h2")).toHaveText("Say hello");
});
test("timeline uses bounded canvas and correct keyframe navigation", async ({
  page,
}) => {
  await example(page);
  await node(page, "Gesture timeline").dblclick();
  await expect(
    page.getByRole("combobox", { name: "Timeline keyframe" }),
  ).toBeVisible();
  await page
    .getByRole("combobox", { name: "Timeline keyframe" })
    .selectOption("1");
  await expect(node(page, "Finish pose")).toBeVisible();
  await expect(
    page.getByRole("spinbutton", { name: "Current frame" }),
  ).toHaveValue("60");
  await page
    .getByRole("button", { name: "Timeline zoom in", exact: true })
    .click();
  const canvas = page.locator(".timeline-viewport canvas");
  const bounds = await canvas.evaluate((c: HTMLCanvasElement) => ({
    width: c.width,
    viewport: c.getBoundingClientRect().width,
    dpr: devicePixelRatio,
  }));
  expect(bounds.width).toBeLessThanOrEqual(
    Math.ceil(bounds.viewport * bounds.dpr) + 1,
  );
  await page
    .getByRole("combobox", { name: "Timeline keyframe" })
    .selectOption("0");
  await expect(node(page, "Prepare pose")).toBeVisible();
});
test("outline search reveals nested boxes and clears stale details", async ({
  page,
}) => {
  await example(page);
  await page.getByRole("tab", { name: "Outline", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Find behaviors and boxes" })
    .fill("Nested step");
  await page.locator(".search-result").click();
  await expect(node(page, "Nested step")).toBeVisible();
  await expect(page.locator(".inspector h2")).toHaveText("Nested step");
});
test("keyboard panel movement, resizing, close/reopen and persistence", async ({
  page,
}) => {
  await example(page);
  await page
    .getByRole("button", { name: "Actions for Inspector", exact: true })
    .click();
  const widthBefore = await page
    .locator(".inspector")
    .evaluate((e) => e.getBoundingClientRect().width);
  await page.getByRole("button", { name: "Wider", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(() =>
      page
        .locator(".inspector")
        .evaluate((e) => e.getBoundingClientRect().width),
    )
    .toBeGreaterThan(widthBefore);
  await page
    .getByRole("combobox", { name: "Destination panel group" })
    .selectOption({ label: "Project / Outline" });
  await page
    .getByRole("combobox", { name: "Panel placement" })
    .selectOption("center");
  await page.getByRole("button", { name: "Move panel", exact: true }).click();
  await expect(
    page.getByRole("tab", { name: "Inspector", exact: true }),
  ).toBeVisible();
  const movedX = (await page
    .getByRole("tab", { name: "Inspector", exact: true })
    .boundingBox())!.x;
  expect(movedX).toBeLessThan(400);
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("choren.layout.v1")))
    .not.toBeNull();
  await page.reload();
  await expect(
    page.getByRole("tab", { name: "Inspector", exact: true }),
  ).toBeVisible();
  expect(
    (await page
      .getByRole("tab", { name: "Inspector", exact: true })
      .boundingBox())!.x,
  ).toBeLessThan(400);
  await page
    .getByRole("button", { name: "Close Inspector", exact: true })
    .click();
  await expect(
    page.getByRole("tab", { name: "Inspector", exact: true }),
  ).toHaveCount(0);
  await command(page, "Show Inspector");
  await expect(
    page.getByRole("tab", { name: "Inspector", exact: true }),
  ).toBeVisible();
  await command(page, "Restore default layout");
  expect(
    (await page
      .getByRole("tab", { name: "Inspector", exact: true })
      .boundingBox())!.x,
  ).toBeGreaterThan(900);
});
test("resizing panel dividers does not select content or disable editor selection", async ({
  page,
}) => {
  await example(page);
  for (const orientation of ["horizontal", "vertical"]) {
    const sash = page
      .locator(`.dv-split-view-container.dv-${orientation} > .dv-sash-container > .dv-sash:visible`)
      .first();
    const before = (await sash.boundingBox())!;
    const x = before.x + before.width / 2;
    const y = before.y + before.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(
      x + (orientation === "horizontal" ? 60 : 0),
      y + (orientation === "vertical" ? 60 : 0),
      { steps: 12 },
    );
    await expect
      .poll(() => page.evaluate(() => window.getSelection()?.toString()))
      .toBe("");
    await page.mouse.up();
    await expect
      .poll(async () => {
        const after = (await sash.boundingBox())!;
        return orientation === "horizontal" ? after.x - before.x : after.y - before.y;
      })
      .toBeGreaterThan(40);
    await expect
      .poll(() => page.evaluate(() => window.getSelection()?.toString()))
      .toBe("");
  }
  await node(page, "Say hello").dblclick();
  await page.locator(".cm-line", { hasText: "class Greeting" }).dblclick();
  await expect
    .poll(() => page.evaluate(() => window.getSelection()?.toString().length ?? 0))
    .toBeGreaterThan(0);
});

test("pointer dragging rearranges panels", async ({ page }) => {
  await example(page);
  const from = await page
    .getByRole("tab", { name: "Inspector", exact: true })
    .boundingBox();
  const to = await page
    .getByRole("tab", { name: "Outline", exact: true })
    .boundingBox();
  await page.mouse.move(from!.x + 28, from!.y + 14);
  await page.mouse.down();
  await page.mouse.move(from!.x - 30, from!.y + 14, { steps: 4 });
  await page.mouse.move(to!.x + 25, to!.y + 14, { steps: 14 });
  await page.mouse.up();
  await expect
    .poll(
      async () =>
        (await page
          .getByRole("tab", { name: "Inspector", exact: true })
          .boundingBox())!.x,
    )
    .toBeLessThan(400);
});
test("themes and compact layouts do not overlap or overflow the window", async ({
  page,
  browserName,
}) => {
  await example(page);
  await node(page, "Say hello").click();
  await expect(page.locator(".inspector h2")).toHaveText("Say hello");
  if (process.env.CAPTURE_UI && browserName === "chromium")
    await page.screenshot({
      path: "../assets/screenshots/choren-studio-light.png",
    });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page
    .getByRole("combobox", { name: "Appearance", exact: true })
    .selectOption("dark");
  // Capture before the layout audit: its 150% text pass can leave dockview's tab overflow
  // chevron behind once the text shrinks back.
  if (process.env.CAPTURE_UI && browserName === "chromium") {
    await page.getByRole("button", { name: "Close dialog" }).click();
    await page.screenshot({
      path: "../assets/screenshots/choren-studio-dark.png",
    });
    await page.getByRole("button", { name: "Settings", exact: true }).click();
  }
  await expectTidyLayout(page, "dialog[open]");
  await page.getByRole("button", { name: "Close dialog" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  for (const viewport of [
    { width: 1024, height: 640 },
    { width: 1280, height: 720 },
    { width: 1920, height: 1080 },
  ]) {
    await page.setViewportSize(viewport);
    await expect(
      page.getByRole("button", { name: "Fit graph", exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(() => document.body.scrollWidth),
    ).toBeLessThanOrEqual(viewport.width);
    expect(
      await page.evaluate(() => document.body.scrollHeight),
    ).toBeLessThanOrEqual(viewport.height);
    const graph = await page.getByTestId("graph").boundingBox();
    expect(graph!.width).toBeGreaterThan(300);
    await expectTidyLayout(page);
  }
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});
test("every panel keeps its text inside its controls at default and large text sizes", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await example(page);
  await node(page, "Say hello").click();
  const titles = (await page.evaluate(() => import(`${location.origin}/src/workspace/registry.ts`).then((m) =>
    m.panels.filter((p: { available: boolean }) => p.available).map((p: { title: string }) => p.title)))) as string[];
  for (const title of titles) {
    await command(page, `Show ${title}`);
    await expect(page.getByRole("tab", { name: title, exact: true })).toBeVisible();
    await expectTidyLayout(page);
  }
});
test("corrupt saved layout recovers and unsupported commands explain availability", async ({
  page,
}) => {
  await page.addInitScript(() =>
    localStorage.setItem(
      "choren.layout.v1",
      '{"version":1,"layout":{"grid":{},"panels":{"broken":{"contentComponent":"workspace"}}}}',
    ),
  );
  await page.goto("/");
  await expect(
    page.getByRole("tab", { name: "Project", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Search commands", exact: false })
    .click();
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("Save project");
  await expect(
    page.getByRole("button", { name: /Save project/ }),
  ).toBeDisabled();
  await expect(
    page.getByText("Editing needs the desktop app; this browser preview is read-only.").first(),
  ).toBeVisible();
});

test("keyboard and mixed selection keep Inspector synchronized", async ({
  page,
}) => {
  await example(page);
  await node(page, "Say hello").click();
  await node(page, "Sequence").click({ modifiers: ["Shift"] });
  await expect(page.locator(".inspector h2")).toHaveText("2 items selected");
  const edge = page.locator(".react-flow__edge").first();
  await edge.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".inspector h2")).toHaveText("Connection");
  await page.keyboard.press("Delete");
  await expect(page.locator(".react-flow__edge")).toHaveCount(4);
  await node(page, "Say hello").click();
  await expect(page.locator(".inspector h2")).toHaveText("Say hello");
  await page.keyboard.press("Escape");
  await expect(page.locator(".inspector h2")).toHaveText("Project overview");
});

test("F6 cycles keyboard focus between panel groups", async ({ page }) => {
  await example(page);
  await page
    .getByRole("tab", { name: "Welcome behavior", exact: true })
    .focus();
  const focusedGroup = () =>
    page.evaluate(() =>
      document.activeElement
        ?.closest(".dv-groupview")
        ?.getAttribute("aria-label"),
    );
  const before = await focusedGroup();
  await page.keyboard.press("F6");
  await expect.poll(focusedGroup).not.toBe(before);
  expect(await focusedGroup()).toBeTruthy();
  await page.keyboard.press("Shift+F6");
  await expect.poll(focusedGroup).toBe(before);
});
