import { expect, test, type Page } from "@playwright/test";

async function backend(page: Page, format = "pml") {
  await page.addInitScript((format) => {
    let rid = 0;
    let doc: any;
    const calls: { command: string; args: any }[] = [];
    const picks: unknown[] = ["/project/demo.pml"];
    const menuItems: Record<number, any> = {};
    Object.assign(window, {
      calls, picks, menuItems, isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => ++rid,
        invoke: async (command: string, args: any) => {
          calls.push({ command, args });
          if (command === "plugin:menu|new") {
            menuItems[++rid] = args;
            return [rid, String(rid)];
          }
          if (command === "plugin:dialog|open") return picks.shift() ?? null;
          if (command.startsWith("plugin:") || command === "authorize_project_root") return;
          if (command === "library_tree") return [];
          if (command === "robot_status") return null;
          if (command === "project_open") {
            const { openProject } = await import(/* @vite-ignore */ `${location.origin}/src/format/project.ts`);
            const xml = '<Package name="My project"><BehaviorDescriptions/></Package>';
            doc = { ...await openProject("/project/demo.pml", async () => new TextEncoder().encode(xml), async () => true), format, session: 7 };
            return structuredClone(doc);
          }
          if (command === "project_import_files" || command === "project_import_behavior") {
            if ((window as any).failImport) throw new Error("Destination already exists. Choose a different destination.");
            const behaviorFolder = command === "project_import_files" ? (window as any).folderBehavior : args.name;
            if (command === "project_import_files") {
              const names = args.folder ? ["media/nested/note.txt"] : args.paths.map((p: string) => p.split("/").pop());
              doc.files.push(...names.map((name: string) => ({ path: `${args.destination ? `${args.destination}/` : ""}${name}`, kind: "resource" })));
            }
            if (behaviorFolder) {
              const { openProject } = await import(/* @vite-ignore */ `${location.origin}/src/format/project.ts`);
              const source = `${behaviorFolder}/behavior.xar`;
              const xml = '<ChoregrapheProject><Box id="-1" name="Imported"><Timeline enable="0"><BehaviorLayer name="Flow"><BehaviorKeyframe name="Start" index="1"><Diagram/></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>';
              const imported = await openProject(`/project/${source}`, async () => new TextEncoder().encode(xml), async () => true);
              doc.behaviors.push({ ...imported.behaviors[0], id: `/project/${source}`, name: behaviorFolder, source });
              doc.files.push({ path: source, kind: "behavior", behaviorId: `/project/${source}` });
            }
            return { history: { revision: calls.length, dirty: true, created: [] }, document: structuredClone(doc) };
          }
          if (command === "project_save") return { revision: calls.length, dirty: false, created: [] };
          throw new Error(`Unexpected IPC: ${command}`);
        },
      },
    });
  }, format);
  await page.goto("/");
  await page.getByRole("button", { name: "Open project", exact: true }).first().click();
  await expect(page.locator(".project-heading")).toContainText("My project");
}

test("imports multiple files and folder with destination, failure and picker cancellation", async ({ page }) => {
  await backend(page);
  await expect(page.getByRole("button", { name: "Import files…", exact: true })).toBeHidden();
  await page.getByRole("button", { name: "Import content", exact: true }).click();
  await page.getByRole("button", { name: "Import files…", exact: true }).click();
  const files = page.getByRole("dialog", { name: "Import files", exact: true });
  await files.getByRole("button", { name: "Choose files…", exact: true }).click();
  await expect(files.getByRole("button", { name: "Import files", exact: true })).toBeDisabled();
  await page.evaluate(() => (window as any).picks.push(["/outside/声音.wav", "/outside/photo.png"]));
  await files.getByRole("button", { name: "Choose files…", exact: true }).click();
  await expect(files.getByRole("list", { name: "Selected sources" })).toContainText("声音.wav");
  await files.getByRole("textbox", { name: "Destination folder" }).fill("assets");
  await page.evaluate(() => ((window as any).failImport = true));
  await files.getByRole("button", { name: "Import files", exact: true }).click();
  await expect(files.getByRole("alert")).toContainText("already exists");
  await expect(page.locator(".project-heading")).toContainText("My project");
  await expect(files.getByRole("textbox", { name: "Destination folder" })).toHaveValue("assets");
  await page.evaluate(() => ((window as any).failImport = false));
  await files.getByRole("button", { name: "Import files", exact: true }).click();
  await expect(files).toBeHidden();
  await expect(page.locator('.file-tree button[title="assets/声音.wav"]')).toBeVisible();
  await expect(page.locator(".project-heading")).toContainText("Unsaved changes");
  await page.getByRole("button", { name: "Import content", exact: true }).click();
  await page.getByRole("button", { name: "Import folder…", exact: true }).click();
  const folder = page.getByRole("dialog", { name: "Import folder", exact: true });
  await page.evaluate(() => (window as any).picks.push("/outside/media"));
  await folder.getByRole("button", { name: "Choose folder…", exact: true }).click();
  await page.screenshot({ path: "/tmp/choren-import-folder.png" });
  await folder.getByRole("button", { name: "Import folder", exact: true }).click();
  await expect(page.locator('.file-tree button[title="media/nested/note.txt"]')).toBeVisible();
  const imports = await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === "project_import_files"));
  expect(imports[1].args).toEqual({ session: 7, paths: ["/outside/声音.wav", "/outside/photo.png"], destination: "assets", folder: false });
  expect(imports[2].args).toEqual({ session: 7, paths: ["/outside/media"], destination: "", folder: true });
  await page.getByRole("button", { name: "Save project", exact: true }).click();
  await expect(page.locator(".project-heading")).toContainText("Editable");
});

test("behavior import opens the added behavior without replacing the project", async ({ page }) => {
  await backend(page);
  await expect.poll(() => page.evaluate(() => {
    const menus = (window as any).menuItems;
    const file: any = Object.values(menus).find((item: any) => item.kind === "Submenu" && item.options.text === "File");
    return file?.options.items.map(([rid]: [number]) => menus[rid].options.id).filter((id: string) => id?.startsWith("import-"));
  })).toEqual(["import-behavior", "import-files", "import-folder"]);
  const more = page.getByRole("button", { name: "Import content", exact: true });
  const menu = page.getByRole("group", { name: "Import content", exact: true });
  await more.click();
  await expect(menu).toBeVisible();
  await page.screenshot({ path: `/tmp/choren-import-menu-${test.info().project.name}.png` });
  await page.locator(".project-heading").click();
  await expect(menu).toBeHidden();
  await more.focus();
  await page.keyboard.press("Enter");
  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(more).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Import behavior…", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Import behavior", exact: true });
  await page.evaluate(() => (window as any).picks.push("/outside/Dance/behavior.xar"));
  await dialog.getByRole("button", { name: "Choose behavior file…", exact: true }).click();
  await expect(dialog.getByRole("textbox", { name: "Behavior folder name" })).toHaveValue("Dance");
  await dialog.getByRole("button", { name: "Import behavior", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(menu).toBeHidden();
  await expect(page.locator(".project-heading")).toContainText("My project");
  await expect(page.locator(".breadcrumbs")).toContainText("Dance");
  expect(await page.evaluate(() => (window as any).calls.find((c: any) => c.command === "project_import_behavior").args)).toEqual({ session: 7, path: "/outside/Dance/behavior.xar", name: "Dance" });
});

test("folder import opens the behavior it brings and lists it in the Outline", async ({ page }) => {
  await backend(page);
  await page.evaluate(() => ((window as any).folderBehavior = "show/behavior_1"));
  await page.getByRole("button", { name: "Import content", exact: true }).click();
  await page.getByRole("button", { name: "Import folder…", exact: true }).click();
  const folder = page.getByRole("dialog", { name: "Import folder", exact: true });
  await expect(folder).toContainText("Each behavior.xar becomes a behavior");
  await page.evaluate(() => (window as any).picks.push("/outside/show"));
  await folder.getByRole("button", { name: "Choose folder…", exact: true }).click();
  await folder.getByRole("button", { name: "Import folder", exact: true }).click();
  await expect(folder).toBeHidden();
  await expect(page.locator(".notice")).toContainText("Folder imported with 1 behavior");
  await expect(page.locator(".breadcrumbs")).toContainText("show/behavior_1");
  await page.getByRole("tab", { name: "Outline", exact: true }).click();
  await expect(page.locator(".outline-owner summary", { hasText: "show/behavior_1" })).toBeVisible();
});

test("imports explain the saved-project prerequisite", async ({ page }) => {
  await backend(page, "crg");
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill("Import files");
  const command = page.getByRole("dialog").getByRole("button", { name: /Import files/ });
  await expect(command).toBeDisabled();
  await expect(command).toContainText("Save as project folder first");
});
