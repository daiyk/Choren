import { expect, test, type Page } from "@playwright/test";

const flow = (name: string, boxes: string, extra = "") =>
  `<ChoregrapheProject><Box id="-1" name="${name}"><Timeline enable="0"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Start" index="1"><Diagram>${boxes}${extra}</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>`;
const box = (id: number, name: string, x = 140, y = 120) =>
  `<Box id="${id}" name="${name}" x="${x}" y="${y}" tooltip="First&#xA;Second"><Input id="1" name="onStart" type="1" nature="2"/><bitmap>missing.png</bitmap></Box>`;

/** Exercise the actual file adapter and UI with deterministic IPC; OS scopes are tested natively. */
async function bridge(
  page: Page,
  files: Record<string, string>,
  picks: string[],
  delayed?: string,
) {
  await page.addInitScript(
    ({ files, picks, delayed }) => {
      let rid = 0;
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => ++rid,
          invoke: async (
            command: string,
            args: { path?: string; options?: { directory?: boolean } },
          ) => {
            if (command === "plugin:menu|new") return [++rid, String(rid)];
            if (
              command.startsWith("plugin:menu|") ||
              command === "plugin:resources|close" ||
              command === "authorize_project_root"
            )
              return;
            if (command === "plugin:dialog|open") return picks.shift() ?? null;
            const read = async (path: string) => {
              if (path === delayed)
                await new Promise((resolve) => {
                  Object.assign(window, { finishRead: resolve });
                });
              if (!(path in files)) throw new Error(`Missing ${path}`);
              return new TextEncoder().encode(files[path]);
            };
            if (command === "plugin:fs|exists") return args.path! in files;
            if (command === "plugin:fs|read_file")
              return [...(await read(args.path!))];
            // Stand-in for the Rust backend: the preview parser produces the same document shape.
            if (command === "project_open") {
              const { openProject } = await import(
                /* @vite-ignore */ `${location.origin}/src/format/project.ts`
              );
              const doc = await openProject(
                args.path!,
                read,
                async (p: string) => p in files,
              );
              return { session: ++rid, ...doc };
            }
            if (command === "project_close") return;
            throw new Error(`Unexpected IPC: ${command}`);
          },
        },
      });
    },
    { files, picks, delayed },
  );
  await page.goto("/");
}
const open = (page: Page) =>
  page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
const palette = async (page: Page, title: string) => {
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(title);
  await page.getByRole("button", { name: title, exact: false }).last().click();
};

test("file import keeps the previous project on failure and handles empty projects", async ({
  page,
}) => {
  const root = "D:/Projects/中文 Demo";
  await bridge(
    page,
    {
      [`${root}/demo.pml`]:
        '<Package name="Unicode Demo"><BehaviorDescriptions><BehaviorDescription src="Hello" xar="behavior.xar"/></BehaviorDescriptions><Resources><File src="missing.wav"/></Resources></Package>',
      [`${root}/Hello/behavior.xar`]: flow(
        "Hello",
        box(1, "Unicode ✓"),
        '<Link outputowner="999" indexofoutput="1" inputowner="1" indexofinput="1"/>',
      ),
      "/empty.pml": '<Package name="Empty"><BehaviorDescriptions/></Package>',
    },
    [`${root}/demo.pml`, "/broken.xar", "/empty.pml"],
  );
  await open(page);
  await expect(page.locator(".react-flow__node-box")).toHaveCount(1);
  await expect(page.locator(".project-heading")).toContainText("Unicode Demo");
  await page.getByRole("button", { name: "2 problems", exact: true }).click();
  await expect(page.locator(".problems")).toContainText(
    "Referenced file is missing",
  );
  await expect(page.locator(".problems")).toContainText("missing box or port");
  await open(page);
  await expect(page.locator(".notice-error")).toContainText(
    "Missing /broken.xar",
  );
  await expect(page.locator(".project-heading")).toContainText("Unicode Demo");
  await open(page);
  await expect(page.locator(".project-heading")).toContainText("Empty");
  await expect(page.locator(".react-flow__node-box")).toHaveCount(0);
  await expect(page.locator(".inspector")).not.toContainText("Unicode ✓");
});

test("a slow open cannot replace a newer project and reopening resets graph view", async ({
  page,
}) => {
  await bridge(
    page,
    { "/slow.xar": flow("Slow", box(1, "Old box")) },
    ["/slow.xar"],
    "/slow.xar",
  );
  await open(page);
  await expect(
    page.getByRole("button", { name: "Opening…", exact: true }),
  ).toBeDisabled();
  await page
    .getByRole("button", { name: "Explore example", exact: true })
    .click();
  await page.evaluate(() =>
    (window as unknown as { finishRead: () => void }).finishRead(),
  );
  await expect(page.locator(".project-heading")).toContainText(
    "Welcome to Choren",
  );
  await expect(page.locator(".react-flow__node-box")).toHaveCount(4);
  await page.getByRole("button", { name: "Zoom in", exact: true }).click();
  await palette(page, "Explore example project");
  await expect(page.getByLabel("Zoom", { exact: true })).not.toHaveText("100%");
  await expect(page.locator(".inspector h2")).toHaveText("Project overview");
});

test("large graphs virtualize and long timelines stay bounded after resize, scroll and zoom", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const boxes = Array.from({ length: 500 }, (_, i) =>
    box(i + 1, `Step ${i + 1}`, (i % 25) * 200, Math.floor(i / 25) * 140),
  ).join("");
  const keys = Array.from(
    { length: 160000 },
    (_, i) => `<Key frame="${i * 6 + 1}"/>`,
  ).join("");
  const timeline = `<Box id="501" name="Long timeline" x="0" y="3000"><Timeline enable="1" size="1000000" fps="25"><BehaviorLayer name="Behavior"><BehaviorKeyframe name="Start" index="1"><Diagram>${box(1, "Start")}</Diagram></BehaviorKeyframe><BehaviorKeyframe name="Later" index="500000"><Diagram>${box(1, "Later")}</Diagram></BehaviorKeyframe></BehaviorLayer><ActuatorList><ActuatorCurve actuator="HeadYaw">${keys}</ActuatorCurve></ActuatorList></Timeline></Box>`;
  await bridge(page, { "/large.xar": flow("Large", boxes + timeline) }, [
    "/large.xar",
  ]);
  await open(page);
  await expect(page.locator(".project-heading")).toContainText("large", {
    timeout: 15000,
  });
  await page.getByRole("tab", { name: "Outline", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Find behaviors and boxes" })
    .fill("Step 250");
  await page.locator(".search-result").click();
  await expect(page.locator(".inspector h2")).toHaveText("Step 250");
  await expect
    .poll(() => page.locator(".react-flow__node-box").count())
    .toBeLessThan(100);
  await page
    .getByRole("textbox", { name: "Find behaviors and boxes" })
    .fill("Long timeline");
  await page.locator(".search-result").first().click();
  await page
    .locator(".react-flow__node-box")
    .filter({ hasText: "Long timeline" })
    .dblclick();
  await expect(
    page.getByRole("spinbutton", { name: "Current frame" }),
  ).toHaveAttribute("max", "1000000");
  await page
    .getByRole("combobox", { name: "Timeline keyframe" })
    .selectOption("1");
  await expect(page.locator(".react-flow__node-box")).toContainText("Later");
  await page.setViewportSize({ width: 1024, height: 640 });
  await page
    .getByRole("button", { name: "Timeline zoom in", exact: true })
    .click();
  const slider = page.getByRole("slider", {
    name: "Timeline horizontal position",
  });
  await slider.fill("5000000");
  const canvas = page.locator(".timeline-viewport canvas");
  const bounds = await canvas.evaluate((c: HTMLCanvasElement) => ({
    width: c.width,
    height: c.height,
    rect: c.getBoundingClientRect().toJSON(),
    dpr: devicePixelRatio,
  }));
  expect(bounds.width).toBeLessThanOrEqual(
    Math.ceil(bounds.rect.width * bounds.dpr) + 1,
  );
  expect(bounds.height).toBeLessThanOrEqual(
    Math.ceil(bounds.rect.height * bounds.dpr) + 1,
  );
  await canvas.click({ position: { x: 184, y: 40 } });
  await expect(
    page.getByRole("spinbutton", { name: "Current frame" }),
  ).toHaveValue("500005");
  expect(errors).toEqual([]);
});

for (const style of ["choren", "classic"]) {
  test(`${style} behavior switches hide retained graphs and preserve their viewport`, async ({ page }) => {
    await bridge(page, {
      "/switch.pml": '<Package name="Switching"><BehaviorDescriptions><BehaviorDescription src="First" xar="behavior.xar"/><BehaviorDescription src="Second" xar="behavior.xar"/></BehaviorDescriptions></Package>',
      "/First/behavior.xar": flow("First", box(1, "First box")),
      "/Second/behavior.xar": flow("Second", box(1, "Second box")),
    }, ["/switch.pml"]);
    await open(page);
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption(style);
    await page.getByRole("button", { name: "Close dialog" }).click();
    const first = page.locator('.graph-document').filter({ has: page.locator('.breadcrumbs button').filter({ hasText: /^First$/ }) });
    const second = page.locator('.graph-document').filter({ has: page.locator('.breadcrumbs button').filter({ hasText: /^Second$/ }) });
    await expect(first).toBeVisible();
    await page.getByRole("button", { name: "Zoom in", exact: true }).click();
    await expect.poll(async () => {
      const before = await first.locator('.react-flow__viewport').getAttribute('style');
      await new Promise((resolve) => setTimeout(resolve, 180));
      return before === await first.locator('.react-flow__viewport').getAttribute('style');
    }).toBe(true);
    const viewport = await first.locator('.react-flow__viewport').getAttribute('style');
    for (let i = 0; i < 3; i++) {
      await page.locator('.file-tree button[title="Second/behavior.xar"]').click();
      await expect(second.locator('.react-flow__node-box')).toBeVisible();
      await expect(first.locator('.react-flow__node-box')).toBeHidden();
      await expect(first.locator('.react-flow__handle').first()).toBeHidden();
      await expect(page.locator('.react-flow__node-box:visible')).toHaveCount(1);
      await page.getByRole('tab', { name: 'First', exact: true }).click();
      await expect(first.locator('.react-flow__node-box')).toBeVisible();
      await expect(second.locator('.react-flow__node-box')).toBeHidden();
      await expect(first.locator('.react-flow__viewport')).toHaveAttribute('style', viewport!);
    }
    // Both graphs remain visible when their tabs are placed in separate groups.
    await page.getByRole("button", { name: "Actions for First", exact: true }).click();
    await page.getByRole("combobox", { name: "Destination panel group" }).selectOption({ label: "First / Second" });
    await page.getByRole("combobox", { name: "Panel placement" }).selectOption("right");
    await page.getByRole("button", { name: "Move panel", exact: true }).click();
    await first.getByRole("button", { name: "Fit graph", exact: true }).click();
    await second.getByRole("button", { name: "Fit graph", exact: true }).click();
    await expect(first.locator('.react-flow__node-box')).toBeVisible();
    await expect(second.locator('.react-flow__node-box')).toBeVisible();
    await expect(page.locator('.react-flow__node-box:visible')).toHaveCount(2);
  });
}
