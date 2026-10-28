import { expect, test, type Locator, type Page } from "@playwright/test";
import {editorProject, calls, lastEdit, menu} from "./editor-fixture";

const folder = (page: Page, name: string) => page.locator(".file-tree summary", {hasText: new RegExp(`^${name}$`)});
// Playwright's WebKit drag emulation leaves the button held after dragTo; release it.
const drag = async (page: Page, source: Locator, target: Locator, targetPosition: {x: number; y: number}) => {
  await source.dragTo(target, {targetPosition});
  await page.mouse.up();
};
const treeOrder = (page: Page) => page.locator(".file-tree > li > details > summary").allTextContents();

test("Project and Outline menus rename, duplicate and copy behaviors", async ({page}) => {
  await editorProject(page, true);
  await page.evaluate(() => { (navigator.clipboard as any).writeText = async (text: string) => { (window as any).copied = text; }; });
  const other = page.locator('button.tree-row[title="other/behavior.xar"]');
  await other.click();
  await expect(page.getByRole("tab", {name: /^Other behavior/})).toBeVisible();
  await other.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Copy path"}).click();
  expect(await page.evaluate(() => (window as any).copied)).toBe("other");
  await other.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Rename behavior…"}).click();
  const rename = page.getByRole("dialog", {name: "Rename behavior"});
  await expect(rename.getByRole("textbox", {name: "New name"})).toHaveValue("other");
  await expect(rename.getByRole("button", {name: "Rename", exact: true})).toBeDisabled();
  await rename.getByRole("textbox", {name: "New name"}).fill("bad/name");
  await expect(rename).toContainText("cannot contain /");
  await rename.getByRole("textbox", {name: "New name"}).fill("Dance");
  await expect(rename.locator(".import-hint")).toHaveText("Renamed to Dance");
  await expect(rename.locator(".import-tip")).toContainText("not updated");
  await rename.getByRole("button", {name: "Rename", exact: true}).click();
  await expect.poll(() => calls(page, "project_move_path")).toEqual([{session: 7, from: "other", to: "Dance"}]);
  await expect(rename).toHaveCount(0);
  await expect(page.locator('button.tree-row[title="Dance/behavior.xar"]')).toBeVisible();
  await expect(page.locator(".notice")).toContainText("Renamed other to Dance");

  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  await page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Other behavior"}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Duplicate behavior"}).click();
  await expect.poll(() => calls(page, "project_duplicate_behavior")).toEqual([{session: 7, behavior: "b2"}]);
  await expect(page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Other behavior copy"})).toBeVisible();
  await expect(page.getByRole("tab", {name: /^Other behavior copy/})).toBeVisible();
});

test("the Outline reorders behaviors by menu and by drag", async ({page}) => {
  await editorProject(page, true);
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  const outlineNames = () => page.locator(".panel-scroll > .outline-owner > summary").allTextContents();
  await page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Editor behavior"}).click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Move up"})).toBeDisabled();
  await menu(page).getByRole("menuitem", {name: "Move down"}).click();
  await expect.poll(() => calls(page, "project_reorder")).toEqual([{session: 7, behavior: "b1", target: "b2", after: true}]);
  await expect.poll(outlineNames).toEqual(["Other behavior", "Editor behavior"]);
  const editor = page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Editor behavior"});
  await drag(page, page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Other behavior"}), editor, {x: 20, y: 20});
  await expect.poll(async () => (await calls(page, "project_reorder")).at(-1)).toEqual({session: 7, behavior: "b2", target: "b1", after: true});
  await expect.poll(outlineNames).toEqual(["Editor behavior", "Other behavior"]);
});

test("any Project tree folder or file reorders by drag and by menu, and folders accept drops", async ({page}) => {
  await editorProject(page, true, [{path: "Assets/hello.wav", kind: "resource"}, {path: "Assets/wave.png", kind: "resource"}, {path: "notes.txt", kind: "resource"}]);
  expect(await treeOrder(page)).toEqual(["behavior", "other", "Assets"]);
  // A resource folder moves above a behavior folder (upper edge of the target).
  await drag(page, folder(page, "Assets"), folder(page, "behavior"), {x: 30, y: 2});
  await expect.poll(() => treeOrder(page)).toEqual(["Assets", "behavior", "other"]);
  // Menu moves work for every folder and file, including resources.
  await folder(page, "other").click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Move up"}).click();
  await expect.poll(() => treeOrder(page)).toEqual(["Assets", "other", "behavior"]);
  const wave = page.locator('button.tree-row[title="Assets/wave.png"]');
  await wave.click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Move down"})).toBeDisabled();
  await menu(page).getByRole("menuitem", {name: "Move up"}).click();
  await expect(folder(page, "Assets").locator("xpath=..").locator(".tree-row")).toHaveText(["wave.png", "hello.wav"]);
  // The order is project data for the .chorenproj file (one undo step each), not behavior order.
  expect(await calls(page, "project_reorder")).toEqual([]);
  const saved = await calls(page, "project_set_choren");
  expect(saved.at(-1)).toEqual({session: 7, key: "treeOrder", label: "Reorder project tree", value: {"": ["Assets", "other", "behavior", "notes.txt"], "Assets": ["wave.png", "hello.wav"]}});
  await expect(page.locator(".project-heading")).toContainText("Unsaved changes");
  // Middle of a folder: into it, and the moved folder keeps its place in the new name.
  const target = folder(page, "behavior");
  const box = (await target.boundingBox())!;
  await drag(page, folder(page, "other"), target, {x: 30, y: box.height / 2});
  await expect.poll(() => calls(page, "project_move_path")).toEqual([{session: 7, from: "other", to: "behavior/other"}]);
  await expect(page.locator('button.tree-row[title="behavior/other/behavior.xar"]')).toBeVisible();
  await expect(page.locator(".notice")).toContainText("Scripts that start a behavior by its path are not updated");
});

test("a project opens in the tree order saved in its .chorenproj", async ({page}) => {
  await editorProject(page, true, [{path: "Assets/hello.wav", kind: "resource"}], {treeOrder: {"": ["Assets", "other"]}});
  await expect.poll(() => treeOrder(page)).toEqual(["Assets", "other", "behavior"]);
});

test("the selected Project and Outline row is highlighted", async ({page}) => {
  await editorProject(page, true);
  const other = folder(page, "other");
  const file = page.locator('button.tree-row[title="behavior/behavior.xar"]');
  await other.click();
  await expect(other).toHaveAttribute("data-selected", "true");
  await file.click({button: "right"});
  await expect(file).toHaveAttribute("data-selected", "true");
  await expect(other).not.toHaveAttribute("data-selected");
  await expect(page.locator(".file-tree [data-selected]")).toHaveCount(1);
  expect(await file.evaluate(e => getComputedStyle(e).backgroundColor)).not.toBe("rgba(0, 0, 0, 0)");
  await page.keyboard.press("Escape");
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  const box = page.getByRole("button", {name: "Other script", exact: true});
  await box.click();
  await expect(box).toHaveAttribute("data-selected", "true");
  await expect(page.locator(".panel-scroll [data-selected]")).toHaveCount(1);
});

test("Outline boxes can be renamed", async ({page}) => {
  await editorProject(page, true);
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  await page.getByRole("button", {name: "Other script", exact: true}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Rename box…"}).click();
  const rename = page.getByRole("dialog", {name: "Rename box"});
  await expect(rename.getByRole("textbox", {name: "New name"})).toHaveValue("Other script");
  await rename.getByRole("textbox", {name: "New name"}).fill("Greeter");
  await rename.getByRole("button", {name: "Rename", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toEqual(expect.objectContaining({behavior: "b2", label: "Rename box", commands: [{type: "setAttributes", node: 20, values: {name: "Greeter"}}]}));
});
