import { expect, test, type Page } from "@playwright/test";
import { expectTidyLayout } from "./layout-audit";

async function backend(page: Page, panel = "Show Robot files") {
  await page.addInitScript(() => {
    let rid = 0;
    const callbacks: Record<number, (e: unknown) => void> = {};
    const listeners: Record<string, number> = {};
    const calls: { command: string; args: any }[] = [];
    const folders: Record<string, { name: string; dir: boolean; size: number }[]> = {
      "/home/nao": [{ name: "recordings", dir: true, size: 0 }, { name: "hello.wav", dir: false, size: 2048 }],
      "/home/nao/recordings": [], "/home": [],
    };
    let rejectTransfer: (reason: string) => void;
    Object.assign(window, {
      calls, isTauri: true, uploadPath: "/local/dance.wav",
      emit: (event: string, payload: unknown) => callbacks[listeners[event]]?.({ event, id: 0, payload }),
      __TAURI_INTERNALS__: {
        transformCallback: (cb: (e: unknown) => void) => { callbacks[++rid] = cb; return rid; },
        invoke: async (command: string, args: any) => {
          calls.push({ command, args });
          if (command === "plugin:menu|new") return [++rid, String(rid)];
          if (command === "plugin:event|listen") { listeners[args.event] = args.handler; return rid; }
          if (command === "plugin:dialog|open") return (window as any).uploadPath;
          if (command === "plugin:dialog|save") return "/local/saved.wav";
          if (command.startsWith("plugin:")) return;
          if (command === "library_index") return [];
          if (command === "robot_status") return null;
          if (command === "sftp_connect") {
            if ((window as any).failConnect) throw "SSH authentication error.";
            return "/home/nao";
          }
          if (command === "sftp_disconnect") {
            if ((window as any).failDisconnect) throw "Disconnect failed";
            return;
          }
          if (command === "sftp_list") {
            if ((window as any).failList) throw "Permission denied";
            return { path: args.path, entries: structuredClone(folders[args.path] ?? []) };
          }
          if (command === "sftp_upload" || command === "sftp_download") {
            await new Promise<void>((resolve, reject) => {
              rejectTransfer = reject;
              (window as any).finishTransfer = (error?: string) => error ? reject(error) : resolve();
            });
            if (command === "sftp_upload") {
              const folder = args.remote.slice(0, args.remote.lastIndexOf("/"));
              const name = args.remote.split("/").pop();
              if (!folders[folder].some((e) => e.name === name)) folders[folder].push({ name, dir: false, size: 4096 });
            }
            return;
          }
          if (command === "sftp_cancel") { rejectTransfer("Transfer canceled."); return; }
          if (command === "sftp_mkdir") {
            const folder = args.path.slice(0, args.path.lastIndexOf("/"));
            folders[folder].push({ name: args.path.split("/").pop(), dir: true, size: 0 });
            folders[args.path] = [];
            return;
          }
          if (command === "sftp_remove") {
            const folder = args.path.slice(0, args.path.lastIndexOf("/"));
            folders[folder] = folders[folder].filter((e) => e.name !== args.path.split("/").pop());
            return;
          }
          throw new Error(`Unexpected IPC: ${command}`);
        },
      },
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill(panel);
  await page.getByRole("button", { name: panel, exact: false }).last().click();
}
const calls = (page: Page, command: string) => page.evaluate((c) => (window as any).calls.filter((x: any) => x.command === c).map((x: any) => x.args), command);
async function connect(page: Page) {
  await page.getByLabel("Robot address", { exact: true }).fill("192.168.1.20");
  await page.getByLabel("Password", { exact: true }).fill("secret");
  await page.getByRole("button", { name: "Connect to files" }).click();
  await expect(page.getByRole("region", { name: "Robot file browser" })).toBeVisible();
}

test("SFTP connects independently, reports login failures, and clears the password", async ({ page }, info) => {
  await backend(page);
  await page.getByLabel("Robot address", { exact: true }).fill("192.168.1.20");
  await page.getByLabel("Password", { exact: true }).fill("wrong");
  await page.evaluate(() => (window as any).failConnect = true);
  await page.getByRole("button", { name: "Connect to files" }).click();
  await expect(page.locator(".files-connect, .files-panel").getByRole("alert")).toHaveText("SSH authentication error.");
  await page.screenshot({ path: info.outputPath("files-login.png") });
  await page.evaluate(() => (window as any).failConnect = false);
  await connect(page);
  expect((await calls(page, "sftp_connect")).at(-1)).toEqual({ host: "192.168.1.20", user: "nao", password: "secret" });
  await page.evaluate(() => (window as any).failDisconnect = true);
  await page.getByRole("button", { name: "Disconnect files" }).click();
  await expect(page.locator(".files-connect, .files-panel").getByRole("alert")).toHaveText("Disconnect failed");
  await page.evaluate(() => (window as any).failDisconnect = false);
  await page.getByRole("button", { name: "Disconnect files" }).click();
  await expect(page.getByLabel("Password", { exact: true })).toHaveValue("");
});

test("SFTP uploads with progress, locks navigation, and supports cancellation and retry", async ({ page }, info) => {
  await backend(page);
  await connect(page);
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect.poll(() => calls(page, "sftp_upload")).toEqual([{ local: "/local/dance.wav", remote: "/home/nao/dance.wav" }]);
  await expect(page.getByRole("button", { name: "Cancel transfer" })).toBeVisible();
  await expect(page.getByRole("button", { name: "recordings", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Parent folder" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Disconnect files" })).toBeDisabled();
  await page.getByRole("row", { name: "recordings Folder" }).dblclick();
  expect(await calls(page, "sftp_list")).not.toContainEqual({ path: "/home/nao/recordings" });
  await page.evaluate(() => (window as any).emit("sftp://progress", [2048, 4096]));
  await expect(page.getByRole("progressbar")).toHaveAttribute("value", "2048");
  await page.screenshot({ path: info.outputPath("files-upload.png") });
  await page.getByRole("button", { name: "Cancel transfer" }).click();
  await expect(page.locator(".files-panel").getByRole("status")).toContainText("Transfer canceled.");
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect.poll(() => calls(page, "sftp_upload")).toHaveLength(2);
  await page.evaluate(() => (window as any).finishTransfer());
  await expect(page.getByRole("button", { name: "dance.wav", exact: true })).toBeVisible();
  await expect(page.locator(".files-panel").getByRole("status")).toContainText("Uploaded dance.wav");
});

test("SFTP confirms replacement, handles failures, and leaves a canceled picker alone", async ({ page }) => {
  await backend(page);
  await connect(page);
  await page.evaluate(() => (window as any).uploadPath = null);
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect(page.getByRole("button", { name: "Upload file", exact: true })).toBeEnabled();
  expect(await calls(page, "sftp_upload")).toEqual([]);
  await page.evaluate(() => (window as any).uploadPath = "/local/hello.wav");
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Replace file?" });
  await expect(modal).toContainText("cannot be restored");
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(await calls(page, "sftp_upload")).toEqual([]);
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await modal.getByRole("button", { name: "Replace file", exact: true }).click();
  await expect.poll(() => calls(page, "sftp_upload")).toHaveLength(1);
  await page.evaluate(() => (window as any).finishTransfer("Disk full"));
  await expect(page.locator(".files-connect, .files-panel").getByRole("alert")).toHaveText("Disk full");
  await expect(page.getByRole("button", { name: "Upload file", exact: true })).toBeEnabled();
  await page.evaluate(() => (window as any).uploadPath = "/local/recordings");
  await page.getByRole("button", { name: "Upload file", exact: true }).click();
  await expect(page.locator(".files-connect, .files-panel").getByRole("alert")).toContainText("A folder named “recordings” already exists");
  expect(await calls(page, "sftp_upload")).toHaveLength(1);
});

test("SFTP browses folders, downloads, creates and deletes with keyboard access", async ({ page }) => {
  await backend(page);
  await connect(page);
  await page.getByRole("button", { name: "hello.wav", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page, "sftp_download")).toEqual([{ remote: "/home/nao/hello.wav", local: "/local/saved.wav" }]);
  await page.evaluate(() => (window as any).finishTransfer());
  await expect(page.locator(".files-panel").getByRole("status")).toContainText("Downloaded hello.wav");
  await page.getByRole("button", { name: "recordings", exact: true }).click();
  await expect(page.getByText("This folder is empty", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Parent folder" }).click();
  await expect(page.getByRole("button", { name: "hello.wav", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "New folder", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New folder", exact: true });
  await modal.getByLabel("Folder name").fill("..");
  await expect(modal.getByRole("button", { name: "Create folder" })).toBeDisabled();
  await modal.getByLabel("Folder name").fill("sounds");
  await modal.getByRole("button", { name: "Create folder" }).click();
  await expect.poll(() => calls(page, "sftp_mkdir")).toEqual([{ path: "/home/nao/sounds" }]);
  await page.getByRole("button", { name: "sounds", exact: true }).focus();
  await page.getByRole("button", { name: "Delete selected item" }).click();
  await page.getByRole("dialog", { name: "Delete item?" }).getByRole("button", { name: "Delete", exact: true }).click();
  await expect.poll(() => calls(page, "sftp_remove")).toEqual([{ path: "/home/nao/sounds", dir: true }]);
  await expect(page.getByRole("button", { name: "sounds", exact: true })).toHaveCount(0);
});

test("SFTP keeps its actions visible in a compact dark workspace", async ({ page }, info) => {
  await page.setViewportSize({ width: 1024, height: 640 });
  await backend(page);
  await connect(page);
  await page.evaluate(() => document.documentElement.dataset.theme = "dark");
  const panel = page.getByRole("region", { name: "Robot file browser" });
  await expect(page.getByRole("button", { name: "Upload file", exact: true })).toBeInViewport();
  await expect(page.getByRole("button", { name: "New folder", exact: true })).toBeInViewport();
  expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expectTidyLayout(page);
  await page.screenshot({ path: info.outputPath("files-compact-dark.png") });
});


test("Robot view opens SFTP without requiring a behavior connection", async ({ page }) => {
  await backend(page, "Show Robot view");
  await page.getByRole("button", { name: "Robot files…", exact: true }).click();
  await expect(page.getByRole("form", { name: "SFTP connection" })).toBeVisible();
  expect(await calls(page, "robot_connect")).toEqual([]);
});
