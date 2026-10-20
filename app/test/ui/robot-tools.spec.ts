import { expect, test, type Page } from "@playwright/test";
import { expectTidyLayout } from "./layout-audit";

// A timeline root box, so the motion recorder has somewhere to store keys.
const xar = `<ChoregrapheProject><Box id="-1" name="root"><Input id="1" name="onStart" type="1" nature="2"/><Output id="2" name="onStopped" type="1" nature="1"/><Timeline enable="1" fps="25" size="100"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Start" index="1"><Diagram><Box id="1" name="Other" x="100" y="80"><Timeline enable="1" fps="25" size="100"/></Box></Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>`;

/** Desktop IPC stand-in with a virtual robot, the install folder and robot events. */
async function backend(page: Page) {
  await page.addInitScript(
    ({ xar }) => {
      localStorage.setItem("choregraphe-share-dir", "/share/choregraphe");
      localStorage.setItem("choren-user-poses", JSON.stringify("/u/poses.xap"));
      localStorage.setItem(
        "choren-memory-keys",
        JSON.stringify(["Choren/Test"]),
      );
      let rid = 0;
      const calls: { command: string; args: any }[] = [];
      const callbacks: Record<number, (e: unknown) => void> = {};
      const listeners: Record<string, number> = {};
      const joint = (name: string, axis: number[], chain: string) => ({
        name,
        parent: null,
        origin: [0, 0, 0.1],
        axis,
        min: -2,
        max: 2,
        chain,
      });
      const descriptor = {
        id: "nao6",
        name: "NAO 6",
        bodyNames: ["HeadYaw", "LHand"],
        postures: [],
        touch: [],
        tips: {},
        joints: [
          joint("HeadYaw", [0, 0, 1], "Head"),
          joint("LHand", [0, 0, 0], "LArm"),
        ],
        meshes: { Torso: "naoTorsoV6", HeadYaw: "naoHeadYaw" },
      };
      // One triangle in choren_robot::ogre::to_bytes layout.
      const triangle = () => {
        const u = new Uint32Array(1 + 3 + 18 + 3);
        const f = new Float32Array(u.buffer);
        u.set([1, 0xd4d4d4, 3, 3]);
        f.set([0, 0, 0, 0.1, 0, 0, 0, 0.1, 0], 4);
        f.set([0, 0, 1, 0, 0, 1, 0, 0, 1], 13);
        u.set([0, 1, 2], 22);
        return u.buffer;
      };
      const pose = (name: string, motors: [string, number][]) => ({
        folder: "Basics",
        name,
        description: "",
        bitmap: "",
        motors,
      });
      let doc: any;
      Object.assign(window, {
        calls,
        emit: (event: string, payload: unknown) =>
          callbacks[listeners[event]]?.({ event, id: 1, payload }),
        isTauri: true,
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
            if (command === "plugin:dialog|open") return "/p/demo.xar";
            if (
              command.startsWith("plugin:") ||
              command === "authorize_project_root" ||
              command === "project_close"
            )
              return;
            if (command === "robot_start_virtual")
              return {
                url: "tcp://127.0.0.1:9559",
                model: "nao6",
                virtual: true,
              };
            if (command === "robot_status") return null;
            if (command === "library_tree") return [];
            if (command === "robot_descriptor") return descriptor;
            if (command === "robot_mesh") {
              if (localStorage.getItem("test-mesh-error")) throw "Cannot read mesh naoHeadYaw: No such file or directory";
              return triangle();
            }
            if (command === "robot_call")
              return args.method === "getData" ? 0.42 : args.method === "getDataListName" ? ["Choren/Test", "Choren/Other"] : null;
            if (command === "poses_read")
              return args.path === "/u/poses.xap"
                ? []
                : [
                    pose("Stand", [
                      ["HeadYaw", 30],
                      ["LHand", 0.5],
                    ]),
                  ];
            if (command === "poses_write") return [args.pose];
            if (command === "project_open") {
              const { openProject } = await import(
                /* @vite-ignore */ `${location.origin}/src/format/project.ts`
              );
              doc = await openProject(args.path, async () =>
                new TextEncoder().encode(xar),
              );
              doc.behaviors[0].root.node = 100;
              doc.behaviors[0].root.timeline.node = 101;
              doc.behaviors[0].root.timeline.layers[0].keyframes[0].diagram.boxes[0].timeline.node = 102;
              return { session: 7, ...structuredClone(doc) };
            }
            if (command === "project_edit") {
              if ((window as any).failRecording)
                throw new Error("Recording write failed");
              if ((window as any).delayRecording)
                await new Promise((resolve) => setTimeout(resolve, 750));
              return {
                history: {
                  revision: 2,
                  dirty: true,
                  undo: args.label,
                  created: [],
                },
                document: structuredClone(doc),
              };
            }
            throw new Error(`Unexpected IPC: ${command}`);
          },
        },
      });
    },
    { xar },
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
const robotCalls = async (page: Page) =>
  (await calls(page, "robot_call")).filter(
    (c: any) => !["getData", "getRunningBehaviors"].includes(c.method),
  );

async function run(page: Page, title: string) {
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(title);
  await page.getByRole("button", { name: new RegExp(`^${title}`) }).click();
}

async function startRobot(page: Page) {
  await run(page, "Start virtual NAO 6");
  await expect(page.locator(".connection-status")).toHaveText("Virtual");
  await page.evaluate(() =>
    (window as any).emit("robot://state", {
      angles: [0.5, 0.3],
      stiffness: [0, 1],
    }),
  );
}

test("robot view says why it falls back to the simple model", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("test-mesh-error", "1"));
  await backend(page);
  await startRobot(page);
  await expect(page.getByRole("img", { name: "3-D robot view" })).toHaveAttribute("data-model", "primitives");
  await expect(page.getByRole("status").filter({ hasText: "NAO meshes could not be loaded" })).toContainText("Choose the Choregraphe installation in Settings");
});

test("robot view loads install meshes; poses apply and store; memory keys are watched", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await backend(page);
  await startRobot(page);
  expect((await calls(page, "robot_start_virtual"))[0]).toEqual({
    model: "nao6",
    shareDir: "/share/choregraphe",
  });
  await expect(
    page.getByRole("img", { name: "3-D robot view" }),
  ).toHaveAttribute("data-model", "meshes");
  expect(
    [
      ...new Set((await calls(page, "robot_mesh")).map((c: any) => c.name)),
    ].sort(),
  ).toEqual(["naoHeadYaw", "naoTorsoV6"]);

  await run(page, "Show Pose library");
  await page.getByRole("button", { name: "Apply Stand" }).click();
  await expect
    .poll(() => robotCalls(page))
    .toEqual([
      {
        service: "ALMotion",
        method: "angleInterpolationWithSpeed",
        args: [["HeadYaw", "LHand"], [Math.PI / 6, 0.5], 0.3],
      },
    ]);
  await page.getByRole("textbox", { name: "New pose name" }).fill("Look");
  await page.getByRole("button", { name: "Store", exact: true }).click();
  await expect(page.getByRole("button", { name: "Delete Look" })).toBeVisible();
  await expectTidyLayout(page);
  const [write] = await calls(page, "poses_write");
  expect(write.path).toBe("/u/poses.xap");
  expect(write.pose.name).toBe("Look");
  expect(write.pose.motors[0][0]).toBe("HeadYaw");
  expect(write.pose.motors[0][1]).toBeCloseTo((0.5 * 180) / Math.PI, 4);
  expect(write.pose.motors[1]).toEqual(["LHand", 0.3]);

  await run(page, "Show Memory watcher");
  await expect(
    page.getByRole("table", { name: "Watched memory keys" }),
  ).toContainText("Choren/Test0.42");
  await expectTidyLayout(page);
  await page.getByRole("combobox", { name: "Memory key" }).fill("Choren/Other");
  await page.getByRole("button", { name: "Watch", exact: true }).click();
  await expect
    .poll(async () =>
      (await calls(page, "robot_call")).some(
        (c: any) => c.method === "getData" && c.args[0] === "Choren/Other",
      ),
    )
    .toBe(true);
  expect(errors).toEqual([]);
});

test("motion recording relaxes chains, places joints and stores timeline keys", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await expect(page.locator(".status-mode")).toHaveText("Saved");
  await startRobot(page);
  await run(page, "Show Motion recording");
  const toolbar = page.getByRole("toolbar", { name: "Recording" });
  await toolbar.getByRole("button", { name: "Stiffness off" }).click();
  await expect(page.getByText("HeadYaw · relaxed")).toBeVisible();
  await page.getByRole("slider", { name: "HeadYaw" }).fill("1");
  await toolbar.getByRole("button", { name: "Store at 1" }).click();
  await expect
    .poll(async () => (await calls(page, "project_edit")).length)
    .toBe(1);
  const [edit] = await calls(page, "project_edit");
  expect(edit.label).toBe("Store joints");
  expect(edit.commands[0].type).toBe("recordKeys");
  expect(edit.commands[0].timeline).toBe(101);
  expect(edit.commands[0].frame).toBe(1);
  expect(edit.commands[0].values[0][0]).toBe("HeadYaw");
  expect(edit.commands[0].values[0][1]).toBeCloseTo((0.5 * 180) / Math.PI, 4);
  expect(await robotCalls(page)).toEqual([
    { service: "ALMotion", method: "setStiffnesses", args: [["Head"], 0] },
    { service: "ChorenSim", method: "setAngles", args: [["HeadYaw"], [1]] },
  ]);
  expect(errors).toEqual([]);
});

test("virtual memory events validate JSON and send typed values", async ({
  page,
}) => {
  await backend(page);
  await startRobot(page);
  await page
    .getByRole("button", { name: "Memory & events", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Event name", exact: true })
    .fill("Choren/Trigger");
  await page
    .getByRole("textbox", { name: "Event value (JSON)" })
    .fill("bad json");
  await page.getByRole("button", { name: "Raise event" }).click();
  await expect(
    page
      .getByRole("form", { name: "Simulate memory event" })
      .getByRole("alert"),
  ).toContainText("Enter a JSON value");
  expect(await robotCalls(page)).toEqual([]);
  await page
    .getByRole("textbox", { name: "Event value (JSON)" })
    .fill('["hello", 0.8]');
  await page.getByRole("button", { name: "Raise event" }).click();
  await expect
    .poll(() => robotCalls(page))
    .toEqual([
      {
        service: "ALMemory",
        method: "raiseEvent",
        args: ["Choren/Trigger", ["hello", 0.8]],
      },
    ]);
});

test("recording serializes writes and stops on failure or disconnect", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await startRobot(page);
  await page
    .getByRole("button", { name: "Joints & recording", exact: true })
    .click();
  const toolbar = page.getByRole("toolbar", { name: "Recording" });
  await page.evaluate(() => ((window as any).delayRecording = true));
  await toolbar.getByRole("button", { name: "Record", exact: true }).click();
  await expect
    .poll(async () => (await calls(page, "project_edit")).length)
    .toBe(1);
  await page.waitForTimeout(550);
  expect(await calls(page, "project_edit")).toHaveLength(1);
  await page.evaluate(() => ((window as any).failRecording = true));
  await expect(
    toolbar.getByRole("button", { name: "Record", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Could not store motion. Recording stopped."),
  ).toBeVisible();
  const afterFailure = (await calls(page, "project_edit")).length;
  await page.waitForTimeout(650);
  expect(await calls(page, "project_edit")).toHaveLength(afterFailure);
  await page.evaluate(() => {
    (window as any).failRecording = false;
    (window as any).delayRecording = false;
  });
  await toolbar.getByRole("button", { name: "Record", exact: true }).click();
  await page.evaluate(() => (window as any).emit("robot://disconnected", null));
  await expect(
    page.getByText("Connect to a robot to record motion."),
  ).toBeVisible();
  await page.waitForTimeout(650);
  expect(await calls(page, "project_edit")).toHaveLength(afterFailure);
});

test("recording stops when navigating to another timeline", async ({
  page,
}) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Open project", exact: true })
    .first()
    .click();
  await startRobot(page);
  await page
    .getByRole("button", { name: "Joints & recording", exact: true })
    .click();
  const toolbar = page.getByRole("toolbar", { name: "Recording" });
  expect(
    (await page
      .getByRole("tabpanel", { name: "demo", exact: true })
      .boundingBox())!.width,
  ).toBeGreaterThan(500);
  await toolbar.getByRole("button", { name: "Record", exact: true }).click();
  await page.getByRole("tab", { name: "Outline", exact: true }).click();
  await page.getByRole("button", { name: "Other", exact: true }).dblclick();
  // Navigation shows Timeline. Return to Recording to inspect the stopped take.
  await page
    .getByRole("button", { name: "Record motion", exact: true })
    .click();
  await expect(
    toolbar.getByRole("button", { name: "Record", exact: true }),
  ).toHaveAttribute("aria-pressed", "false");
  const afterNavigation = (await calls(page, "project_edit")).length;
  await page.waitForTimeout(650);
  expect(await calls(page, "project_edit")).toHaveLength(afterNavigation);
  await toolbar.getByRole("button", { name: "Store at 1" }).click();
  await expect
    .poll(
      async () =>
        (await calls(page, "project_edit")).at(-1)?.commands[0].timeline,
    )
    .toBe(102);
});

test("memory discovery, sampling period and recording expose working controls", async ({page}) => {
  await backend(page);
  await startRobot(page);
  await run(page, "Show Memory watcher");
  await page.getByRole("combobox", {name: "Memory key"}).fill("Choren/Test");
  await page.getByRole("button", {name: "Watch", exact: true}).click();
  await page.getByRole("spinbutton", {name: "Memory sampling period"}).fill("1000");
  await page.getByRole("button", {name: "Discover memory keys"}).click();
  await expect.poll(async () => (await calls(page, "robot_call")).filter((c: any) => c.method === "getDataListName").length).toBe(1);
  await page.getByRole("button", {name: "Record memory", exact: true}).click();
  await expect(page.getByRole("spinbutton", {name: "Memory sampling period"})).toBeDisabled();
  await expect(page.getByRole("button", {name: /Export [1-9]\d* samples/})).toBeEnabled();
  await page.getByRole("button", {name: "Stop recording memory", exact: true}).click();
  await expect(page.getByRole("spinbutton", {name: "Memory sampling period"})).toBeEnabled();
});

test("pose properties change folders and descriptions without replacing joint values", async ({page}) => {
  await backend(page); await startRobot(page); await run(page, "Show Pose library");
  await page.getByRole("textbox", {name: "New pose name"}).fill("Look");
  await page.getByRole("button", {name: "Store", exact: true}).click();
  await page.getByRole("button", {name: "Properties of Look"}).click();
  const modal = page.getByRole("dialog", {name: "Pose properties", exact: true});
  await modal.getByRole("textbox", {name: "Pose folder", exact: true}).fill("Gestures");
  await modal.getByRole("textbox", {name: "Pose name", exact: true}).fill("Look left");
  await modal.getByRole("textbox", {name: "Pose description", exact: true}).fill("A gentle look");
  await modal.getByRole("button", {name: "Save pose properties"}).click();
  await expect(page.getByRole("button", {name: "Properties of Look left"})).toBeVisible();
  const writes = await calls(page, "poses_write");
  expect(writes[1].previous).toEqual(["My poses", "Look"]);
  expect(writes[1].pose).toMatchObject({folder: "Gestures", name: "Look left", description: "A gentle look", motors: writes[0].pose.motors});
});

test("robot log tags and colours each level and keeps the app's errors", async ({ page }) => {
  await backend(page);
  await startRobot(page);
  await page.evaluate(() => {
    const emit = (window as any).emit;
    emit("robot://log", ["info", "ALMotion", "Wake up done"]);
    emit("robot://log", ["warning", "ALRobotPosture", "Using the built-in postures: missing graph"]);
    emit("robot://log", ["debug", "qimessaging", "Session ready"]);
    // Long lines wrap inside their own row.
    emit("robot://log", ["warning", "behavior.translator", "Fail to load translation for '/home/nao/.local/share/PackageManager/apps/robot_activities_demos-26d702' lang 'de_DE'"]);
    emit("robot://log", ["info", "audio.altexttospeech.acapela.personalData", "Eq 0 comp 0 start synthesis of \\RSPD=100\\ \\VCT=100\\ \\mrk=1\\ \\mrk=2\\ Schau mir zuerst zu! \\mrk=65533\\ \\mrk=65532\\ \\RST\\"]);
    // A behavior failure is reported in the Robot view; the log keeps it too, once.
    emit("robot://behavior", ["Greeter", "error: Script error in Greeter, line 3"]);
    emit("robot://behavior", ["Greeter", "error: Script error in Greeter, line 3"]);
  });
  await page.setViewportSize({ width: 1024, height: 700 });
  await run(page, "Show Robot logs");
  const log = page.getByRole("log");
  // Lines arriving while the panel is open lay out like the rest.
  for (const n of [1, 2, 3]) {
    await page.evaluate((n) => (window as any).emit("robot://log", ["warning", "behavior.translator", `Live ${n}: fail to load translation for '/home/nao/.local/share/PackageManager/apps/robot_activities_demos-26d702' lang 'de_DE'`]), n);
    await expect(log.locator(".log-line", { hasText: `Live ${n}:` })).toBeVisible();
  }
  // Wrapped messages make their line taller, also after the panel narrows; no line reaches into the next.
  await page.setViewportSize({ width: 900, height: 700 });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const spill = await log.evaluate((el) => [...el.querySelectorAll(".log-line")].map((line) => {
    const box = line.getBoundingClientRect();
    return Math.max(...[...line.children].map((c) => c.getBoundingClientRect().bottom)) - box.bottom;
  }));
  expect(spill.every((px) => px <= 1), `cells below their line: ${spill}`).toBe(true);
  const line = (text: string) => log.locator(".log-line", { hasText: text });
  await expect(line("Wake up done").locator(".log-level")).toHaveText("INFO");
  await expect(line("built-in postures").locator(".log-level")).toHaveText("WARN");
  await expect(line("Session ready").locator(".log-level")).toHaveText("DEBUG");
  const error = line("Script error in Greeter");
  await expect(error).toHaveCount(1);
  await expect(error.locator(".log-level")).toHaveText("ERROR");
  await expect(error.locator(".log-category")).toHaveText("Choren");
  // Each level has its own colour.
  const colour = (text: string) => line(text).locator(".log-level").evaluate((el) => getComputedStyle(el).color);
  expect(new Set([await colour("Wake up done"), await colour("built-in postures"), await colour("Script error"), await colour("Session ready")]).size).toBe(4);

  await page.getByRole("combobox", { name: "Log severity" }).selectOption("error");
  await expect(log.locator(".log-line")).toHaveCount(1);
  await expect(page.getByRole("combobox", { name: "Log severity" }).locator("option")).toHaveText(["All levels", "ERROR (1)", "WARN (5)", "INFO (2)", "DEBUG (1)"]);
  await page.getByRole("combobox", { name: "Log severity" }).selectOption("all");
  await expectTidyLayout(page, "[role=log]");
  await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
  await expectTidyLayout(page, "[role=log]");
});
