import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";

const xar = `<ChoregrapheProject><Box id="-1" name="root"><Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/><Timeline enable="0"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Start" index="1"><Diagram><Box id="1" name="Greeter" x="140" y="120" tooltip="Says hi"><script language="4"><content><![CDATA[a = 1]]></content></script><Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/><Parameter id="3" name="Text" content_type="3" value="Hi"/></Box><Link outputowner="0" indexofoutput="1" inputowner="1" indexofinput="1"/></Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>`;

/**
 * Desktop IPC stand-in: the preview parser plays the backend, with node handles assigned the way
 * the Rust view does. Edits are applied to the JSON document so the UI round trip is observable.
 */
async function backend(
  page: Page,
  source = xar,
  format: "xar" | "pml" = "xar",
) {
  await page.addInitScript(
    ({ xar, format }) => {
      let rid = 0;
      const calls: { command: string; args: unknown }[] = [];
      let doc: any, original: string;
      let dirty = false;
      let n = 100; // node handles are only unique within a behavior's document
      Object.assign(window, {
        calls,
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => ++rid,
          invoke: async (command: string, args: any) => {
            calls.push({ command, args });
            if (command === "plugin:menu|new") return [++rid, String(rid)];
            if (command === "library_tree") return [];
            if (command === "robot_status") return null;
            if (
              command.startsWith("plugin:") ||
              command === "authorize_project_root"
            ) {
              if (command === "plugin:dialog|open") return "/p/demo.xar";
              return;
            }
            const history = (undo?: string) => ({
              revision: 1,
              dirty,
              undo,
              created: [],
            });
            if (command === "project_open") {
              const { openProject } = await import(
                /* @vite-ignore */ `${location.origin}/src/format/project.ts`
              );
              doc = await openProject(args.path, async () =>
                new TextEncoder().encode(xar),
              );
              n = 100;
              const walk = (box: any) => {
                box.node = n++;
                box.params.forEach((p: any) => (p.node = n++));
                box.timeline?.layers.forEach((l: any) =>
                  l.keyframes.forEach((k: any) => {
                    if (!k.diagram) return;
                    k.diagram.node = n++;
                    k.diagram.links.forEach((link: any) => (link.node = n++));
                    k.diagram.boxes.forEach(walk);
                  }),
                );
              };
              doc.behaviors.forEach((b: any) => walk(b.root));
              if (format === "pml") {
                doc.format = format;
                doc.id = doc.source = "/p/demo.pml";
                Object.assign(doc.behaviors[0], {
                  id: "/p/demo.pml::behavior_1/behavior.xar",
                  name: "behavior_1",
                  source: "behavior_1/behavior.xar",
                });
              }
              original = JSON.stringify(doc);
              return { session: 7, ...structuredClone(doc) };
            }
            if (command === "project_add_behavior") {
              let i = 1;
              while (doc.behaviors.some((b: any) => b.name === `behavior_${i}`))
                i++;
              const behavior = JSON.parse(original).behaviors[0];
              behavior.name = args.name || `behavior_${i}`;
              behavior.root.timeline.enabled = args.root === "Timeline";
              behavior.root.timeline.node = 150;
              if (args.root === "Timeline") behavior.root.timeline.size = 100;
              behavior.source = `${behavior.name}/behavior.xar`;
              behavior.id = `${doc.id}::${behavior.source}`;
              const diagram =
                behavior.root.timeline.layers[0].keyframes[0].diagram;
              diagram.boxes = [];
              diagram.links = [];
              doc.behaviors.push(behavior);
              n = 200; // recreating a behavior also recreates its node arena
              dirty = true;
              return { history: history(), document: structuredClone(doc) };
            }
            if (command === "project_removal_files") return [];
            if (command === "project_remove_behavior") {
              if ((window as any).failRemoval)
                throw new Error("Removal failed");
              doc.behaviors = doc.behaviors.filter(
                (b: any) => b.id !== args.behavior,
              );
              dirty = true;
              return { history: history(), document: structuredClone(doc) };
            }
            const behavior =
              doc.behaviors.find((b: any) => b.id === args?.behavior) ??
              doc.behaviors[0];
            const diagram = () =>
              behavior.root.timeline.layers[0].keyframes[0].diagram;
            const find = (node: number) =>
              diagram().boxes.find(
                (b: any) =>
                  b.node === node || b.params.some((p: any) => p.node === node),
              );
            if (command === "project_edit") {
              if ((window as any).holdEdits)
                await new Promise<void>((resolve) => { (window as any).releaseEdit = resolve; });
              if ((window as any).failEdit) throw new Error("Save position failed");
              for (const c of args.commands) {
                const box = c.node !== undefined ? find(c.node) : undefined;
                if (c.type === "setAttributes" && box?.node === c.node)
                  Object.assign(box, {
                    name: c.values.name ?? box.name,
                    tooltip: c.values.tooltip ?? box.tooltip,
                    category: "choren_category" in c.values ? c.values.choren_category ?? undefined : box.category,
                  });
                if (c.type === "setAttributes" && box && box.node !== c.node)
                  box.params.find((p: any) => p.node === c.node).value =
                    c.values.value;
                if (c.type === "setScript") find(c.box).script = c.text;
                if (c.type === "addPort") {
                  const owner = find(c.box);
                  const id = Math.max(...[...owner.inputs, ...owner.outputs, ...owner.params].map((p: any) => p.id)) + 1;
                  owner[c.kind === "Input" ? "inputs" : "outputs"].push({
                    node: n++, id, name: c.name, type: 1, nature: 2, inner: false, tooltip: "",
                  });
                }
                if (c.type === "moveBoxes")
                  for (const [node, x, y] of c.moves)
                    Object.assign(find(node), { x, y });
                if (c.type === "addLink")
                  diagram().links.push({
                    node: n++,
                    outOwner: c.outOwner,
                    outPort: c.outPort,
                    inOwner: c.inOwner,
                    inPort: c.inPort,
                  });
                if (c.type === "newBox") {
                  const used = diagram().boxes.map((b: any) => b.id);
                  let id = 1;
                  while (used.includes(id)) id++;
                  diagram().boxes.push({
                    node: n++,
                    id,
                    name: c.name,
                    x: c.x,
                    y: c.y,
                    tooltip: "",
                    bitmaps: [],
                    script: "",
                    plugin: "",
                    inputs: [],
                    outputs: [],
                    params: [],
                    resources: [],
                  });
                }
                if (c.type === "remove") {
                  diagram().boxes = diagram().boxes.filter(
                    (b: any) => !c.nodes.includes(b.node),
                  );
                  diagram().links = diagram().links.filter(
                    (l: any) =>
                      !c.nodes.includes(l.node) &&
                      diagram().boxes.some((b: any) => b.id === l.inOwner),
                  );
                }
              }
              dirty = true;
              return {
                history: history(args.label),
                document: structuredClone(doc),
              };
            }
            if (command === "project_undo") {
              doc = JSON.parse(original);
              dirty = false;
              return { history: history(), document: structuredClone(doc) };
            }
            if (command === "project_save") {
              dirty = false;
              return history();
            }
            if (command === "project_close") return;
            throw new Error(`Unexpected IPC: ${command}`);
          },
        },
      });
    },
    { xar: source, format },
  );
  await page.goto("/");
}
const calls = (page: Page, command: string) =>
  page.evaluate(
    (c) =>
      (window as any).calls
        .filter((x: any) => x.command === c)
        .map((x: any) => x.args),
    command,
  );

async function graphUI(page: Page, value: "choren" | "classic") {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption(value);
  await page.getByRole("button", { name: "Close dialog" }).click();
  await expect(page.getByTestId("graph")).toHaveAttribute("data-graph-style", value);
}

test("connection context deletion is undoable without removing its box", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator(".react-flow__node-box");
  await expect(box).toHaveCount(1);
  await box.click();
  const edge = page.locator(".react-flow__edge").first();
  await edge.focus();
  await page.keyboard.press("Shift+F10");
  await page.getByRole("menuitem", { name: "Delete connection" }).click();
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
  await expect(box).toHaveCount(1);
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill("Undo");
  await page.getByRole("button", { name: /Undo Delete selection in diagram/ }).click();
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  await expect(box).toHaveCount(1);
});

test("Graph UI switches preserve selection, positions and wires and persist after reload", async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (/new nodeTypes|not initialized|Couldn't create edge/.test(message.text())) errors.push(message.text());
  });
  await backend(page);
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator(".react-flow__node-box");
  await box.click();
  const original = (await box.boundingBox())!;
  const viewport = await page.locator(".react-flow__viewport").getAttribute("style");
  const edge = page.locator(".react-flow__edge-path");
  const path = await edge.getAttribute("d");
  const source = page.locator('[data-id="rail-left"]').getByRole("button", { name: "Connect from onStart" });
  await source.focus();
  await page.keyboard.press("Enter");
  await expect(source).toHaveClass(/clickconnecting/);
  for (const style of ["classic", "choren", "classic"] as const) {
    await graphUI(page, style);
    await expect(page.locator(".clickconnecting")).toHaveCount(0);
    await expect(box).toHaveClass(/selected/);
    await expect(page.getByRole("textbox", { name: "Box name", exact: true })).toHaveValue("Greeter");
    await expect(page.locator(".react-flow__viewport")).toHaveAttribute("style", viewport!);
    const bounds = (await box.boundingBox())!;
    expect(bounds.x).toBeCloseTo(original.x, 0);
    expect(bounds.y).toBeCloseTo(original.y, 0);
    expect(bounds.width).toBeCloseTo(original.width * (style === "choren" ? 1 : 90 / 176), 0);
    await expect.poll(async () => {
      const endpoint = await edge.evaluate((path: SVGPathElement) => {
        const point = path.getPointAtLength(0).matrixTransform(path.getScreenCTM()!);
        return { x: point.x, y: point.y };
      });
      const anchor = await box.locator('[data-handleid="i1"]').evaluate((el) => {
        const rect = el.getBoundingClientRect();
        return { x: rect.x, y: rect.y };
      });
      return Math.hypot(endpoint.x - anchor.x, endpoint.y - anchor.y);
    }).toBeLessThan(1);
    if (style === "choren") await expect(edge).toHaveAttribute("d", path!);
  }
  expect(await calls(page, "project_edit")).toEqual([]);
  if (process.env.CAPTURE_UI) await page.getByTestId("graph").screenshot({ path: testInfo.outputPath("classic-ui.png") });
  await page.reload();
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.getByTestId("graph")).toHaveAttribute("data-graph-style", "classic");
  await expect(box.locator(".box-classic")).toHaveCount(1);
  await graphUI(page, "choren");
  await expect(box.locator(".box-card")).toHaveCount(1);
  expect(errors).toEqual([]);
});

test("Classic UI supports painted ports, keyboard linking, parameters and dragging", async ({ page }) => {
  await backend(page, xar.replace(/<Link[^>]*\/>/, ""));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await graphUI(page, "classic");
  const box = page.locator(".react-flow__node-box");
  const bounds = (await box.boundingBox())!;
  const left = (await page.locator('[data-id="rail-left"]').boundingBox())!;
  const right = (await page.locator('[data-id="rail-right"]').boundingBox())!;
  const zoom = bounds.width / 90;
  await page.mouse.move(left.x + 10, left.y + 25);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 6 * zoom, bounds.y + 34 * zoom, { steps: 10 });
  await page.mouse.up();
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  await page.mouse.click(right.x + 10, right.y + 25);
  await page.mouse.click(bounds.x + 100 * zoom, bounds.y + 12 * zoom);
  await expect(page.locator(".react-flow__edge")).toHaveCount(2);
  await page.getByRole("button", { name: "Parameters for Greeter" }).click();
  await expect(page.getByRole("textbox", { name: "Text value" })).toHaveValue("Hi");
  await page.getByRole("button", { name: "Input", exact: true }).click();
  const source = page.locator('[data-id="rail-left"]').getByRole("button", { name: "Connect from onStart" });
  const input = box.locator('[data-handleid="i4"]');
  await expect(input).toHaveCount(1);
  await source.focus();
  await page.keyboard.press("Enter");
  await input.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".react-flow__edge")).toHaveCount(3);
  const edge = page.locator(".react-flow__edge-path").first();
  const path = await edge.getAttribute("d");
  await page.mouse.move(bounds.x + 40 * zoom, bounds.y + 10 * zoom);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 40 * zoom + 60, bounds.y + 10 * zoom + 40, { steps: 10 });
  await page.mouse.up();
  await expect(edge).not.toHaveAttribute("d", path!);
  expect((await calls(page, "project_edit")).at(-1).commands[0].type).toBe("moveBoxes");
  await box.dblclick();
  await expect(page.locator(".cm-content")).toContainText("a = 1");
});

test("visible port anchors connect an editable graph", async ({ page }) => {
  await backend(page, xar.replace(/<Link[^>]*\/>/, ""));
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  const rail = page.locator('[data-id="rail-left"]');
  const box = page.locator(".react-flow__node-box");
  await expect(box).toBeVisible();
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText("125%");
  const from = (await rail.boundingBox())!;
  const to = (await box.boundingBox())!;
  const zoom = to.width / 176;
  // Use the painted button centers, not the link anchors outside their squares.
  await page.mouse.move(from.x + 10, from.y + 25);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y + 68 * zoom, { steps: 10 });
  await page.mouse.up();
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  const endpoints = await page.locator(".react-flow__edge-path").evaluate((path: SVGPathElement) => {
    const transform = path.getScreenCTM()!;
    const start = path.getPointAtLength(0).matrixTransform(transform);
    const end = path.getPointAtLength(path.getTotalLength()).matrixTransform(transform);
    return { start: { x: start.x, y: start.y }, end: { x: end.x, y: end.y } };
  });
  expect(endpoints.start.x).toBeCloseTo(to.x - 10 * zoom, 0);
  expect(endpoints.start.y).toBeCloseTo(to.y + 68 * zoom, 0);
  expect(endpoints.end.x).toBeCloseTo(from.x + 22, 0);
  expect(endpoints.end.y).toBeCloseTo(from.y + 24, 0);
});

test("painted port buttons connect by clicking in either direction", async ({ page }) => {
  await backend(page, xar.replace(/<Link[^>]*\/>/, ""));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText("125%");
  const box = (await page.locator(".react-flow__node-box").boundingBox())!;
  const left = (await page.locator('[data-id="rail-left"]').boundingBox())!;
  const right = (await page.locator('[data-id="rail-right"]').boundingBox())!;
  const zoom = box.width / 176;
  await page.mouse.click(left.x + 10, left.y + 25);
  await expect(page.locator(".clickconnecting")).toHaveCount(1);
  await page.mouse.click(box.x, box.y + 68 * zoom);
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  // Begin at the destination: root output, then the box's output.
  await page.mouse.click(right.x + 10, right.y + 25);
  await page.mouse.click(box.x + 176 * zoom, box.y + 68 * zoom);
  await expect(page.locator(".react-flow__edge")).toHaveCount(2);
  const edits = await calls(page, "project_edit");
  expect(edits.map((edit: any) => edit.commands[0])).toMatchObject([
    { type: "addLink", outOwner: 0, outPort: 1, inOwner: 1, inPort: 1 },
    { type: "addLink", outOwner: 1, outPort: 2, inOwner: 0, inPort: 2 },
  ]);
});

test("port keyboard connections support Escape and canvas cancellation", async ({ page }) => {
  await backend(page, xar.replace(/<Link[^>]*\/>/, ""));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText("125%");
  const source = page.locator('[data-id="rail-left"]').getByRole("button", { name: "Connect from onStart" });
  const target = page.locator(".react-flow__node-box").getByRole("button", { name: "Connect to onStart" });
  await source.focus();
  await page.keyboard.press("Enter");
  await expect(source).toHaveClass(/clickconnecting/);
  await page.keyboard.press("Escape");
  await expect(page.locator(".clickconnecting")).toHaveCount(0);
  await page.keyboard.press("Space");
  await expect(source).toHaveClass(/clickconnecting/);
  await page.locator(".react-flow__pane").click({ position: { x: 100, y: 220 } });
  await expect(page.locator(".clickconnecting")).toHaveCount(0);
  expect(await calls(page, "project_edit")).toEqual([]);
  await source.focus();
  await page.keyboard.press("Space");
  await target.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
});

test("the wrench selects its box and reopens parameter editing", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await page.getByRole("button", { name: "Close Inspector", exact: true }).click();
  const wrench = page.getByRole("button", { name: "Parameters for Greeter" });
  await wrench.click();
  await expect(page.getByRole("textbox", { name: "Text value" })).toHaveValue("Hi");
  await page.getByRole("button", { name: "Close Inspector", exact: true }).click();
  await wrench.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("textbox", { name: "Text value" })).toHaveValue("Hi");
  await expect(page.locator(".cm-content")).toHaveCount(0);
  expect(await calls(page, "project_edit")).toEqual([]);
});

test("appearance categories survive rename and keep connections attached", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator(".react-flow__node-box");
  await box.click();
  const edge = page.locator(".react-flow__edge-path");
  const original = await edge.getAttribute("d");
  for (const category of ["subgraph", "timeline", "timer", "terminal", "script"]) {
    await page.getByRole("combobox", { name: "Appearance category" }).selectOption(category);
    await expect(box.locator(".box-card")).toHaveAttribute("data-category", category);
    await expect(box.locator(".box-stack")).toHaveCount(category === "subgraph" ? 2 : 0);
    await expect(page.locator(".inspector .category-badge")).toHaveAttribute("data-category", category);
    await expect(edge).toHaveAttribute("d", original!);
  }
  await page.getByRole("combobox", { name: "Appearance category" }).selectOption("terminal");
  await page.getByRole("textbox", { name: "Box name", exact: true }).fill("Final step");
  await page.getByRole("textbox", { name: "Box name", exact: true }).press("Enter");
  await expect(box.locator(".box-card")).toHaveAttribute("data-category", "terminal");
  await expect(box.locator(".box-title")).toHaveText("Final step");
  await page.getByRole("combobox", { name: "Appearance category" }).selectOption("");
  await expect(box.locator(".box-card")).toHaveAttribute("data-category", "script");
});

test("category cards show labels and duration in both themes", async ({ page }, testInfo) => {
  const names = ["Introduction v2", "Gesture (Anger) v2", "Wait", "Ending", "A custom script with a long title"];
  const boxes = ["subgraph", "timeline", "timer", "terminal", "script"].map((category, i) => `<Box id="${i + 1}" name="${names[i]}" choren_category="${category}" x="${140 + (i % 3) * 280}" y="${100 + Math.floor(i / 3) * 270}"><Input id="1" name="onStart" nature="2" type="1"/><Input id="3" name="onStop" nature="3" type="1"/><Output id="2" name="onStopped" nature="1" type="1"/>${category === "timer" ? '<Parameter id="4" name="Delay (s)" content_type="2" value="2.5"/>' : ""}</Box>`).join("");
  const links = '<Link outputowner="1" indexofoutput="2" inputowner="2" indexofinput="1"/><Link outputowner="2" indexofoutput="2" inputowner="3" indexofinput="1"/><Link outputowner="3" indexofoutput="2" inputowner="4" indexofinput="1"/>';
  await backend(page, xar.replace(/<Diagram>.*<\/Diagram>/, `<Diagram>${boxes}${links}</Diagram>`));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.locator(".box-card")).toHaveCount(5);
  await expect(page.locator('.box-card[data-category="timer"] .box-category text')).toHaveText("Timer · 2.5 s");
  await expect(page.locator('.box-card[data-category="script"] .box-title')).toContainText("…");
  for (const theme of ["light", "dark"]) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("combobox", { name: "Appearance", exact: true }).selectOption(theme);
    await page.getByRole("button", { name: "Close dialog" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const colors = await page.locator(".box-header").evaluateAll((headers) => headers.map((header) => getComputedStyle(header).fill));
    expect(new Set(colors).size).toBe(5);
    if (process.env.CAPTURE_UI) await page.getByTestId("graph").screenshot({ path: testInfo.outputPath(`graph-${theme}.png`) });
  }
});

test("many ports and a linked parameter stay inside the card with aligned wires", async ({ page }, testInfo) => {
  const extra = Array.from({ length: 7 }, (_, i) => `<Input id="${i + 4}" name="Long_input_name_${i}" type="2" nature="1"/>`).join("");
  const outputs = Array.from({ length: 9 }, (_, i) => `<Output id="${i + 20}" name="Long_output_name_${i}" type="2" nature="1"/>`).join("");
  await backend(page, xar.replace('<Parameter id="3"', `${extra}${outputs}<Parameter id="3"`).replace('</Diagram>', '<Link outputowner="0" indexofoutput="1" inputowner="1" indexofinput="3"/></Diagram>'));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator(".react-flow__node-box");
  await expect(box.locator(".box-port")).toHaveCount(19);
  const bounds = (await box.boundingBox())!;
  const port = (await box.locator('[data-port="i3"] .box-port-button').boundingBox())!;
  expect(port.y + port.height).toBeLessThan(bounds.y + bounds.height);
  await expect(box.locator(".box-category-art")).toHaveCount(0);
  const endpoint = await page.locator('[data-id="l1"] .react-flow__edge-path').evaluate((path: SVGPathElement) => {
    const p = path.getPointAtLength(0).matrixTransform(path.getScreenCTM()!);
    return { x: p.x, y: p.y };
  });
  expect(Math.abs(endpoint.y - (port.y + port.height / 2))).toBeLessThan(1);
  const anchor = (await box.locator('[data-handleid="i3"]').boundingBox())!;
  expect(endpoint.x).toBeCloseTo(anchor.x, 0);
  const lastOutput = (await box.locator('[data-port="o28"] .box-port-button').boundingBox())!;
  expect(lastOutput.y + lastOutput.height).toBeLessThan(bounds.y + bounds.height);
  const inputLabel = (await box.locator('[data-port="i4"] text').boundingBox())!;
  const outputLabel = (await box.locator('[data-port="o20"] text').boundingBox())!;
  expect(inputLabel.x + inputLabel.width).toBeLessThan(outputLabel.x);
  await expect(box.locator('[data-port="i4"] title')).toHaveText("Long_input_name_0");
  await expect(box.locator('[data-port="o20"] title')).toHaveText("Long_output_name_0");
  if (process.env.CAPTURE_UI) await page.getByTestId("graph").screenshot({ path: testInfo.outputPath("compact-many-ports.png") });
});

test("connections follow a dragged box through edits and undo", async ({ page }) => {
  const warnings: string[] = [];
  page.on("console", (message) => {
    if (message.text().includes("not initialized")) warnings.push(message.text());
  });
  await backend(page, xar.replace("</Diagram>", `<Box id="2" name="Waver" x="340" y="200"><Input id="1" name="onStart" type="1" nature="2"/></Box><Link outputowner="1" indexofoutput="2" inputowner="2" indexofinput="1"/></Diagram>`));
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator('.react-flow__node-box[data-id="1"]');
  const edges = page.locator(".react-flow__edge-path");
  const edge = page.locator('[data-id="l1"] .react-flow__edge-path');
  await expect(edge).toHaveCount(1);
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText("125%");
  const original = await edge.getAttribute("d");
  for (let i = 0; i < 2; i++) {
    const bounds = (await box.boundingBox())!;
    await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + 12);
    await page.mouse.down();
    await page.mouse.move(bounds.x + bounds.width / 2 + 60, bounds.y + 52, { steps: 10 });
    await expect(edges).toHaveCount(2);
    await page.mouse.up();
    await expect.poll(async () => (await calls(page, "project_edit")).length).toBe(i + 1);
    await expect(edges).toHaveCount(2);
    await expect(edge).not.toHaveAttribute("d", original!);
    await expect.poll(async () => (await box.boundingBox())!.x).toBeGreaterThan(bounds.x + 40);
  }
  await page.getByRole("button", { name: /Undo/ }).first().click();
  await expect(edge).toHaveCount(1);
  await expect(edge).toHaveAttribute("d", original!);
  expect(warnings).toEqual([]);
});

test("added inputs and outputs resize the compact box and keep new wires aligned", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  const box = page.locator(".react-flow__node-box");
  await expect(page.getByLabel("Zoom", { exact: true })).toHaveText("125%");
  const before = (await box.boundingBox())!;
  await box.click();
  await page.getByRole("button", { name: "Input", exact: true }).click();
  await expect(box.locator('[data-handleid="i4"]')).toHaveCount(1);
  const from = (await page.locator('[data-id="rail-left"]').boundingBox())!;
  const to = (await box.boundingBox())!;
  expect(to.height).toBeCloseTo(before.height + 22 * to.width / 176, 0);
  const zoom = to.width / 176;
  await page.mouse.move(from.x + 21, from.y + 24);
  await page.mouse.down();
  await page.mouse.move(to.x - 10 * zoom, to.y + 90 * zoom, { steps: 10 });
  await page.mouse.up();
  await expect(page.locator(".react-flow__edge-path")).toHaveCount(2);
  await page.getByRole("button", { name: "Output", exact: true }).click();
  await expect(box.locator('[data-handleid="o5"]')).toHaveCount(1);
  expect((await box.boundingBox())!.height).toBe(to.height);
  await page.getByRole("button", { name: "Output", exact: true }).click();
  const output = box.locator('[data-handleid="o6"]');
  await expect(output).toHaveCount(1);
  expect((await box.boundingBox())!.height).toBeCloseTo(to.height + 22 * zoom, 0);
  await output.focus();
  await page.keyboard.press("Enter");
  await page.locator('[data-id="rail-right"]').getByRole("button", { name: "Connect to onStopped" }).focus();
  await page.keyboard.press("Enter");
  const edge = page.locator('[data-id="l2"] .react-flow__edge-path');
  await expect(edge).toHaveCount(1);
  await expect.poll(async () => {
    const end = await edge.evaluate((path: SVGPathElement) => {
      const p = path.getPointAtLength(path.getTotalLength()).matrixTransform(path.getScreenCTM()!);
      return { x: p.x, y: p.y };
    });
    const anchor = (await output.boundingBox())!;
    return Math.hypot(end.x - anchor.x, end.y - anchor.y);
  }).toBeLessThan(1);
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(box.locator(".box-port")).toHaveCount(2);
  expect((await box.boundingBox())!.height).toBe(before.height);
});

test("new behavior chooses its name and timeline root", async ({ page }) => {
  await backend(page, xar, "pml");
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await page
    .getByRole("toolbar", { name: "Project editing" })
    .getByRole("button", { name: "New behavior", exact: true })
    .click();
  const modal = page.getByRole("dialog", { name: "New behavior" });
  await modal.getByRole("textbox", { name: "Behavior name" }).fill("Dance");
  await modal
    .getByRole("combobox", { name: "Behavior type" })
    .selectOption("Timeline");
  await modal.getByRole("button", { name: "Create", exact: true }).click();
  await expect
    .poll(() => calls(page, "project_add_behavior"))
    .toEqual([{ session: 7, name: "Dance", root: "Timeline" }]);
  await expect(
    page.getByRole("toolbar", { name: "Timeline editing" }),
  ).toBeVisible();
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
});

test("editing goes through the backend: rename, parameter, delete, undo and save", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await expect(page.locator(".react-flow__node-box")).toHaveCount(1);
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  await page.locator(".react-flow__node-box").click();

  await page.getByRole("textbox", { name: "Box name" }).fill("Welcomer");
  await page.getByRole("textbox", { name: "Box name" }).press("Enter");
  await expect(page.locator(".react-flow__node-box")).toContainText("Welcomer");
  expect((await calls(page, "project_edit"))[0]).toMatchObject({
    session: 7,
    label: "Edit box name",
    commands: [
      { type: "setAttributes", node: 103, values: { name: "Welcomer" } },
    ],
  });
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");

  await page.getByRole("button", { name: "Input", exact: true }).click();
  await expect
    .poll(async () => (await calls(page, "project_edit")).at(-1)?.commands[0])
    .toMatchObject({ type: "addPort", box: 103, kind: "Input", name: "input" });

  await page.getByRole("textbox", { name: "Text value" }).fill("Hello");
  await page.getByRole("textbox", { name: "Text value" }).blur();
  await expect
    .poll(async () => (await calls(page, "project_edit")).length)
    .toBe(3);

  await page.locator(".react-flow__node-box").click();
  await page.getByTestId("graph").press("Delete");
  await expect(page.locator(".react-flow__node-box")).toHaveCount(0);
  expect((await calls(page, "project_edit"))[3].commands[0].type).toBe(
    "remove",
  );

  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator(".react-flow__node-box")).toHaveCount(1);
  await expect(page.locator(".react-flow__node-box")).toContainText("Greeter");

  await page.locator(".react-flow__node-box").click();
  await page.getByRole("textbox", { name: "Box name" }).fill("Again");
  await page.getByRole("textbox", { name: "Box name" }).press("Enter");
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  expect(await calls(page, "project_save")).toEqual([{ session: 7 }]);
});

test("unsaved work is guarded and script tabs follow undo", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await page.locator(".react-flow__node-box").dblclick();
  const editor = page.locator(".cm-content");
  await expect(editor).toHaveText("a = 1");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("a = 2");
  await page.getByRole("button", { name: "Apply to box" }).click();
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  await expect(editor).toHaveText("a = 1");

  // An unapplied draft alone also triggers the guard; Cancel keeps everything.
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("draft");
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  const dialog = page.getByRole("dialog", { name: "Unsaved changes" });
  await expect(dialog).toContainText(
    "1 editor draft has text that was not applied",
  );
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(editor).toHaveText("draft");
  expect(await calls(page, "project_open")).toHaveLength(1);

  // Discard lets the open go ahead.
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await dialog.getByRole("button", { name: "Discard" }).click();
  await expect
    .poll(async () => (await calls(page, "project_open")).length)
    .toBe(2);
});

test("script drafts survive closing their tab and block Save-and-continue", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await page.locator(".react-flow__node-box").dblclick();
  const editor = page.locator(".cm-content");
  await expect(editor).toHaveText("a = 1");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("a = 999");
  // Close the script tab, then reopen it from the diagram.
  await page.getByRole("tab", { name: /Greeter\.py/ }).hover();
  await page
    .getByRole("tab", { name: /Greeter\.py/ })
    .locator(".dv-default-tab-action")
    .click();
  await expect(editor).toHaveCount(0);
  await page.locator(".react-flow__node-box").dblclick();
  await expect(editor).toHaveText("a = 999");
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeEnabled();

  // A document change plus a draft: Save is disabled, Discard stays explicit.
  await page.getByRole("tab", { name: /^demo/ }).click();
  await page.locator(".react-flow__node-box").click();
  await page.getByRole("textbox", { name: "Box name" }).fill("Renamed");
  await page.getByRole("textbox", { name: "Box name" }).press("Enter");
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  const dialog = page.getByRole("dialog", { name: "Unsaved changes" });
  await expect(dialog.getByRole("button", { name: "Save" })).toBeDisabled();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  expect(await calls(page, "project_save")).toEqual([]);

  // Quit (Cmd+Q on macOS) asks the same question instead of terminating.
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("Quit Choren");
  await page.getByRole("button", { name: /^Quit Choren/ }).click();
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  expect(
    (await page.evaluate(() => (window as any).calls)).some((c: any) =>
      c.command.includes("destroy"),
    ),
  ).toBe(false);
});

test("an emptied script is a draft, and Save as folder asks before dropping drafts", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await page.locator(".react-flow__node-box").dblclick();
  const editor = page.locator(".cm-content");
  await expect(editor).toHaveText("a = 1");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Backspace");
  await page.getByRole("tab", { name: /Greeter\.py/ }).hover();
  await page
    .getByRole("tab", { name: /Greeter\.py/ })
    .locator(".dv-default-tab-action")
    .click();
  await page.locator(".react-flow__node-box").dblclick();
  await expect(editor).toHaveText("");
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeEnabled();

  const pickers = async () => (await calls(page, "plugin:dialog|open")).length;
  const before = await pickers();
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("Save as project folder");
  await page.getByRole("button", { name: /^Save as project folder/ }).click();
  const dialog = page.getByRole("dialog", { name: "Unsaved changes" });
  await expect(dialog).toContainText("editor drafts cannot follow");
  await expect(dialog.getByRole("button", { name: "Save" })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  expect(await pickers()).toBe(before);
  await expect(editor).toHaveText("");
});

test("drafts never move to a new box that reuses an id, and empty scripts reopen", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  const box = page.locator(".react-flow__node-box");
  const editor = page.locator(".cm-content");
  const closeTab = async () => {
    await page.getByRole("tab", { name: /\.py/ }).hover();
    await page
      .getByRole("tab", { name: /\.py/ })
      .locator(".dv-default-tab-action")
      .click();
    await expect(editor).toHaveCount(0);
  };
  // Applied empty script: still openable.
  await box.dblclick();
  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.press("Backspace");
  await page.getByRole("button", { name: "Apply to box" }).click();
  await closeTab();
  await box.dblclick();
  await expect(editor).toHaveText("");
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeDisabled();

  // Draft on box 1, delete it, create a new box (which gets id 1 again).
  await page.keyboard.type("a = 999");
  await closeTab();
  await box.click();
  await page.getByTestId("graph").press("Delete");
  await expect(box).toHaveCount(0);
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("Create a new box");
  await page.getByRole("button", { name: /^Create a new box/ }).click();
  await page
    .getByRole("dialog", { name: "Create a new box" })
    .getByRole("button", { name: "Create" })
    .click();
  await expect(box).toHaveCount(1);
  await box.dblclick();
  await expect(editor).toHaveText("");
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeDisabled();
});

const runCommand = async (page: Page, title: string) => {
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(title);
  await page
    .getByRole("dialog")
    .getByRole("button", { name: new RegExp(`^${title}`) })
    .click();
};
const closeScriptTab = async (page: Page) => {
  await page
    .getByRole("tab", { name: /\.py/ })
    .locator(".dv-default-tab-action")
    .click();
  await expect(page.locator(".cm-content")).toHaveCount(0);
};
const createBox = async (page: Page, name: string) => {
  await runCommand(page, "Create a new box");
  const dialog = page.getByRole("dialog", { name: "Create a new box" });
  await dialog.getByRole("textbox", { name: "Name" }).fill(name);
  await dialog.getByRole("button", { name: "Create", exact: true }).click();
};

test("removing a behavior retires open and closed drafts while preserving other behaviors", async ({
  page,
}) => {
  await backend(page, xar, "pml");
  page.on("dialog", (dialog) => dialog.accept());
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  const box = (name: string) =>
    page.locator(".react-flow__node-box:visible", { hasText: name });
  const editor = page.locator(".cm-content");
  await box("Greeter").dblclick();
  await editor.fill("keep_this_draft = True");
  await closeScriptTab(page);

  await runCommand(page, "New behavior");
  await page
    .getByRole("dialog", { name: "New behavior" })
    .getByRole("button", { name: "Create", exact: true })
    .click();
  await createBox(page, "Closed");
  await box("Closed").dblclick();
  await editor.fill("closed_draft = True");
  await closeScriptTab(page);
  await createBox(page, "Open");
  await page.getByRole("tab", { name: "Outline", exact: true }).click();
  await page.getByRole("button", { name: "Open", exact: true }).dblclick();
  await editor.fill("open_draft = True");
  await runCommand(page, "Remove behavior from project");
  await page.getByRole("dialog", { name: "Delete behavior" }).getByRole("button", { name: "Remove from project, keep files" }).click();
  await expect(editor).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /^behavior_2/ })).toHaveCount(0);

  // Both the behavior ID and its first box node handle are reused.
  await runCommand(page, "New behavior");
  await page
    .getByRole("dialog", { name: "New behavior" })
    .getByRole("button", { name: "Create", exact: true })
    .click();
  await createBox(page, "Replacement");
  await box("Replacement").dblclick();
  await expect(editor).toHaveText("");
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeDisabled();
  await closeScriptTab(page);

  await page.getByRole("tab", { name: /^behavior_1/ }).click();
  await box("Greeter").dblclick();
  await expect(editor).toHaveText("keep_this_draft = True");
  await page.getByRole("button", { name: "Apply to box" }).click();
  await expect(
    page.getByRole("button", { name: "Apply to box" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await expect
    .poll(async () => (await calls(page, "project_open")).length)
    .toBe(2);
});

test("failed behavior removal keeps its tabs and drafts", async ({ page }) => {
  await backend(page, xar, "pml");
  page.on("dialog", (dialog) => dialog.accept());
  await page.evaluate(() => {
    (window as any).failRemoval = true;
  });
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await page.locator(".react-flow__node-box").dblclick();
  await page.locator(".cm-content").fill("keep_after_failure = True");
  await runCommand(page, "Remove behavior from project");
  await page.getByRole("dialog", { name: "Delete behavior" }).getByRole("button", { name: "Remove from project, keep files" }).click();
  await expect(page.locator(".notice[role=alert]")).toContainText(
    "Removal failed",
  );
  await expect(page.getByRole("tab", { name: /^behavior_1/ })).toBeVisible();
  await expect(page.locator(".cm-content")).toHaveText(
    "keep_after_failure = True",
  );
  await closeScriptTab(page);
  await page.locator(".react-flow__node-box").dblclick();
  await expect(page.locator(".cm-content")).toHaveText(
    "keep_after_failure = True",
  );
});

for (const enabled of [0, 1]) {
  test(`an emptied ${enabled ? "timeline" : "diagram"} script can be reopened`, async ({
    page,
  }) => {
    const source = xar.replace(
      "</Box><Link",
      `<Timeline enable="${enabled}"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram/></BehaviorKeyframe></BehaviorLayer></Timeline></Box><Link`,
    );
    await backend(page, source);
    await page
      .getByRole("button", { name: "Open project", exact: true })
      .first()
      .click();
    await page.locator(".react-flow__node-box").click();
    await page
      .getByRole("button", { name: "Open script", exact: true })
      .click();
    await page.locator(".cm-content").fill("");
    await page.getByRole("button", { name: "Apply to box" }).click();
    await expect(
      page.getByRole("button", { name: "Apply to box" }),
    ).toBeDisabled();
    await closeScriptTab(page);
    await page.locator(".react-flow__node-box").click();
    await page
      .getByRole("button", { name: "Open script", exact: true })
      .click();
    await expect(page.locator(".cm-content")).toHaveText("");
    await expect(
      page.getByRole("button", { name: "Apply to box" }),
    ).toBeDisabled();
  });
}


for (const style of ["choren", "classic"] as const) {
  test(`${style} displays each nested comment, multiline text and long content`, async ({ page }, testInfo) => {
    const source = readFileSync(new URL("../fixtures/comments.xar", import.meta.url), "utf8");
    await backend(page, source);
    await page.getByRole("button", { name: "Open project", exact: true }).first().click();
    await graphUI(page, style);
    await page.locator('.react-flow__node-box[data-id="1"]').dblclick();
    const first = page.getByRole("note", { name: "Comment in Step 1", exact: true });
    const second = page.getByRole("note", { name: "Comment in Step 1.1", exact: true });
    await expect(first).toHaveText("Introduction\nAsk the child to copy the gesture.");
    await expect(second).toContainText("Keep <b>this</b> & every line as plain text.");
    await expect(second.locator("b")).toHaveCount(0);
    await expect(page.getByRole("note", { name: "Comment in Empty note" })).toHaveText("Empty comment");
    expect(await second.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
    await second.evaluate((el) => { el.scrollTop = el.scrollHeight; });
    expect(await second.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    await first.click();
    await expect(page.getByRole("textbox", {name: "Comment text", exact: true})).toHaveValue("Introduction\nAsk the child to copy the gesture.");
    await second.click();
    await expect(page.getByRole("textbox", {name: "Comment text", exact: true})).toHaveValue(/The final instruction\./);
    if (process.env.CAPTURE_UI) await page.getByTestId("graph").screenshot({ path: testInfo.outputPath(`${style}-comments.png`) });
  });

  test(`${style} keeps dropped positions and wires stable until save, undo and failure`, async ({ page }) => {
    await backend(page);
    await page.getByRole("button", { name: "Open project", exact: true }).first().click();
    await graphUI(page, style);
    const box = page.locator('.react-flow__node-box[data-id="1"]');
    const position = () => box.evaluate((el) => el.style.transform);
    const original = await position();
    const edge = page.locator(".react-flow__edge-path");
    const originalWire = await edge.getAttribute("d");
    const drag = async () => {
      const rect = (await box.boundingBox())!;
      await page.mouse.move(rect.x + rect.width / 2, rect.y + 10);
      await page.mouse.down();
      await page.mouse.move(rect.x + rect.width / 2 + 90, rect.y + 70, { steps: 12 });
      await page.mouse.up();
      await expect.poll(() => page.evaluate(() => !!(window as any).releaseEdit)).toBe(true);
    };
    await page.evaluate(() => { (window as any).holdEdits = true; });
    await drag();
    // Sample painted frames while the backend is deliberately blocked, catching even a brief snap back.
    const frames = await box.evaluate(async (node) => {
      const positions: string[] = [];
      for (let i = 0; i < 15; i++) {
        await new Promise(requestAnimationFrame);
        positions.push((node as HTMLElement).style.transform);
      }
      return positions;
    });
    expect(new Set(frames).size).toBe(1);
    const dropped = await position();
    expect(dropped).not.toBe(original);
    expect(await edge.getAttribute("d")).not.toBe(originalWire);
    const droppedWire = await edge.getAttribute("d");
    await page.evaluate(() => { (window as any).holdEdits = false; (window as any).releaseEdit(); });
    await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
    await expect.poll(position).toBe(dropped);
    await expect(edge).toHaveAttribute("d", droppedWire!);
    await runCommand(page, "Undo");
    await expect.poll(position).toBe(original);
    await expect(edge).toHaveAttribute("d", originalWire!);
    await page.evaluate(() => {
      (window as any).holdEdits = true;
      (window as any).failEdit = true;
      (window as any).releaseEdit = undefined;
    });
    await drag();
    expect(await position()).not.toBe(original);
    await page.evaluate(() => (window as any).releaseEdit());
    await expect(page.locator(".notice[role=alert]")).toContainText("Save position failed");
    await expect.poll(position).toBe(original);
    await expect(edge).toHaveAttribute("d", originalWire!);
  });
}
