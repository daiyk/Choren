import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { expectTidyLayout } from "./layout-audit";

test.use({ storageState: { cookies: [], origins: [] } });

const share = "/Applications/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe";

/** Desktop IPC stand-in: the folder picker returns `window.picked`; only Choregraphe.app is an installation. */
async function desktop(page: Page) {
  await page.addInitScript((share) => {
    let rid = 0;
    Object.assign(window, {
      isTauri: true,
      picked: "/Users/me/Documents",
      __TAURI_INTERNALS__: {
        transformCallback: () => ++rid,
        invoke: async (command: string, args: any) => {
          if (command === "plugin:menu|new") {
            ((window as any).menuTexts ??= []).push(args.options?.text);
            return [++rid, String(rid)];
          }
          if (command === "plugin:dialog|open") return (window as any).picked;
          if (command === "library_share_dir") {
            if (args.path !== "/Applications/Choregraphe.app") throw "This folder is not a Choregraphe installation.";
            return share;
          }
          if (command === "library_tree") return [];
          if (command === "robot_status") return null;
        },
      },
    });
  }, share);
  await page.goto("/");
}

test("first launch introduces Choren and links the Choregraphe installation", async ({ page }) => {
  await desktop(page);
  const welcome = page.getByRole("dialog", { name: "Welcome to Choren" });
  await expect(welcome).toBeVisible();
  await expect(welcome).toContainText("Not linked yet");
  await expectTidyLayout(page, "dialog[open]");

  await welcome.getByRole("button", { name: "Choose Choregraphe…" }).click();
  await expect(welcome.getByRole("alert")).toHaveText("This folder is not a Choregraphe installation.");

  await page.evaluate(() => ((window as any).picked = "/Applications/Choregraphe.app"));
  await welcome.getByRole("button", { name: "Choose Choregraphe…" }).click();
  await expect(welcome).toContainText("Choregraphe linked");
  await expect(welcome).toContainText(share);
  await expect(welcome.getByRole("alert")).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem("choregraphe-share-dir"))).toBe(share);
  await expectTidyLayout(page, "dialog[open]");

  await welcome.getByRole("button", { name: "Get started" }).click();
  await expect(welcome).toBeHidden();
  await page.reload();
  await expect(page.getByRole("button", { name: "Explore example" })).toBeVisible();
  await expect(welcome).toBeHidden();

  // Help › Welcome and setup… reopens it.
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page.getByRole("textbox", { name: "Search commands" }).fill("Welcome and setup");
  await page.getByRole("button", { name: /Welcome and setup/ }).click();
  await expect(welcome).toContainText("Choregraphe linked");
});

test("skipping the setup is remembered", async ({ page }) => {
  await desktop(page);
  const welcome = page.getByRole("dialog", { name: "Welcome to Choren" });
  await welcome.getByRole("button", { name: "Skip for now" }).click();
  await expect(welcome).toBeHidden();
  await page.reload();
  await expect(page.getByRole("button", { name: "Explore example" })).toBeVisible();
  await expect(welcome).toBeHidden();
});

test("the browser preview opens without the setup window", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Explore example" })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Welcome to Choren" })).toBeHidden();
});

for (const platform of ["MacIntel", "Win32"]) {
  test(`About Choren is one menu item on ${platform} and shows the app version`, async ({ page }) => {
    const { version, bundle } = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
    await page.addInitScript((platform) => Object.defineProperty(navigator, "platform", { get: () => platform }), platform);
    await desktop(page);
    await page.getByRole("dialog", { name: "Welcome to Choren" }).getByRole("button", { name: "Skip for now" }).click();
    // macOS puts it in the Choren menu, Windows in Help; once per menu, like Welcome (the dev build installs the menu twice).
    const count = (text: string) => page.evaluate((text) => (window as any).menuTexts?.filter((t: string) => t === text).length ?? 0, text);
    await expect.poll(() => count("Welcome and setup…")).toBeGreaterThan(0);
    expect(await count("About Choren")).toBe(await count("Welcome and setup…"));

    await page.getByRole("button", { name: /Search commands/ }).click();
    await page.getByRole("textbox", { name: "Search commands" }).fill("About Choren");
    await page.getByRole("button", { name: /About Choren/ }).click();
    const about = page.getByRole("dialog", { name: "About Choren" });
    await expect(about).toContainText(`Version ${version}`);
    await expect(about).toContainText(bundle.copyright);
    await expectTidyLayout(page, "dialog[open]");
    await about.getByRole("button", { name: "Close dialog" }).click();
    await expect(about).toBeHidden();
  });
}
