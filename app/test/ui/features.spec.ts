import { expect, test, type Page } from "@playwright/test";
import { expectTidyLayout } from "./layout-audit";

/**
 * Authoring features against a mock backend that serves a hand-built document with node handles
 * (as the Rust view does) and records every IPC call. Each test checks the commands the UI sends.
 */
async function backend(page: Page, format: "pml" | "crg" = "pml", timelineRoot = false, inputType?: number, nested = false) {
  await page.addInitScript(
    ({ format, timelineRoot, inputType, nested }) => {
      let rid = 0;
      let running = false;
      let revision = 0;
      const paramValues: Record<number, string> = {};
      const calls: { command: string; args: any }[] = [];
      const callbacks: Record<number, (e: unknown) => void> = {};
      const listeners: Record<string, number> = {};
      const libraryEntries: any[] = [
        { name: "Say", path: "/bundled/Say/box.xar", category: "Choren / Speech / Creation", source: "choren" },
        { name: "Nod", path: "/bundled/Nod/box.xar", category: "Choren/Animation/Creation", source: "choren" },
        { name: "Eyes Color", path: "/bundled/Eyes/box.xar", category: "Choren / LEDs / Creation", source: "choren" },
        { name: "Custom", path: "/user/Custom/box.xar", category: "", source: "user" },
      ];
      const port = (node: number, id: number, name: string, nature: number, extra = {}) => ({
        node,
        id,
        name,
        type: 1,
        nature,
        inner: false,
        tooltip: "",
        ...extra,
      });
      const box = (node: number, id: number, name: string, x: number, extra = {}) => ({
        node,
        id,
        name,
        x,
        y: 80,
        tooltip: "",
        bitmaps: [],
        script: "",
        plugin: "",
        inputs: [port(node + 1, 1, "onStart", 2, { type: inputType ?? 1 })],
        outputs: [port(node + 2, 2, "onStopped", 1)],
        params: [],
        resources: [],
        ...extra,
      });
      const key = (node: number, frame: number, value: number, extra = {}) => ({
        node,
        frame,
        value,
        smooth: false,
        symmetrical: false,
        ...extra,
      });
      const doc = () => ({
        id: "/p/demo.pml",
        source: format === "crg" ? "/p/demo.crg" : "/p/demo.pml",
        name: "demo",
        format,
        metadata: {},
        diagnostics: [],
        files: [
          { path: "manifest.xml", kind: "manifest" },
          { path: "greet/greet_enu.top", kind: "topic" },
          { path: "greet/greet.dlg", kind: "dialog" },
          { path: "translations/translation_en_US.ts", kind: "translation" },
          { path: "sounds/hello.ogg", kind: "resource" },
        ],
        behaviors: [
          {
            id: "b1",
            name: "behavior_1",
            source: "behavior_1/behavior.xar",
            root: box(1, -1, "root", 0, {
              timeline: {
                node: 5,
                enabled: timelineRoot,
                fps: 25,
                size: timelineRoot ? 50 : 0,
                curves: [],
                layers: [
                  {
                    node: 6,
                    name: "behavior_layer1",
                    keyframes: [
                      {
                        node: 7,
                        name: "keyframe1",
                        index: 1,
                        diagram: {
                          node: 10,
                          scale: 100,
                          links: [{ node: 11, outOwner: 1, outPort: 2, inOwner: 2, inPort: 1 }],
                          boxes: [
                            box(20, 1, "Greeter", 40, {
                              ...(nested ? { timeline: { enabled: false, fps: 25, size: 0, curves: [], layers: [{ name: "Flow", keyframes: [{ name: "Start", index: 1, diagram: { node: 88, scale: 100, links: [], boxes: [box(90, 42, "Nested", 40)] } }] }] } } : {}),
                              params: [
                                { node: 24, id: 3, name: "Speed", contentType: 1, value: paramValues[24] ?? "5", defaultValue: "5", tooltip: "", min: "0", max: "10", customChoice: false, choices: [] },
                                { node: 25, id: 4, name: "Side", contentType: 3, value: paramValues[25] ?? "Left", defaultValue: "Left", tooltip: "", customChoice: false, choices: [{ node: 26, value: "Left" }, { node: 270, value: "Right" }] },
                              ],
                            }),
                            box(30, 2, "Waver", 260, {
                              params: [{ node: 31, id: 5, name: "Sound", contentType: 4, value: paramValues[31] ?? "", defaultValue: "", tooltip: "File to play", customChoice: false, choices: [] }],
                            }),
                            box(40, 3, "Dance", 480, {
                              timeline: {
                                node: 41,
                                enabled: true,
                                fps: 25,
                                size: 50,
                                layers: [],
                                curves: [
                                  {
                                    node: 42,
                                    actuator: "HeadYaw",
                                    frames: [10, 20],
                                    values: [5, -5],
                                    unit: 0,
                                    mute: false,
                                    keys: [
                                      key(43, 10, 5),
                                      key(44, 20, -5, {
                                        left: { node: 45, interp: "bezier", abscissa: -3, ordinate: 0 },
                                        right: { node: 46, interp: "bezier", abscissa: 3, ordinate: 0 },
                                      }),
                                    ],
                                  },
                                ],
                              },
                            }),
                          ],
                        },
                      },
                    ],
                  },
                ],
              },
            }),
          },
        ],
      });
      const textFiles: Record<string, string> = {};
      const changed = (label: string, document = doc()) => ({
        history: { revision: ++revision, dirty: true, undo: label, created: [] },
        document,
      });
      Object.assign(window, {
        calls,
        isTauri: true,
        emit: (event: string, payload: unknown) => callbacks[listeners[event]]?.({ event, id: 0, payload }),
        __TAURI_INTERNALS__: {
          transformCallback: (cb: (e: unknown) => void) => {
            callbacks[++rid] = cb;
            return rid;
          },
          invoke: async (command: string, args: any) => {
            calls.push({ command, args });
            if (command === "plugin:menu|new") return [++rid, String(rid)];
            if (command === "plugin:event|listen") {
              listeners[args.event] = args.handler;
              return rid;
            }
            if (command === "plugin:dialog|open") {
              const o = args.options ?? {};
              if (o.directory) return "/p/out";
              if (o.filters?.[0]?.extensions?.includes("cbl")) return "/p/Legacy.cbl";
              if (o.filters?.[0]?.extensions?.includes("qianim")) return "/p/in.qianim";
              return "/p/demo.pml";
            }
            if (command === "plugin:dialog|save") return `/p/out.${args.options.filters[0].extensions[0]}`;
            if (command.startsWith("plugin:") || command === "authorize_project_root") return;
            if (command === "project_open") return { session: 7, ...doc() };
            if (command === "project_edit") {
              if ((window as any).holdMenuEdit)
                await new Promise<void>(resolve => { (window as any).releaseMenuEdit = resolve; });
              for (const c of args.commands) if (c.type === "setAttributes" && c.values.value !== undefined) paramValues[c.node] = c.values.value;
              return changed(args.label);
            }
            if (command === "project_save") {
              if ((window as any).failSave) throw new Error("Cannot save Gesture Imitation English/behavior.xar: Permission denied (os error 13)");
              return { revision, dirty: false, created: [] };
            }
            if (command === "project_copy")
              return '<Box id="1" name="Greeter" />';
            if (command === "library_tree") {
              if ((window as any).failLibraryGrant && args.userDirs?.length) throw new Error("This location has not been granted to the app.");
              if ((window as any).libraryTrees && !(window as any).failLibraryGrant) return structuredClone((window as any).libraryTrees);
              const roots = [{path: "/bundled", name: "Choren", source: "choren", writable: false, children: [] as any[]},
                ...(args.userDirs ?? []).map((path: string) => ({path, name: path.split("/").at(-1), source: "user", writable: true, children: [] as any[]}))];
              for (const entry of libraryEntries) {
                const root = roots.find(r => r.source === entry.source && (r.source !== "user" || entry.path.startsWith(`${r.path}/`))) ?? roots[0];
                let children = root.children;
                const parts = entry.source === "user" ? entry.path.slice(root.path.length + 1).split("/").slice(0, -2) : entry.category.split("/").map(p => p.trim()).filter(p => p && p !== "Choren");
                let relativePath = "";
                for (const part of parts) {
                  relativePath += `${relativePath ? "/" : ""}${part}`;
                  let folder = children.find((c: any) => c.name === part);
                  if (!folder) {folder = {name: part, relativePath, kind: "folder", boxFile: null, children: []}; children.push(folder);}
                  children = folder.children;
                }
                children.push({name: entry.name, relativePath: `${relativePath ? relativePath + "/" : ""}${entry.name}`, kind: "box", boxFile: entry.path, hasScript: true, children: []});
              }
              return roots;
            }
            if (command === "library_box")
              return box(90, 0, "Say", 0, { script: "say()" });
            if (command === "library_create" || command === "library_import_cbl" || command === "library_export_cbl") {
              if ((window as any).failLibraryArchive) throw new Error("folder not empty");
              return;
            }
            if (command === "library_copy" || command === "library_manage") {if ((window as any).failLibraryManage) throw new Error("An item with that name already exists."); return;}
            if (command === "library_add_box") {
              if ((window as any).failLibrarySave)
                throw new Error("Library is read only");
              const path = `${args.library}/${args.folder}/${args.name}/box.xar`;
              libraryEntries.push({
                name: args.name,
                path,
                category: args.folder,
                source: "user",
              });
              return path;
            }
            if (command === "library_remove") {
              const i = libraryEntries.findIndex(
                (entry) =>
                  entry.path === `${args.library}/${args.path}/box.xar`,
              );
              libraryEntries.splice(i, 1);
              return;
            }
            if (command === "project_insert_library_box") {
              if ((window as any).libraryConflict && !args.policy)
                return { document: doc(), insert: { result: "conflicts", value: ["sound.wav"] } };
              const result = changed("Insert box");
              return { ...result, insert: { result: "done", value: result.history } };
            }
            if (command === "robot_status") return inputType === undefined ? null : { virtual: true, model: "nao6", url: "tcp://127.0.0.1:9559" };
            if (command === "robot_descriptor") return { id: "nao6", name: "NAO 6", bodyNames: [], joints: [], postures: [], touch: [], tips: {} };
            if (command === "robot_play") {
              // holdPlay: the upload takes until the test calls releasePlay().
              if ((window as any).holdPlay) await new Promise((r) => ((window as any).releasePlay = r));
              running = true;
              (window as any).emit("robot://behavior", ["demo/behavior_1", "started"]);
              return "demo/behavior_1";
            }
            if (command === "robot_set_parameters") {
              if ((window as any).failParameters) throw new Error("Parameter delivery failed");
              return;
            }
            if (command === "robot_stop") { running = false; return; }
            if (command === "robot_call") {
              if (args.method === "getLanguage") return "English";
              if (args.method === "getAvailableLanguages") return ["English", "French"];
              if (args.method === "getActivatedTopics") return ["greet"];
              if (args.method === "forceInput") {
                if ((window as any).failDialog) throw new Error("Dialog input failed");
                (window as any).emit("robot://dialog-input", [args.args[0]]);
                return;
              }
              if (args.method === "getRunningBehaviors") return running ? ["demo/behavior_1"] : [];
              return null;
            }
            if (command === "robot_activate_input") {
              if ((window as any).failInput) throw new Error("Input delivery failed");
              return null;
            }
            if (command === "project_manifest")
              return { uuid: "demo", version: "1.0.0", names: [], supportedLanguages: ["en_US", "fr_FR"],
                behaviors: [{ path: "behavior_1", nature: "interactive", userRequestable: false, names: [["en_US", "Greeting"]], descriptions: [], tags: [["en_US", "hello"]], triggerSentences: [["en_US", "Say hello"]], loadingResponses: [], conditions: [["launchTrigger", "a < b"]], permissions: [] }],
                requirements: { naoqi: [["2.8", ""]], robots: [{ model: "NAO", minHeadVersion: "6", maxHeadVersion: "", minBodyVersion: "6", maxBodyVersion: "" }] },
              };
            if (command === "project_set_manifest") return changed("Edit project properties");
            if (command === "project_export_motion_python") {
              if ((window as any).failMotionExport) throw new Error("Cannot export motion");
              return args.bezier ? "# Backend motion\nmotion.angleInterpolationBezier(names, times, keys)\n" : "# Backend linear motion\n";
            }
            if (command === "project_update_translations") return changed("Update translations");
            if (command === "project_add_dialog") return changed(`Add dialog ${args.name}`);
            if (command === "project_read_text")
              return textFiles[args.path] ?? (args.path.endsWith(".top")
                ? "topic: ~greet()\nlanguage: enu\n\n"
                : '<TS version="2.1" language="en_US"><context><message><translation type="unfinished">Hi</translation></message></context></TS>');
            if (command === "project_write_text") {textFiles[args.path] = args.text; return changed("Edit text");}
            if (command === "project_export_crg" || command === "project_export_qianim") return;
            if (command === "project_import_qianim") return changed("Import animation");
            if (command === "project_save_as_folder") return { session: 7, ...doc(), format: "pml" };
            if (command === "project_close") return;
            throw new Error(`Unexpected IPC: ${command}`);
          },
        },
      });
    },
    { format, timelineRoot, inputType, nested },
  );
  await page.goto("/");
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.locator(".react-flow__node-box")).toHaveCount(3);
}

const calls = (page: Page, command: string) =>
  page.evaluate((c) => (window as any).calls.filter((x: any) => x.command === c).map((x: any) => x.args), command);
const lastEdit = async (page: Page) => (await calls(page, "project_edit")).at(-1);
const palette = async (page: Page, title: string) => {
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(title);
  await page.getByRole("button", { name: title, exact: false }).last().click();
};

test("convert to box wraps the selected boxes", async ({ page }) => {
  await backend(page);
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await page.locator(".react-flow__node-box", { hasText: "Waver" }).click({ modifiers: ["Shift"] });
  await palette(page, "Group into box");
  await page.getByRole("textbox", { name: "Name of the new box" }).fill("Greeting");
  await page.getByRole("button", { name: "Group", exact: true }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({
    label: "Group into box",
    commands: [{ type: "convertToBox", diagram: 10, name: "Greeting" }],
  });
  expect([...(await lastEdit(page)).commands[0].boxes].sort()).toEqual([20, 30]);
});

test("diagram toolbar copies, pastes and opens the box editor", async ({
  page,
}) => {
  await backend(page);
  const toolbar = page.getByRole("toolbar", { name: "Diagram editing" });
  await expect(
    toolbar.getByRole("button", { name: "Paste", exact: true }),
  ).toBeDisabled();
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await toolbar.getByRole("button", { name: "Copy", exact: true }).click();
  await toolbar.getByRole("button", { name: "Paste", exact: true }).click();
  await expect
    .poll(() => lastEdit(page))
    .toMatchObject({
      commands: [
        { type: "paste", diagram: 10, xml: '<Box id="1" name="Greeter" />' },
      ],
    });
  await toolbar.getByRole("button", { name: "New box", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Create a new box" }),
  ).toBeVisible();
});

test("libraries group shared category paths into collapsible folders", async ({ page }) => {
  await backend(page);
  await page.getByRole("toolbar", { name: "Diagram editing" }).getByRole("button", { name: "Libraries", exact: true }).click();
  const root = page.locator("details.library-root").filter({ has: page.locator(":scope > summary", { hasText: /^Choren/ }) });
  await expect(root).toHaveCount(1);
  await expect(root.locator(":scope > details > summary")).toHaveText(["Speech", "Animation", "LEDs"]);
  const nod = page.getByRole("button", { name: "Nod", exact: true });
  await expect(nod).toBeHidden();
  await root.locator("summary", { hasText: /^Animation$/ }).click();
  const animation = root.locator("details").filter({ has: page.locator(":scope > summary", { hasText: /^Animation$/ }) });
  await animation.locator("summary", { hasText: /^Creation$/ }).click();
  await expect(nod).toBeVisible();
  await root.locator(":scope > summary").click();
  await expect(nod).toBeHidden();
  const search = page.getByRole("textbox", { name: "Search libraries" });
  await search.fill("Eyes");
  await expect(page.getByRole("button", { name: "Eyes Color", exact: true })).toBeVisible();
  await expect(root.locator(":scope > details > summary")).toHaveText(["LEDs"]);
  await search.fill("");
  await expect(page.getByRole("button", { name: "Eyes Color", exact: true })).toBeHidden();
  await expect(root.locator(":scope > summary")).toContainText("Read-only");
});

test("bundled libraries work without an install; user libraries create, save and delete boxes", async ({
  page,
}, info) => {
  await backend(page);
  await page
    .getByRole("toolbar", { name: "Diagram editing" })
    .getByRole("button", { name: "Libraries", exact: true })
    .click();
  await page.getByRole("textbox", { name: "Search libraries" }).fill("Say");
  await page.getByRole("button", { name: "Say", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Insert into diagram" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Delete from library" }),
  ).toHaveCount(0);
  expect((await calls(page, "library_tree"))[0]).toEqual({
    shareDir: null,
    userDirs: [],
  });
  await page.getByRole("button", { name: "Insert into diagram" }).click();
  await expect
    .poll(() => calls(page, "project_insert_library_box"))
    .toHaveLength(1);

  await page.getByRole("button", { name: "New library…", exact: true }).click();
  await page.getByRole("textbox", { name: "Library name" }).fill("Reusable");
  await page.getByRole("button", { name: "Choose location…" }).click();
  await expect
    .poll(() => calls(page, "library_create"))
    .toEqual([{ dir: "/p/out/Reusable", title: "Reusable" }]);
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await page.getByRole("button", { name: "Save selected box…" }).click();
  const modal = page.getByRole("dialog", { name: "Save box to library" });
  await modal.getByRole("textbox", { name: "Box name" }).fill("Greeting");
  await modal.getByRole("textbox", { name: "Category" }).fill("Speech");
  await page.evaluate(() => ((window as any).failLibrarySave = true));
  await modal.getByRole("button", { name: "Save to library" }).click();
  await expect(modal.getByRole("alert")).toHaveText(
    "Error: Library is read only",
  );
  await expect(modal.getByRole("textbox", { name: "Box name" })).toHaveValue(
    "Greeting",
  );
  await page.evaluate(() => ((window as any).failLibrarySave = false));
  await modal.getByRole("button", { name: "Save to library" }).click();
  await expect(modal).toHaveCount(0);
  expect((await calls(page, "library_add_box")).at(-1)).toEqual({
    session: 7,
    behavior: "b1",
    node: 20,
    library: "/p/out/Reusable",
    folder: "Speech",
    name: "Greeting",
  });
  await page
    .getByRole("textbox", { name: "Search libraries" })
    .fill("Greeting");
  await page.getByRole("button", { name: "Greeting", exact: true }).click();
  await page.screenshot({ path: info.outputPath("studio-libraries.png") });
  page.once("dialog", (d) => d.accept());
  await page.getByRole("button", { name: "Delete from library" }).click();
  await expect
    .poll(() => calls(page, "library_remove"))
    .toEqual([{ library: "/p/out/Reusable", path: "Speech/Greeting" }]);
  await expect(
    page.getByRole("button", { name: "Greeting", exact: true }),
  ).toHaveCount(0);
});

test("timeline tools edit names, length and keyframe position without duplicate frames", async ({
  page,
}, info) => {
  await backend(page, "pml", true);
  const toolbar = page.getByRole("toolbar", { name: "Timeline editing" });
  await expect(
    toolbar.getByRole("button", { name: "Keyframe at 1", exact: true }),
  ).toBeDisabled();
  await page.getByRole("spinbutton", { name: "Current frame" }).fill("12");
  await toolbar
    .getByRole("button", { name: "Keyframe at 12", exact: true })
    .click();
  await expect
    .poll(() => lastEdit(page))
    .toMatchObject({
      commands: [
        {
          type: "insertXml",
          parent: 6,
          xml: '<BehaviorKeyframe name="keyframe12" index="12"><Diagram /></BehaviorKeyframe>',
        },
      ],
    });
  await toolbar.getByRole("button", { name: "Timeline properties…" }).click();
  const modal = page.getByRole("dialog", { name: "Timeline properties" });
  await modal
    .getByRole("textbox", { name: "Layer name", exact: true })
    .fill("Actions");
  await modal
    .getByRole("textbox", { name: "Keyframe name", exact: true })
    .fill("Greeting");
  await modal.getByRole("spinbutton", { name: "Keyframe position" }).fill("60");
  await modal.getByRole("button", { name: "Apply", exact: true }).click();
  await expect
    .poll(() => lastEdit(page))
    .toMatchObject({
      commands: [
        { type: "setAttributes", node: 5, values: { size: "60" } },
        { type: "setAttributes", node: 6, values: { name: "Actions" } },
        {
          type: "setAttributes",
          node: 7,
          values: { name: "Greeting", index: "60" },
        },
      ],
    });
  await page.screenshot({ path: info.outputPath("studio-editor-timeline.png") });
  await toolbar
    .getByRole("button", { name: "Record motion", exact: true })
    .click();
  await expect(
    page.getByText("Connect to a robot to record motion."),
  ).toBeVisible();
});

test("resource parameters pick a project file from the list or by dropping it from the Project panel", async ({ page }) => {
  await backend(page);
  await page.locator(".react-flow__node-box", { hasText: "Waver" }).click();
  await expectTidyLayout(page, ".inspector");
  // Resource values point from the behavior folder (behavior_1/) with a leading slash.
  await page.getByRole("combobox", { name: "Sound project file" }).selectOption("sounds/hello.ogg");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "setAttributes", node: 31, values: { value: "/../sounds/hello.ogg" } }] });
  await page.getByRole("textbox", { name: "Sound value" }).fill("");
  await page.getByRole("textbox", { name: "Sound value" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ node: 31, values: { value: "" } }] });
  await page.getByRole("tab", { name: "Project", exact: true }).click();
  await page.getByRole("button", { name: "hello.ogg" }).dragTo(page.locator(".resource-field"));
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ node: 31, values: { value: "/../sounds/hello.ogg" } }] });
});

test("ports: nature, ALMemory key, parameter range and choices", async ({ page }) => {
  await backend(page);
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await page.getByRole("combobox", { name: "onStart nature" }).selectOption("ALMemory");
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "setAttributes", node: 21, values: { nature: "4", stm_value_name: "onStart" } }],
  });
  await page.getByRole("combobox", { name: "onStopped nature" }).selectOption("punctual");
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "setAttributes", node: 22, values: { nature: "2", stm_value_name: null } }],
  });
  await page.getByRole("button", { name: "Event", exact: true }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "addPort", box: 20, kind: "Input", name: "event", values: { nature: "4", type: "0", inner: "1", stm_value_name: "event" } }],
  });
  // Parameter definitions are folded under each parameter's value.
  for (const name of ["Speed", "Side"]) await page.locator(".parameter", { hasText: name }).getByText("Parameter properties").click();
  await expectTidyLayout(page, ".inspector");
  await page.getByRole("textbox", { name: "Speed maximum" }).fill("100");
  await page.getByRole("textbox", { name: "Speed maximum" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({
    label: "Edit parameter: Speed",
    commands: [{ type: "setAttributes", node: 24, values: { max: "100" } }],
  });
  await page.getByRole("textbox", { name: "Side new choice" }).fill('Right "side"');
  await page.getByRole("textbox", { name: "Side new choice" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "insertXml", parent: 25, xml: '<Choice value="Right &quot;side&quot;" />' }],
  });
  await page.getByRole("button", { name: "Delete choice Left" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "remove", nodes: [26] }] });
  await page.getByRole("checkbox", { name: "Side custom value allowed" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "setAttributes", node: 25, values: { custom_choice: "1" } }],
  });
});

test("motion curves: keys, interpolation, tangents, dope sheet, mirror and .qianim", async ({ page }) => {
  await backend(page);
  await page.locator(".react-flow__node-box", { hasText: "Dance" }).dblclick();
  await page.getByRole("button", { name: "Motion curves" }).click();
  const table = page.getByRole("table", { name: "HeadYaw keys" });
  await expect(table.getByRole("row")).toHaveCount(3);
  await expect(page.getByLabel("HeadYaw curve")).toBeVisible();

  await page.getByRole("combobox", { name: "Key 10 interpolation" }).selectOption("Linear");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "setInterpolation", keys: [43], interp: "Linear" }] });
  await page.getByRole("spinbutton", { name: "Key 10 value" }).fill("7.5");
  await page.getByRole("spinbutton", { name: "Key 10 value" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "setAttributes", node: 43, values: { value: "7.5" } }] });
  await page.getByRole("spinbutton", { name: "Key 10 frame" }).fill("12");
  await page.getByRole("spinbutton", { name: "Key 10 frame" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "moveKeys", keys: [43], delta: 2 }] });
  await page.getByRole("spinbutton", { name: "Key 20 right tangent value" }).fill("2");
  await page.getByRole("spinbutton", { name: "Key 20 right tangent value" }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "moveTangent", tangent: 46, abscissa: 3, ordinate: 2 }] });
  await page.getByRole("checkbox", { name: "Key 20 smooth" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "setAttributes", node: 44, values: { smooth: "1" } }] });

  // dope sheet: group store needs the robot's joint state
  await page.getByRole("spinbutton", { name: "Motion frame" }).fill("30");
  await page.getByRole("combobox", { name: "Joint group" }).selectOption("Head");
  await expect(page.getByRole("button", { name: "Store joints" })).toBeDisabled();
  await page.evaluate(() => (window as any).emit("robot://state", { angles: [0.1, 0.2] }));
  await page.getByRole("button", { name: "Store joints" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({
    commands: [{ type: "storeKeys", timeline: 41, frame: 30, group: "Head", angles: [0.1, 0.2] }],
  });
  await page.getByRole("button", { name: "Remove joints" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "removeKeys", timeline: 41, frame: 30, group: "Head" }] });
  await page.getByRole("button", { name: "Flip" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "mirror", timeline: 41, mode: "Flip" }] });
  await page.getByRole("button", { name: "Mirror L→R" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "mirror", timeline: 41, mode: "LeftToRight" }] });

  await page.getByRole("button", { name: "Export .qianim" }).click();
  await expect.poll(() => calls(page, "project_export_qianim")).toEqual([{ session: 7, behavior: "b1", timeline: 41, path: "/p/out.qianim" }]);
  await page.getByRole("button", { name: "Import .qianim" }).click();
  await expect.poll(() => calls(page, "project_import_qianim")).toEqual([{ session: 7, behavior: "b1", timeline: 41, path: "/p/in.qianim" }]);
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
});

test("translations and dialog topics", async ({ page }) => {
  await backend(page);
  await palette(page, "Update translations");
  await expect.poll(() => calls(page, "project_update_translations")).toEqual([{ session: 7 }]);
  await expect(page.getByLabel("Translation file text")).toHaveValue(/type="unfinished"/);
  await expect(page.locator(".project-text-editor .panel-toolbar")).toContainText("1 strings · 1 unfinished");

  await palette(page, "New dialog topic");
  await page.getByRole("textbox", { name: "Topic name" }).fill("greet");
  await page.getByRole("checkbox", { name: "fr_FR" }).uncheck();
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect.poll(() => calls(page, "project_add_dialog")).toEqual([{ session: 7, name: "greet", languages: ["en_US"] }]);
  const editor = page.getByRole("textbox", { name: "Topic text" });
  await expect(editor).toHaveValue("topic: ~greet()\nlanguage: enu\n\n");
  await editor.fill("topic: ~greet()\nlanguage: enu\n\nu:(hello) hi\n");
  await page.getByRole("button", { name: "Apply to project" }).click();
  await expect.poll(() => calls(page, "project_write_text")).toEqual([
    { session: 7, path: "greet/greet_enu.top", text: "topic: ~greet()\nlanguage: enu\n\nu:(hello) hi\n" },
  ]);
});

test("export to CRG", async ({ page }) => {
  await backend(page);
  await palette(page, "Export to CRG");
  await expect.poll(() => calls(page, "project_export_crg")).toEqual([{ session: 7, behavior: "b1", path: "/p/out.crg" }]);
});

test("an imported .crg saves as a project folder in the same session", async ({ page }) => {
  await backend(page, "crg");
  await palette(page, "Save as project folder");
  await expect.poll(() => calls(page, "project_save_as_folder")).toEqual([{ session: 7, dir: "/p/out", name: "demo" }]);
  await expect(page.locator(".status-notice, .notice, [role=status]").first()).toContainText("Saved as /p/out/demo.pml");
  expect(await calls(page, "project_close")).toEqual([]);
});


test("library drag inserts at the zoomed drop point and retains it through file conflicts", async ({ page }) => {
  await backend(page);
  await page.getByRole("toolbar", { name: "Diagram editing" }).getByRole("button", { name: "Libraries", exact: true }).click();
  await page.getByRole("textbox", { name: "Search libraries" }).fill("Say");
  const entry = page.getByRole("button", { name: "Say", exact: true });
  await expect(entry).toHaveAttribute("draggable", "true");
  await page.getByRole("button", { name: "Zoom in", exact: true }).click();
  await page.waitForTimeout(200); // complete the graph's 120 ms zoom animation
  const graph = page.getByTestId("graph");
  const position = { x: 300, y: 200 };
  const expected = await graph.evaluate((el, point) => {
    const matrix = new DOMMatrix(getComputedStyle(el.querySelector(".react-flow__viewport")!).transform);
    return { x: (point.x - matrix.e) / matrix.a, y: (point.y - matrix.f) / matrix.d };
  }, position);
  await page.evaluate(() => (window as any).libraryConflict = true);
  await entry.dragTo(graph, { targetPosition: position });
  await expect.poll(() => calls(page, "project_insert_library_box")).toHaveLength(1);
  const first = (await calls(page, "project_insert_library_box"))[0];
  expect(first).toMatchObject({ session: 7, behavior: "b1", diagram: 10, path: "/bundled/Say/box.xar" });
  expect(first.x).toBeCloseTo(expected.x, 0);
  expect(first.y).toBeCloseTo(expected.y, 0);
  await page.getByRole("button", { name: "Rename new attached file" }).click();
  await expect.poll(() => calls(page, "project_insert_library_box")).toHaveLength(2);
  expect((await calls(page, "project_insert_library_box"))[1]).toEqual({ ...first, policy: "Rename" });
});

const activatePort = async (page: Page, target: "input" | "output" | "root" = "input") => {
  const element = page.locator(target === "root" ? '[data-id="rail-left"]' : '.react-flow__node-box[data-id="1"]');
  await expect(element).toBeVisible();
  const bounds = (await element.boundingBox())!;
  const compact = await element.locator(".box-classic").count() > 0;
  const zoom = bounds.width / (compact ? 90 : 176);
  const x = target === "input" ? (compact ? 6 : 0) : (compact ? 100 : 176);
  const y = compact ? (target === "input" ? 34 : 12) : 68;
  await page.mouse.dblclick(bounds.x + (target === "root" ? 10 : x * zoom), bounds.y + (target === "root" ? 25 : y * zoom));
};
const activations = async (page: Page) => (await calls(page, "robot_activate_input")).map((c: any) => ({ path: c.path, input: c.input, value: c.value }));

for (const graphStyle of ["choren", "classic"]) {
test(`double-click activates a running input once in ${graphStyle} UI without connecting or opening its box`, async ({ page }) => {
  await backend(page, "pml", false, 1);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("combobox", { name: "Graph UI", exact: true }).selectOption(graphStyle);
  await page.getByRole("button", { name: "Close dialog" }).click();
  await activatePort(page);
  await expect(page.locator(".notice")).toContainText("The behavior is not running");
  expect(await activations(page)).toEqual([]);
  await palette(page, "Play behavior");
  await activatePort(page);
  await expect.poll(() => activations(page)).toEqual([{ path: [[0, 0, 1]], input: 1, value: null }]);
  expect(await calls(page, "project_edit")).toEqual([]);
  await expect(page.locator(".react-flow__handle.clickconnecting")).toHaveCount(0);
  await expect(page.locator(".script-document")).toHaveCount(0);
  // Root rail inputs use the owner's address, not a child ID.
  await activatePort(page, "root");
  await expect.poll(async () => (await activations(page)).at(-1)).toEqual({ path: [], input: 1, value: null });
  // Output ports must not stimulate an input.
  await activatePort(page, "output");
  expect(await activations(page)).toHaveLength(2);
});
}

for (const [type, label, text, expected] of [[2, "Number", "12.5", 12.5], [3, "Text", "hello", "hello"], [0, "Value (JSON)", '[7,"hi"]', [7, "hi"]]] as const) {
  test(`typed input ${type} validates, submits and reports delivery errors`, async ({ page }) => {
    await backend(page, "pml", false, type);
    await palette(page, "Play behavior");
    await activatePort(page);
    const modal = page.getByRole("dialog", { name: "Activate Greeter.onStart" });
    const field = modal.getByRole("textbox", { name: label, exact: true });
    if (type !== 3) {
      await field.fill("invalid");
      await modal.getByRole("button", { name: "Activate input" }).click();
      await expect(modal.getByRole("alert")).toBeVisible();
      expect(await activations(page)).toEqual([]);
    }
    await field.fill(text);
    await page.evaluate(() => (window as any).failInput = true);
    await modal.getByRole("button", { name: "Activate input" }).click();
    await expect(modal.getByRole("alert")).toContainText("Input delivery failed");
    await expect(field).toHaveValue(text);
    await page.evaluate(() => (window as any).failInput = false);
    await modal.getByRole("button", { name: "Activate input" }).click();
    await expect(modal).toHaveCount(0);
    expect((await activations(page)).at(-1)).toEqual({ path: [[0, 0, 1]], input: 1, value: expected });
  });
}


test("nested inputs use the full diagram address and edited runs require Play again", async ({ page }) => {
  await backend(page, "pml", false, 1, true);
  await palette(page, "Play behavior");
  await page.locator('.react-flow__node-box[data-id="1"]').dblclick();
  const nested = page.locator('.react-flow__node-box[data-id="42"]');
  await expect(nested).toBeVisible();
  const port = nested.getByRole("button", { name: "Connect to onStart" });
  await port.focus();
  await page.keyboard.press("Alt+Enter");
  await expect.poll(async () => (await activations(page)).at(-1)).toEqual({ path: [[0, 0, 1], [0, 0, 42]], input: 1, value: null });
  // A drop goes into this nested diagram, not the root diagram.
  await page.getByRole("toolbar", { name: "Diagram editing" }).getByRole("button", { name: "Libraries", exact: true }).click();
  await page.getByRole("textbox", { name: "Search libraries" }).fill("Say");
  await page.getByRole("button", { name: "Say", exact: true }).dragTo(page.getByTestId("graph"), { targetPosition: { x: 200, y: 150 } });
  await expect.poll(async () => (await calls(page, "project_insert_library_box")).at(-1)?.diagram).toBe(88);
  await port.focus();
  await page.keyboard.press("Alt+Enter");
  await expect(page.locator(".notice")).toContainText("diagram changed since Play");
  expect(await activations(page)).toHaveLength(1);
});


test("read-only examples refuse library drops and input activation", async ({ page }) => {
  await backend(page);
  await palette(page, "Explore example project");
  const graph = page.getByTestId("graph");
  await expect(graph).not.toHaveClass(/editable/);
  const data = await page.evaluateHandle(() => {
    const data = new DataTransfer();
    data.setData("application/x-choren-library-box", "/bundled/Say/box.xar");
    return data;
  });
  await graph.dispatchEvent("drop", { dataTransfer: data, clientX: 500, clientY: 300 });
  await graph.locator(".react-flow__handle").first().dispatchEvent("dblclick");
  expect(await calls(page, "project_insert_library_box")).toEqual([]);
  expect(await activations(page)).toEqual([]);
});


test("Play shows that it is starting, refuses repeat clicks and keeps Stop available", async ({ page }, info) => {
  await backend(page, "pml", false, 1);
  await page.evaluate(() => (window as any).holdPlay = true);
  const run = page.getByRole("button", { name: "Run behavior" });
  await run.click();
  await expect(run).toBeDisabled();
  await expect(run.locator("svg.lucide-loader-circle")).toHaveCount(1);
  await expect(page.locator(".connection-status")).toContainText("Starting…");
  await expect(page.getByRole("button", { name: "Stop behavior" })).toBeEnabled();
  await page.locator("header").screenshot({ path: info.outputPath("play-starting.png") });
  await run.click({ force: true });
  await page.keyboard.press("F5");
  expect(await calls(page, "robot_play")).toHaveLength(1);
  await page.evaluate(() => (window as any).releasePlay());
  await expect(run.locator("svg.lucide-play")).toHaveCount(1);
  await expect(page.locator(".connection-status")).toContainText("Running");
});

test("Play save failures are red alerts with a cross, and successful retry clears the error", async ({ page }, info) => {
  await page.emulateMedia({ colorScheme: "light" });
  await backend(page, "pml", false, 1);
  await page.locator('.react-flow__node-box[data-id="1"]').click();
  await page.getByRole("textbox", { name: "Box name" }).fill("Edited");
  await page.getByRole("textbox", { name: "Box name" }).press("Enter");
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page.evaluate(() => (window as any).failSave = true);
  await palette(page, "Play behavior");
  const notice = page.locator(".notice");
  await expect(notice).toHaveAttribute("role", "alert");
  await expect(notice).toHaveClass(/notice-error/);
  await expect(notice).toContainText("Permission denied");
  await expect(notice).toHaveCSS("color", "rgb(180, 61, 76)");
  await page.screenshot({ path: info.outputPath("save-error.png") });
  await expect(notice.locator("svg.lucide-x")).toHaveCount(1);
  await expect(notice.locator("svg.lucide-check")).toHaveCount(0);
  expect(await calls(page, "robot_play")).toEqual([]);
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page.evaluate(() => (window as any).failSave = false);
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(notice).toHaveAttribute("role", "status");
  await expect(notice).not.toHaveClass(/notice-error/);
  await expect(notice).toContainText("Project saved.");
  await expect(notice.locator("svg.lucide-check")).toHaveCount(1);
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  await notice.getByRole("button", { name: "Dismiss notification" }).click();
  await expect(notice).toHaveCount(0);
});

test("diagram context menu supports Cut, zoom presets and dialog-box creation", async ({page}) => {
  await backend(page);
  await page.locator(".react-flow__node-box", {hasText: "Greeter"}).click({button: "right"});
  const menu = page.getByRole("menu", {name: "Diagram actions"});
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", {name: "Cut", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({label: "Cut boxes", commands: [{type: "remove", nodes: [20]}]});
  await page.getByRole("combobox", {name: "Zoom preset"}).selectOption("50");
  await expect(page.getByLabel("Zoom", {exact: true})).toHaveText("50%");
  await palette(page, "Create a new box");
  const dialog = page.getByRole("dialog", {name: "Create a new box"});
  await dialog.getByRole("combobox", {name: "Type", exact: true}).selectOption("Dialog");
  await dialog.getByRole("combobox", {name: "Dialog file"}).selectOption("greet/greet.dlg");
  await dialog.getByRole("button", {name: "Create", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "newBox", kind: "Dialog", dialogFile: "../greet/greet.dlg"}]});
});

for (const style of ["choren", "classic"] as const) {
  const setupMenus = async (page: Page) => {
    await page.addInitScript(style => localStorage.setItem("choren.graph-style", JSON.stringify(style)), style);
    await backend(page);
  };
  const box = (page: Page, id: number) => page.locator(`.react-flow__node-box[data-id="${id}"]`);
  const wirePoint = (page: Page) => page.locator(".react-flow__edge-path").first().evaluate((path: SVGPathElement) => {
    const point = path.getPointAtLength(path.getTotalLength() / 2).matrixTransform(path.getScreenCTM()!);
    return { x: point.x, y: point.y };
  });
  test(`context menus target connections and keep box groups (${style})`, async ({ page }) => {
    await setupMenus(page);
    const menu = page.getByRole("menu", { name: "Diagram actions" });
    await box(page, 1).click();
    await box(page, 2).click({ modifiers: ["Shift"] });
    await box(page, 1).click({ button: "right" });
    await expect(menu.getByText("2 boxes selected")).toBeVisible();
    await expect(menu.getByRole("menuitem")).toHaveText(["Group into box…", "Cut", "Copy", "Delete selection (2 boxes)", "Fit selection"]);
    await menu.getByRole("menuitem", { name: "Group into box…" }).click();
    await page.getByRole("textbox", { name: "Name of the new box" }).fill("Pair");
    await page.getByRole("button", { name: "Group", exact: true }).click();
    await expect.poll(() => lastEdit(page)).toMatchObject({
      commands: [{ type: "convertToBox", boxes: [20, 30], name: "Pair" }],
    });

    // A different box replaces the prior group.
    await box(page, 1).click();
    await box(page, 2).click({ modifiers: ["Shift"] });
    await box(page, 3).click({ button: "right" });
    await expect(menu.getByRole("menuitem", { name: "Delete box", exact: true })).toBeVisible();
    await menu.getByRole("menuitem", { name: "Copy", exact: true }).click();
    await expect.poll(() => calls(page, "project_copy")).toMatchObject([{ boxes: [40] }]);

    // Right-clicking the wire must never delete the selected boxes.
    await box(page, 1).click();
    await box(page, 2).click({ modifiers: ["Shift"] });
    const point = await wirePoint(page);
    await page.mouse.click(point.x, point.y, { button: "right" });
    await expect(menu.getByRole("menuitem")).toHaveText(["Delete connection"]);
    await expect(page.locator(".react-flow__node-box.selected")).toHaveCount(0);
    await menu.getByRole("menuitem", { name: "Delete connection" }).click();
    await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "remove", nodes: [11] }] });
    await expect(menu).toHaveCount(0);
  });

  test(`context menus isolate canvas actions, clamp size and dismiss (${style})`, async ({ page }, info) => {
    await setupMenus(page);
    await box(page, 1).click();
    const menu = page.getByRole("menu", { name: "Diagram actions" });
    await page.evaluate(() => document.documentElement.style.setProperty("--text-scale", "1.5"));
    const bounds = (await page.getByTestId("graph").boundingBox())!;
    await page.mouse.click(bounds.x + bounds.width - 65, bounds.y + bounds.height - 65, { button: "right" });
    await expect(menu.getByRole("menuitem")).toHaveText(["Create a new box…", "Paste", "Fit graph"]);
    await expect(menu.getByRole("menuitem", { name: "Paste", exact: true })).toBeDisabled();
    await expect(menu.getByRole("menuitem", { name: "Paste", exact: true })).toHaveAttribute("title", "Copy boxes first.");
    await expect(page.locator(".react-flow__node-box.selected")).toHaveCount(1);
    const rect = (await menu.boundingBox())!;
    expect(rect.x).toBeGreaterThanOrEqual(bounds.x);
    expect(rect.y).toBeGreaterThanOrEqual(bounds.y);
    expect(rect.x + rect.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    expect(rect.y + rect.height).toBeLessThanOrEqual(bounds.y + bounds.height);
    await page.screenshot({ path: info.outputPath(`context-menu-${style}.png`) });
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);

    await box(page, 1).click({ button: "right" });
    await menu.getByRole("menuitem", { name: "Copy", exact: true }).click();
    await expect.poll(() => calls(page, "project_copy")).toHaveLength(1);
    await page.mouse.click(bounds.x + bounds.width - 65, bounds.y + bounds.height - 65, { button: "right" });
    await expect(menu.getByRole("menuitem", { name: "Paste", exact: true })).toBeEnabled();
    await page.getByRole("button", { name: "Zoom in", exact: true }).click();
    await expect(menu).toHaveCount(0);
    await box(page, 1).click({ button: "right" });
    await box(page, 3).click();
    await expect(menu).toHaveCount(0);
  });

  test(`context menus support keyboard targets and mixed deletion (${style})`, async ({ page }) => {
    await setupMenus(page);
    const menu = page.getByRole("menu", { name: "Diagram actions" });
    await box(page, 1).focus();
    await page.keyboard.press("Shift+F10");
    await expect(menu.getByRole("menuitem", { name: "Open Greeter" })).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(menu.getByRole("menuitem", { name: "Properties", exact: true })).toBeFocused();
    await page.keyboard.press("End");
    await expect(menu.getByRole("menuitem", { name: "Delete box", exact: true })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(box(page, 1)).toBeFocused();
    const edge = page.locator(".react-flow__edge").first();
    await edge.focus();
    await page.keyboard.press("Shift+F10");
    await expect(menu.getByRole("menuitem", { name: "Delete connection" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(edge).toBeFocused();

    await box(page, 1).click();
    const point = await wirePoint(page);
    await page.keyboard.down("Shift");
    await page.mouse.click(point.x, point.y);
    await page.keyboard.up("Shift");
    await box(page, 1).click({ button: "right" });
    await expect(menu.getByText("1 box, 1 connection selected")).toBeVisible();
    await menu.getByRole("menuitem", { name: /Delete selection/ }).click();
    await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "remove", nodes: [11, 20] }] });
  });

  test(`context menus support selection outlines and close on document changes (${style})`, async ({ page }) => {
    await setupMenus(page);
    const first = (await box(page, 1).boundingBox())!;
    const second = (await box(page, 2).boundingBox())!;
    // Marquee selection creates React Flow's separate selection surface.
    await page.mouse.move(first.x - 8, first.y - 8);
    await page.mouse.down();
    await page.mouse.move(second.x + second.width + 8, second.y + second.height + 8, { steps: 8 });
    await page.mouse.up();
    const outline = page.locator(".react-flow__nodesselection-rect");
    await expect(outline).toBeVisible();
    await outline.focus();
    await page.keyboard.press("Shift+F10");
    const menu = page.getByRole("menu", { name: "Diagram actions" });
    await expect(menu.getByRole("menuitem", { name: "Group into box…" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(outline).toBeFocused();
    await outline.click({ button: "right", position: { x: 5, y: 5 } });
    await expect(menu.getByText(/2 boxes/).first()).toBeVisible();
    await menu.getByRole("menuitem", { name: "Copy", exact: true }).click();
    await expect.poll(() => calls(page, "project_copy")).toMatchObject([{ boxes: [20, 30] }]);

    // A pending edit can finish while another menu is open: discard its stale target.
    await page.evaluate(() => { (window as any).holdMenuEdit = true; });
    await page.keyboard.press("Escape");
    const graph = (await page.getByTestId("graph").boundingBox())!;
    await page.mouse.click(graph.x + graph.width - 65, graph.y + graph.height - 65);
    await box(page, 1).click({ button: "right" });
    await menu.getByRole("menuitem", { name: "Cut", exact: true }).click();
    await expect.poll(() => page.evaluate(() => typeof (window as any).releaseMenuEdit)).toBe("function");
    await box(page, 3).click({ button: "right" });
    await expect(menu).toBeVisible();
    await page.evaluate(() => (window as any).releaseMenuEdit());
    await expect(menu).toHaveCount(0);
  });
}

test("timeline range, acquisition and layer mute emit undoable edits", async ({page}) => {
  await backend(page, "pml", true);
  await page.getByRole("checkbox", {name: "Mute layer"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "setAttributes", node: 6, values: {mute: "1"}}]});
  await page.getByRole("button", {name: "Timeline properties…"}).click();
  const modal = page.getByRole("dialog", {name: "Timeline properties"});
  await modal.getByRole("spinbutton", {name: "Playback start"}).fill("5");
  await modal.getByRole("spinbutton", {name: /Playback end/}).fill("40");
  await modal.getByRole("checkbox", {name: "Stretch keys to the new range"}).check();
  await modal.getByRole("combobox", {name: "Resource acquisition"}).selectOption("Aggressive");
  await modal.getByRole("button", {name: "Apply", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [
    {type: "stretchTimelineRange", timeline: 5, start: 5, end: 40},
    {type: "setAttributes", node: 5, values: {resources_acquisition: "2"}},
  ]});
});

test("curve keys support multiple selection, transforms and pointer dragging", async ({page}, info) => {
  await backend(page);
  await page.locator(".react-flow__node-box", {hasText: "Dance"}).dblclick();
  await page.getByRole("button", {name: "Motion curves", exact: true}).click();
  await page.getByRole("button", {name: "Select all keys"}).click();
  await page.getByText("Transform and simplify", {exact: true}).click();
  await page.getByRole("spinbutton", {name: "Curve value scale"}).fill("2");
  await page.getByRole("button", {name: "Transform selected keys"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "setCurveValues", keys: [43,44], scale: 2, offset: 0}]});
  await page.getByRole("button", {name: "Simplify playback range"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "simplifyCurves", mode: "Choren"}]});
  await page.getByRole("combobox", {name: "Simplifier"}).selectOption("Choregraphe-compatible");
  await page.getByRole("button", {name: "Simplify playback range"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "simplifyCurves", mode: "Choregraphe"}]});
  const marker = page.getByRole("button", {name: "Select key at frame 10", exact: true});
  const rect = (await marker.boundingBox())!;
  await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2);
  await page.mouse.down(); await page.mouse.move(rect.x + rect.width / 2 + 30, rect.y + rect.height / 2 - 12, {steps: 4}); await page.mouse.up();
  await expect.poll(() => lastEdit(page)).toMatchObject({label: "Move curve keys", commands: [{type: "moveKeys", keys: [43,44]}, {type: "setCurveValues", keys: [43,44]}]});
  await page.screenshot({path: info.outputPath("parity-curves.png")});
  await page.getByRole("button", {name: "Export motion code…"}).click();
  await expect(page.getByRole("textbox", {name: "Exported motion code"})).toHaveValue(/angleInterpolationBezier/);
});

test("file drafts survive panel changes and guard closing the project", async ({page}) => {
  await backend(page);
  await palette(page, "Show Dialog / QiChat");
  const text = page.getByRole("textbox", {name: "Topic text"});
  await text.fill("topic: ~greet()\nlanguage: enu\nu:(hello) hello\n");
  await palette(page, "Close project");
  const guard = page.getByRole("dialog", {name: "Unsaved changes"});
  await expect(guard).toContainText("1 editor draft");
  await guard.getByRole("button", {name: "Cancel", exact: true}).click();
  await expect(text).toHaveValue(/u:\(hello\) hello/);
  await page.getByRole("button", {name: "Apply to project", exact: true}).click();
  await expect.poll(() => calls(page, "project_write_text")).toHaveLength(1);
  await expect(text).toHaveValue(/u:\(hello\) hello/);
});

test("resource drafts synchronize between visible editors", async ({page}) => {
  await backend(page);
  await palette(page, "Show Resource editor");
  await page.locator('.file-tree button[title="greet/greet_enu.top"]').click();
  const editors = page.getByRole("textbox", {name: "Topic text", exact: true});
  await expect(editors).toHaveCount(2);
  await editors.first().fill("topic: ~shared()\nlanguage: enu\n");
  await expect(editors.nth(1)).toHaveValue("topic: ~shared()\nlanguage: enu\n");
});

test("named layouts can be saved and restored while keeping the project", async ({page}, info) => {
  await backend(page);
  await page.getByRole("button", {name: "Settings", exact: true}).click();
  const modal = page.getByRole("dialog", {name: "Settings"});
  await modal.getByRole("textbox", {name: "Layout name", exact: true}).fill("Authoring");
  await modal.getByRole("button", {name: "Save layout", exact: true}).click();
  await expect(modal.getByRole("combobox", {name: "Saved layout", exact: true})).toHaveValue("Authoring");
  // Every Settings control shows: a saved layout and a linked Choregraphe folder.
  await page.evaluate(() => localStorage.setItem("choregraphe-share-dir", "/Applications/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe"));
  await modal.getByRole("button", {name: "Close dialog"}).click();
  await page.getByRole("button", {name: "Settings", exact: true}).click();
  await expectTidyLayout(page, "dialog[open]");
  await modal.getByRole("combobox", {name: "Saved layout", exact: true}).selectOption("Authoring");
  await modal.getByRole("button", {name: "Load layout", exact: true}).click();
  await modal.getByRole("button", {name: "Close dialog"}).click();
  await expect(page.locator(".project-heading")).toContainText("demo");
  await expect(page.locator(".react-flow__node-box")).toHaveCount(3);
  await page.screenshot({path: info.outputPath("parity-workspace.png")});
});

test("port menus edit root ports and disconnect links; dropping on a box selects a parameter", async ({page}) => {
  await backend(page);
  const greeter = page.locator('.react-flow__node-box[data-id="1"]');
  const output = (await greeter.getByRole("button", {name: "Connect from onStopped", exact: true}).boundingBox())!;
  await page.mouse.click(output.x - 8, output.y, {button: "right"});
  const menu = page.getByRole("menu", {name: "Port onStopped"});
  await menu.getByRole("menuitem", {name: "Disconnect links"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "remove", nodes: [11]}]});
  const start = page.locator('.react-flow__node[data-id="rail-left"]').getByRole("button", {name: "Connect from onStart", exact: true});
  await start.focus(); await start.press("Shift+F10");
  await page.getByRole("menuitem", {name: "Edit onStart…"}).click();
  const properties = page.getByRole("dialog", {name: "Port properties: root"});
  await expect(properties.getByRole("textbox", {name: "Input onStart name", exact: true})).toHaveValue("onStart");
  await properties.getByRole("button", {name: "Close dialog"}).click();
  await greeter.click();
  const from = (await start.boundingBox())!;
  const to = (await greeter.boundingBox())!;
  await page.mouse.move(from.x - 8, from.y);
  await page.mouse.down(); await page.mouse.move(to.x + to.width / 2, to.y + 65, {steps: 8}); await page.mouse.up();
  const chooser = page.getByRole("dialog", {name: "Connect to parameter of Greeter"});
  await chooser.getByRole("button", {name: "Speed", exact: true}).focus();
  await page.keyboard.press("Delete");
  await expect.poll(() => lastEdit(page)).toMatchObject({label: "Disconnect port"});
  await page.keyboard.press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({commands: [{type: "addLink", diagram: 10, outOwner: 0, outPort: 1, inOwner: 1, inPort: 3}]});
});

test("library definitions can be copied into a user folder", async ({page}) => {
  await page.addInitScript(() => localStorage.setItem("choren.user-libraries", JSON.stringify(["/user"])));
  await backend(page); await palette(page, "Show Libraries");
  await page.getByRole("textbox", {name: "Search libraries", exact: true}).fill("Say");
  await page.locator('.tree-row[title="/bundled/Say/box.xar"]').click();
  await page.getByRole("button", {name: "Copy to user library…"}).click();
  const modal = page.getByRole("dialog", {name: "Manage library folders"});
  await modal.getByRole("textbox", {name: "Library folder path"}).fill("Speech");
  await modal.getByRole("textbox", {name: "Library folder name"}).fill("Say copy");
  await modal.getByRole("button", {name: "Paste into folder"}).click();
  await expect.poll(() => calls(page, "library_copy")).toEqual([{source: "/bundled/Say", library: "/user", folder: "Speech", name: "Say copy"}]);
});


test("backend wiring: selected frame ranges mirror in one undo step and Python export uses Rust", async ({ page }) => {
  await backend(page);
  await page.locator(".react-flow__node-box", { hasText: "Dance" }).dblclick();
  await page.getByRole("button", { name: "Motion curves", exact: true }).click();
  await page.getByRole("button", { name: "Select all keys" }).click();
  await page.getByRole("button", { name: "Mirror L→R" }).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ type: "mirror", timeline: 41, mode: "LeftToRight", frames: [10, 20] }] });
  await page.getByRole("button", { name: "Export motion code…" }).click();
  await expect.poll(() => calls(page, "project_export_motion_python")).toContainEqual({ session: 7, behavior: "b1", timeline: 41, bezier: true });
  await expect(page.getByLabel("Exported motion code")).toHaveValue("# Backend motion\nmotion.angleInterpolationBezier(names, times, keys)\n");
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { (window as any).copiedMotion = text; } } }));
  await page.getByRole("button", { name: "Copy code", exact: true }).click();
  expect(await page.evaluate(() => (window as any).copiedMotion)).toBe("# Backend motion\nmotion.angleInterpolationBezier(names, times, keys)\n");
  await page.getByLabel("Bezier interpolation").uncheck();
  await expect(page.getByLabel("Exported motion code")).toHaveValue("# Backend linear motion\n");
  await page.evaluate(() => (window as any).failMotionExport = true);
  await page.getByLabel("Bezier interpolation").check();
  await expect(page.getByRole("dialog", { name: "Export motion code" }).getByRole("alert")).toContainText("Cannot export motion");
  await expect(page.getByRole("button", { name: "Copy code", exact: true })).toBeDisabled();
});

test("backend wiring: live parameters stay typed across consecutive edits and fail locally without losing the edit", async ({ page }) => {
  await backend(page, "pml", false, 1);
  await palette(page, "Play behavior");
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await page.getByLabel("Auto-update parameters on robot").check();
  await page.getByRole("tab", { name: "Inspector", exact: true }).click();
  await page.getByLabel("Speed value", { exact: true }).fill("7");
  await page.getByLabel("Speed value", { exact: true }).press("Enter");
  await expect.poll(() => calls(page, "robot_set_parameters")).toEqual([{ session: 7, behavior: "b1", path: [[0, 0, 1]], values: [["Speed", 7]] }]);
  // A closed choice list is a dropdown.
  await page.getByLabel("Side value", { exact: true }).selectOption("Right");
  await expect.poll(() => calls(page, "robot_set_parameters")).toHaveLength(2);
  expect((await calls(page, "robot_set_parameters"))[1].values).toEqual([["Side", "Right"]]);
  await page.evaluate(() => (window as any).failParameters = true);
  await page.getByLabel("Speed value", { exact: true }).fill("8");
  await page.getByLabel("Speed value", { exact: true }).press("Enter");
  await expect(page.locator(".notice")).toContainText("Parameter delivery failed");
  await expect(page.getByLabel("Speed value", { exact: true })).toHaveValue("8");
  await page.getByLabel("Auto-update parameters on robot").uncheck();
  await page.getByLabel("Speed value", { exact: true }).fill("9");
  await page.getByLabel("Speed value", { exact: true }).press("Enter");
  await expect.poll(() => lastEdit(page)).toMatchObject({ commands: [{ values: { value: "9" } }] });
  expect(await calls(page, "robot_set_parameters")).toHaveLength(3);
});

test("backend wiring: structural edits prevent live parameter updates until Play", async ({ page }) => {
  await backend(page, "pml", false, 1);
  await palette(page, "Play behavior");
  await page.locator(".react-flow__node-box", { hasText: "Greeter" }).click();
  await page.getByLabel("Auto-update parameters on robot").check();
  await page.getByRole("tab", { name: "Inspector", exact: true }).click();
  await page.getByRole("combobox", { name: "Appearance category" }).selectOption("timer");
  await expect.poll(() => calls(page, "project_edit")).toHaveLength(1);
  await page.getByLabel("Speed value", { exact: true }).fill("8");
  await page.getByLabel("Speed value", { exact: true }).press("Enter");
  await expect.poll(() => calls(page, "project_edit")).toHaveLength(2);
  expect(await calls(page, "robot_set_parameters")).toEqual([]);
});

test("backend wiring: robot frames track the open timeline without moving its editor cursor", async ({ page }) => {
  await backend(page, "pml", false, 1);
  await palette(page, "Play behavior");
  await page.locator(".react-flow__node-box", { hasText: "Dance" }).dblclick();
  await page.getByRole("button", { name: "Motion curves", exact: true }).click();
  await page.evaluate(() => (window as any).emit("robot://frame", ["demo/behavior_1", [3], 20]));
  await expect(page.getByLabel("Robot dope playhead")).toHaveAttribute("data-frame", "20");
  await expect(page.getByLabel("Robot dope playhead")).toHaveAttribute("style", /39.215/);
  expect(Number(await page.getByLabel("Robot curve playhead").getAttribute("x1"))).toBeCloseTo(20 * 1000 / 52);
  await expect(page.getByLabel("Motion frame", { exact: true })).toHaveValue("1");
  await page.evaluate(() => (window as any).emit("robot://frame", ["demo/behavior_1", [1], 30]));
  await expect(page.getByLabel("Robot dope playhead")).toHaveAttribute("data-frame", "20");
  await page.evaluate(() => (window as any).emit("robot://frame", ["demo/behavior_1", [3], 21]));
  await expect(page.getByLabel("Robot dope playhead")).toHaveCount(0);
  await page.evaluate(() => (window as any).emit("robot://frame", ["demo/behavior_1", [3], 10]));
  await expect(page.getByLabel("Robot dope playhead")).toBeVisible();
  await page.evaluate(() => (window as any).emit("robot://frame", ["demo/behavior_1", [3], -1]));
  await expect(page.getByLabel("Robot dope playhead")).toHaveCount(0);
});

test("backend wiring: live dialog sends input, receives answers, changes language and handles failures", async ({ page }, info) => {
  await backend(page, "pml", false, 1);
  await palette(page, "Show Robot dialog");
  await expect(page.getByLabel("Dialog language")).toHaveValue("English");
  await expect(page.getByText("Active topics: greet", { exact: true })).toBeVisible();
  await page.getByLabel("Say to robot").fill("hello");
  await page.getByLabel("Say to robot").press("Enter");
  await expect.poll(() => calls(page, "robot_call")).toContainEqual({ service: "ALDialog", method: "forceInput", args: ["hello"] });
  await page.evaluate(() => (window as any).emit("robot://dialog-answer", ["Hi there"]));
  await expect(page.getByRole("log")).toContainText("Human: hello");
  await expect(page.getByRole("log")).toContainText("Robot: Hi there");
  if (process.env.CAPTURE_UI) await page.locator(".robot-dialog").screenshot({ path: info.outputPath("robot-dialog.png") });
  await page.getByLabel("Dialog language").selectOption("French");
  await expect.poll(() => calls(page, "robot_call")).toContainEqual({ service: "ALDialog", method: "setLanguage", args: ["French"] });
  await page.evaluate(() => (window as any).failDialog = true);
  await page.getByLabel("Say to robot").fill("retry this");
  await page.getByLabel("Say to robot").press("Enter");
  await expect(page.locator(".robot-dialog").getByRole("alert")).toContainText("Dialog input failed");
  await expect(page.getByLabel("Say to robot")).toHaveValue("retry this");
  await page.evaluate(() => (window as any).emit("robot://disconnected", null));
  await expect(page.getByLabel("Say to robot")).toBeDisabled();
  await expect(page.getByRole("log")).toBeEmpty();
});

test("backend wiring: import and export CBL registers and refreshes the chosen library", async ({ page }) => {
  await backend(page);
  await palette(page, "Show Libraries");
  await page.getByRole("button", { name: "Import .cbl…", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Import box library" });
  await expect(modal.getByLabel("Library name")).toHaveValue("Legacy");
  await modal.getByRole("button", { name: "Import library…" }).click();
  await expect.poll(() => calls(page, "library_import_cbl")).toEqual([{ path: "/p/Legacy.cbl", dir: "/p/out/Legacy" }]);
  await expect(page.getByLabel("User library")).toHaveValue("/p/out/Legacy");
  await expect.poll(() => calls(page, "library_tree")).toContainEqual(expect.objectContaining({ userDirs: ["/p/out/Legacy"] }));
  await page.getByRole("button", { name: "Save as .cbl…", exact: true }).click();
  await expect.poll(() => calls(page, "library_export_cbl")).toEqual([{ library: "/p/out/Legacy", path: "/p/out.cbl" }]);
  await page.evaluate(() => (window as any).failLibraryArchive = true);
  await page.getByRole("button", { name: "Import .cbl…", exact: true }).click();
  await modal.getByRole("button", { name: "Import library…" }).click();
  await expect(modal.getByRole("alert")).toContainText("folder not empty");
});

test("backend wiring: structured manifest edits preserve untouched properties and validate conditions", async ({ page }, info) => {
  await backend(page);
  await palette(page, "Edit project properties");
  const modal = page.getByRole("dialog", { name: "Project properties" });
  const languages = modal.getByRole("group", { name: "Supported languages" });
  await expect(languages).toContainText("English");
  await expect(languages).toContainText("French");
  await languages.getByRole("button", { name: "Remove French" }).click();
  await languages.getByRole("button", { name: "Add language" }).click();
  const menu = modal.getByRole("menu", { name: "Robot languages" });
  await expect(menu.getByRole("menuitem", { name: /English/ })).toHaveCount(0);
  if (process.env.CAPTURE_UI) await modal.screenshot({ path: info.outputPath("project-languages.png") });
  await menu.getByRole("menuitem", { name: /German/ }).click();
  await expect(menu).toBeHidden();
  await languages.getByRole("button", { name: "Add language" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(modal).toBeVisible();
  const titles = modal.getByRole("group", { name: "Titles" });
  await titles.getByRole("button", { name: "Add titles" }).click();
  await expect(titles.getByLabel("Titles language 1")).toHaveValue("en_US");
  await expect(titles.getByLabel("Titles language 1").locator("option:checked")).toHaveText("English");
  await titles.getByLabel("Titles text 1").fill("Demos");
  await titles.getByRole("button", { name: "Add titles" }).click();
  await expect(titles.getByLabel("Titles language 2")).toHaveValue("de_DE");
  await titles.getByLabel("Titles text 2").fill("Demos auf Deutsch");
  await modal.locator("summary", { hasText: "behavior_1" }).click();
  const behavior = modal.locator(".behavior-properties");
  await behavior.getByLabel("behavior_1 nature").selectOption("solitary");
  await behavior.getByLabel("canStandUp", { exact: true }).check();
  await behavior.getByLabel("User-requestable").check();
  await behavior.getByLabel("Trigger sentences text 1").fill("Start greeting");
  await behavior.getByLabel("Launch conditions text 1").fill("]]>");
  await modal.getByRole("button", { name: "Apply", exact: true }).click();
  expect(await calls(page, "project_set_manifest")).toEqual([]);
  await expect(behavior.getByText("Conditions cannot contain ]]>", { exact: true })).toBeVisible();
  await behavior.getByLabel("Launch conditions text 1").fill("a < c");
  await modal.locator("summary", { hasText: "Requirements" }).click();
  await modal.getByLabel("NAOqi minimum 1").fill("2.8.1");
  await modal.getByLabel("Robot 1 maximum body version").fill("6");
  await expectTidyLayout(page, "dialog[open]");
  if (process.env.CAPTURE_UI) await modal.screenshot({ path: info.outputPath("project-properties.png") });
  await modal.getByRole("button", { name: "Apply", exact: true }).click();
  await expect.poll(() => calls(page, "project_set_manifest")).toHaveLength(1);
  const saved = (await calls(page, "project_set_manifest"))[0].manifest;
  expect(saved.supportedLanguages).toEqual(["en_US", "de_DE"]);
  expect(saved.names).toEqual([["en_US", "Demos"], ["de_DE", "Demos auf Deutsch"]]);
  expect(saved.behaviors[0]).toEqual({ path: "behavior_1", nature: "solitary", userRequestable: true, names: [["en_US", "Greeting"]], descriptions: [], tags: [["en_US", "hello"]], triggerSentences: [["en_US", "Start greeting"]], loadingResponses: [], conditions: [["launchTrigger", "a < c"]], permissions: ["canStandUp"] });
  expect(saved.requirements).toEqual({ naoqi: [["2.8.1", ""]], robots: [{ model: "NAO", minHeadVersion: "6", maxHeadVersion: "", minBodyVersion: "6", maxBodyVersion: "6" }] });
});

test("library context targets retain raw paths and separate matching roots", async ({page}) => {
  await backend(page);
  await page.evaluate(() => {
    (window as any).libraryTrees = ["/one/Same", "/two/Same"].map(path => ({path, name: "Same", source: "user", writable: true, children: [
      {name: "Empty folder", relativePath: "Empty%20folder", kind: "folder", boxFile: null, children: []},
      {name: "Script box", relativePath: "Script%20box", kind: "box", boxFile: `${path}/Script%20box/box.xar`, hasScript: true, children: []},
    ]}));
  });
  await palette(page, "Show Libraries");
  await page.locator(".library-actions").click({button: "right", position: {x: 4, y: 4}});
  await page.getByRole("menuitem", {name: "Refresh", exact: true}).click();
  const roots = page.locator(".library-root");
  await expect(roots).toHaveCount(2);
  const second = roots.nth(1);
  await second.locator(":scope > summary").click({button: "right"});
  const context = page.getByRole("menu", {name: "Context actions"});
  await expect(context.getByRole("menuitem", {name: "Paste", exact: true})).toBeDisabled();
  await page.keyboard.press("Escape");
  await second.getByText("Empty folder", {exact: true}).click({button: "right"});
  await context.getByRole("menuitem", {name: "Rename…", exact: true}).click();
  const rename = page.getByRole("dialog", {name: "Rename library item"});
  await rename.getByLabel("Library item name").fill("Renamed");
  await page.evaluate(() => (window as any).failLibraryManage = true);
  await rename.getByRole("button", {name: "Rename", exact: true}).click();
  await expect(rename.getByRole("alert")).toContainText("already exists");
  await page.evaluate(() => (window as any).failLibraryManage = false);
  await rename.getByRole("button", {name: "Rename", exact: true}).click();
  await expect.poll(() => calls(page, "library_manage")).toContainEqual({library: "/two/Same", folder: "Empty%20folder", action: "rename", name: "Renamed"});
  await second.getByText("Empty folder", {exact: true}).click({button: "right"});
  page.once("dialog", d => d.dismiss());
  await context.getByRole("menuitem", {name: "Delete folder…"}).click();
  expect(await calls(page, "library_remove")).toEqual([]);
  await roots.first().getByRole("button", {name: "Script box", exact: true}).click({button: "right"});
  await expect(context.getByRole("menuitem", {name: "View script"})).toBeVisible();
  await context.getByRole("menuitem", {name: "Copy", exact: true}).click();
  await second.getByText("Empty folder", {exact: true}).click({button: "right"});
  await context.getByRole("menuitem", {name: "Paste", exact: true}).click();
  const paste = page.getByRole("dialog", {name: "Paste into library"});
  await paste.getByLabel("Library item name").fill("Copy");
  await paste.getByRole("button", {name: "Paste", exact: true}).click();
  await expect.poll(() => calls(page, "library_copy")).toEqual([{source: "/one/Same/Script%20box", library: "/two/Same", folder: "Empty%20folder", name: "Copy"}]);
  expect(await calls(page, "project_edit")).toEqual([]);
});

test("library menus isolate background and read-only items, clamp and restore keyboard focus", async ({page}) => {
  await backend(page);
  await palette(page, "Show Libraries");
  await page.getByRole("textbox", {name: "Search libraries"}).fill("Say");
  const box = page.getByRole("button", {name: "Say", exact: true});
  await box.focus();
  await page.keyboard.press("Shift+F10");
  const context = page.getByRole("menu", {name: "Context actions"});
  await expect(context.getByRole("menuitem", {name: "Delete…"})).toHaveCount(0);
  await expect(context.getByRole("menuitem", {name: "Rename…"})).toHaveCount(0);
  await page.keyboard.press("End");
  await expect(context.getByRole("menuitem", {name: "Copy", exact: true})).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(box).toBeFocused();
  await page.getByRole("textbox", {name: "Search libraries"}).click({button: "right"});
  await expect(context).toHaveCount(0);
  await page.locator(".library-actions").click({button: "right", position: {x: 4, y: 4}});
  await expect(context.getByRole("menuitem", {name: "New library…"})).toBeVisible();
  await expect(context.getByRole("menuitem", {name: "Insert into diagram"})).toHaveCount(0);
  await page.evaluate(() => document.documentElement.style.setProperty("--text-scale", "1.5"));
  const bounds = await context.boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(900);
  await page.keyboard.press("Escape");
  await box.click({button: "right"});
  await page.setViewportSize({width: 1100, height: 780});
  await expect(context).toHaveCount(0);
});

test("expired library grants leave bundled content available and reject write targets", async ({page}) => {
  await backend(page);
  await palette(page, "Show Libraries");
  await page.getByRole("button", {name: "New library…", exact: true}).click();
  await page.getByRole("textbox", {name: "Library name"}).fill("Temporary");
  await page.getByRole("button", {name: "Choose location…"}).click();
  await expect(page.getByLabel("User library")).toHaveValue("/p/out/Temporary");
  await page.evaluate(() => (window as any).failLibraryGrant = true);
  await page.locator(".library-actions").click({button: "right", position: {x: 4, y: 4}});
  await page.getByRole("menuitem", {name: "Refresh", exact: true}).click();
  await expect(page.locator(".panel-message.error[role=alert]")).toContainText("Reopen the folder to grant access");
  await expect(page.locator(".library-root")).toHaveCount(1);
  await page.getByRole("textbox", {name: "Search libraries"}).fill("Say");
  await expect(page.getByRole("button", {name: "Say", exact: true})).toBeVisible();
  await page.locator(".library-root > summary").click({button: "right"});
  await expect(page.getByRole("menuitem", {name: "Paste", exact: true})).toHaveCount(0);
  await expect(page.getByRole("menuitem", {name: "New folder…", exact: true})).toHaveCount(0);
});
