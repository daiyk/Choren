import { expect, test, type Page } from "@playwright/test";

async function setup(page: Page, connected = false) {
  await page.addInitScript(({connected}) => {
    localStorage.setItem("choren-user-poses", JSON.stringify("/user/poses.xap"));
    localStorage.setItem("choren-memory-keys", JSON.stringify(["Test/Value"]));
    let id = 0;
    const callbacks: Record<number, (e: unknown) => void> = {};
    const listeners: Record<string, number> = {};
    const calls: {command: string; args: any}[] = [];
    const pose = {folder: "User", name: "Stored pose", description: "", bitmap: "", motors: [["HeadYaw", 0]]};
    let poses = [pose];
    let running = false;
    let project: any;
    let revision = 1;
    Object.assign(window, {
      calls, isTauri: true,
      emit: (event: string, payload: unknown) => callbacks[listeners[event]]?.({event, id: 0, payload}),
      __TAURI_INTERNALS__: {
        transformCallback: (callback: (e: unknown) => void) => { callbacks[++id] = callback; return id; },
        invoke: async (command: string, args: any) => {
          calls.push({command, args});
          if (command === "plugin:menu|new") return [++id, String(id)];
          if (command === "plugin:event|listen") {listeners[args.event] = args.handler; return id;}
          if (command === "plugin:dialog|open") return "/project/demo.xar";
          if (command === "plugin:dialog|save") return "/local/saved.dat";
          if (command.startsWith("plugin:")) return;
          if (command === "project_open") {
            const {exampleProject} = await import(/* @vite-ignore */ `${location.origin}/src/workspace/example.ts`);
            project = exampleProject();
            project.session = 7;
            project.files.push({path: "note.txt", kind: "resource"});
            let next = 100;
            const walk = (box: any) => {
              box.node = next++;
              [...box.inputs, ...box.outputs, ...box.params, ...box.resources].forEach(item => item.node = next++);
              box.timeline?.layers.forEach((layer: any) => layer.keyframes.forEach((frame: any) => {
                if (!frame.diagram) return;
                frame.diagram.node = next++;
                frame.diagram.boxes.forEach(walk);
              }));
            };
            project.behaviors.forEach((behavior: any) => walk(behavior.root));
            return structuredClone(project);
          }
          if (command === "project_read_text") return "Original file";
          if (["project_edit", "project_resource", "project_write_text", "project_undo"].includes(command)) {
            if (command === "project_resource") {
              if (args.target) project.files.find((file: any) => file.path === args.path).path = args.target;
              else project.files = project.files.filter((file: any) => file.path !== args.path);
            }
            return {document: structuredClone(project), history: {revision: ++revision, dirty: true, undo: args.label ?? "Edit resource", created: []}};
          }
          if (["authorize_project_root", "project_close"].includes(command)) return;
          if (command === "library_tree") return [];
          if (command === "robot_status") return connected ? {url: "tcp://localhost:9559", model: "nao6", virtual: true} : null;
          if (command === "robot_descriptor") return {id: "nao6", name: "Test robot", bodyNames: ["HeadYaw"], postures: [], touch: [], tips: {}, joints: [{name: "HeadYaw", parent: null, origin: [0,0,0.1], axis: [0,0,1], min: -2, max: 2, chain: "Head"}], meshes: {}};
          if (command === "robot_call") {
            if (args.method === "getInstalledBehaviors") return ["demo/dance"];
            if (args.method === "getRunningBehaviors") return running ? ["demo/dance"] : [];
            if (args.method === "getActivatedTopics") return [];
            if (args.method === "startBehavior") {running = true; return;}
            if (args.method === "stopBehavior") {running = false; return;}
            if (args.method === "getData") return 42;
            if (args.method === "getLanguage") return "English";
            if (args.method === "getAvailableLanguages") return ["English"];
            if (args.method === "echo") return "reply";
            return null;
          }
          if (command === "robot_services") return args.service ? {description: "Service", methods: [{id: 1, name: "echo", parameters: "()", returns: "s"}], signals: [{name: "changed", signature: "(s)"}]} : ["Example"];
          if (command === "poses_read") return poses;
          if (command === "poses_write") {poses = []; return poses;}
          if (command === "sftp_connect") return "/home/robot";
          if (command === "sftp_list") return {path: args.path, entries: [{name: "assets", dir: true, size: 0}, {name: "note.txt", dir: false, size: 10}]};
          if (["sftp_disconnect", "sftp_remove", "sftp_download"].includes(command)) return;
          throw new Error(`Unexpected IPC: ${command}`);
        },
      },
    });
  }, {connected});
  await page.goto("/");
}
async function show(page: Page, name: string) {
  await page.getByRole("button", {name: /Search commands/}).click();
  await page.getByRole("textbox", {name: "Search commands"}).fill(`Show ${name}`);
  await page.getByRole("button", {name: `Show ${name}`, exact: false}).last().click();
}
const calls = (page: Page, command: string) => page.evaluate(c => (window as any).calls.filter((x: any) => x.command === c).map((x: any) => x.args), command);
const menu = (page: Page) => page.getByRole("menu", {name: "Context actions"});

test("file menus target clicked entries and preserve remote-delete confirmation", async ({page}) => {
  await setup(page);
  await show(page, "Robot files");
  await page.getByLabel("Robot address", {exact: true}).fill("robot.local");
  await page.getByRole("button", {name: "Connect to files"}).click();
  await page.getByRole("row", {name: "note.txt 10 B"}).click();
  const folder = page.getByRole("row", {name: "assets Folder"});
  await folder.click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Open folder"})).toBeEnabled();
  await expect(menu(page).getByRole("menuitem", {name: "Download…"})).toHaveCount(0);
  await menu(page).getByRole("menuitem", {name: "Delete empty folder…"}).click();
  await expect(page.getByRole("dialog", {name: "Delete item?"})).toContainText("assets");
  expect(await calls(page, "sftp_remove")).toEqual([]);
  await page.getByRole("button", {name: "Cancel", exact: true}).click();
  await folder.focus();
  await page.keyboard.press("Shift+F10");
  await page.keyboard.press("Escape");
  await expect(folder).toBeFocused();
  await folder.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Delete empty folder…"}).click();
  await page.getByRole("dialog", {name: "Delete item?"}).getByRole("button", {name: "Delete", exact: true}).click();
  await expect.poll(() => calls(page, "sftp_remove")).toEqual([{path: "/home/robot/assets", dir: true}]);
});

test("memory row removal works offline while recording stays unavailable", async ({page}) => {
  await setup(page);
  await show(page, "Memory watcher");
  const row = page.getByRole("row", {name: /Test\/Value/});
  await row.focus();
  await page.keyboard.press("Shift+F10");
  await expect(menu(page).getByRole("menuitem", {name: "Copy value"})).toBeDisabled();
  await menu(page).getByRole("menuitem", {name: "Stop watching"}).click();
  await expect(row).toHaveCount(0);
  await page.getByText("Connect to a robot to read its memory.").click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Record memory", exact: true})).toBeDisabled();
  expect(await calls(page, "robot_call")).toEqual([]);
});

test("pose deletion asks before writing and disconnected apply is disabled", async ({page}) => {
  await setup(page);
  await show(page, "Pose library");
  const row = page.getByRole("listitem").filter({hasText: "Stored pose"});
  await row.click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Apply pose"})).toBeDisabled();
  page.once("dialog", dialog => dialog.dismiss());
  await menu(page).getByRole("menuitem", {name: "Delete pose…"}).click();
  expect(await calls(page, "poses_write")).toEqual([]);
  await row.click({button: "right"});
  page.once("dialog", dialog => dialog.accept());
  await menu(page).getByRole("menuitem", {name: "Delete pose…"}).click();
  await expect.poll(() => calls(page, "poses_write")).toEqual([{path: "/user/poses.xap", remove: ["User", "Stored pose"]}]);
});

test("application menus use run and stop handlers; virtual camera remains disabled", async ({page}) => {
  await setup(page, true);
  await show(page, "Robot applications");
  const row = page.locator(".panel-toolbar").filter({hasText: "demo/dance"});
  await row.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Run behavior"}).click();
  await expect(row.getByRole("button", {name: "Stop", exact: true})).toBeVisible();
  await row.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Stop behavior"}).click();
  await expect(row.getByRole("button", {name: "Run", exact: true})).toBeVisible();
  await show(page, "Camera");
  await page.getByLabel("Robot camera image").click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Start camera"})).toBeDisabled();
  await expect(menu(page).getByRole("menuitemcheckbox", {name: "Top camera"})).toBeChecked();
  await menu(page).getByRole("menuitemcheckbox", {name: "Bottom camera"}).click();
  await expect(page.getByLabel("Robot camera", {exact: true})).toHaveValue("1");
});

test("service copy menu never calls methods and text fields keep native menus", async ({page}) => {
  await setup(page, true);
  await show(page, "Service inspector");
  await page.getByLabel("Service result").click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Copy result"})).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.getByLabel("Method arguments").click({button: "right"});
  await expect(menu(page)).toHaveCount(0);
  expect((await calls(page, "robot_call")).filter((x: any) => x.method === "echo")).toEqual([]);
});

test("log menu closes on new data and clear uses the existing log store", async ({page}) => {
  await setup(page, true);
  await show(page, "Robot logs");
  await page.evaluate(() => (window as any).emit("robot://log", ["info", "Test", "First entry"]));
  const line = page.locator(".log-line").filter({hasText: "First entry"});
  await line.click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Copy message"})).toBeVisible();
  await page.evaluate(() => (window as any).emit("robot://log", ["warning", "Test", "Second entry"]));
  await expect(menu(page)).toHaveCount(0);
  await page.getByRole("log").click({button: "right", position: {x: 8, y: 80}});
  await menu(page).getByRole("menuitem", {name: "Clear logs"}).click();
  await expect(page.locator(".log-line")).toHaveCount(0);
});


test("Inspector targets parameter properties and deletion without changing graph selection", async ({page}) => {
  await setup(page);
  await page.getByRole("button", {name: "Open project", exact: true}).first().click();
  await page.locator(".react-flow__node-box").filter({hasText: "Say hello"}).click();
  const parameter = page.locator(".box-inspector .parameter").filter({hasText: "Message"});
  await parameter.click({button: "right", position: {x: 4, y: 4}});
  expect(await calls(page, "project_edit")).toEqual([]);
  await expect(menu(page).getByRole("menuitem", {name: "Move up"})).toBeDisabled();
  await menu(page).getByRole("menuitem", {name: "Parameter properties"}).click();
  await expect(page.getByLabel("Message parameter name")).toBeFocused();
  await parameter.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Delete parameter"}).click();
  await expect.poll(() => calls(page, "project_edit")).toEqual([expect.objectContaining({label: "Remove parameter: Message", commands: [{type: "remove", nodes: [107]}]})]);
});

test("resource header rename preserves drafts and remove requires confirmation", async ({page}) => {
  await setup(page);
  await page.getByRole("button", {name: "Open project", exact: true}).first().click();
  await show(page, "Resource editor");
  await page.getByLabel("Project resource").last().selectOption("note.txt");
  const text = page.getByRole("textbox", {name: "File text", exact: true}).last();
  await expect(text).toHaveValue("Original file");
  await text.fill("Unapplied draft");
  const toolbar = page.locator(".panel-toolbar").filter({has: page.getByLabel("Project resource")}).last();
  await toolbar.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Rename…"}).click();
  await page.getByLabel("Resource path").fill("renamed.txt");
  await page.getByRole("dialog", {name: "Rename resource"}).getByRole("button", {name: "Rename", exact: true}).click();
  await expect(page.getByLabel("Project resource").last()).toHaveValue("renamed.txt");
  await expect(text).toHaveValue("Unapplied draft");
  await toolbar.focus();
  await page.keyboard.press("Shift+F10");
  page.once("dialog", dialog => dialog.dismiss());
  await menu(page).getByRole("menuitem", {name: "Remove from project…"}).click();
  expect(await calls(page, "project_resource")).toEqual([{session: 7, path: "note.txt", target: "renamed.txt"}]);
});


test("active-content menu stops the targeted behavior and keeps completed history inert", async ({page}) => {
  await setup(page, true);
  await show(page, "Robot applications");
  await page.getByRole("button", {name: "Run", exact: true}).click();
  await show(page, "Active content");
  const row = page.getByRole("row").filter({hasText: "demo/dance"});
  await row.getByText("demo/dance", {exact: true}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Stop active item"}).click();
  await expect(row).toContainText("Stopped");
  await row.focus();
  await page.keyboard.press("Shift+F10");
  await expect(menu(page).getByRole("menuitem", {name: "Stop active item"})).toBeDisabled();
  await expect.poll(async () => (await calls(page, "robot_call")).filter((call: any) => call.method === "stopBehavior")).toEqual([{service: "ALBehaviorManager", method: "stopBehavior", args: ["demo/dance"]}]);
});

test("robot conversation offers row copy and clear without sending input", async ({page}) => {
  await setup(page, true);
  await show(page, "Robot dialog");
  await page.evaluate(() => (window as any).emit("robot://dialog-answer", ["Hello there"]));
  const transcript = page.getByRole("log", {name: "Robot conversation"});
  await transcript.getByText("Hello there", {exact: false}).click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Copy message"})).toBeVisible();
  await page.keyboard.press("Escape");
  await transcript.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Clear conversation"}).click();
  await expect(transcript).toHaveText("");
  expect((await calls(page, "robot_call")).filter((call: any) => call.method === "forceInput")).toEqual([]);
});
