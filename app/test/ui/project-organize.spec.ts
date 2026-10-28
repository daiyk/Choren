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

test("behaviors and folders reorder by menu and by drag, and folders accept dropped behaviors", async ({page}) => {
  await editorProject(page, true);
  expect(await treeOrder(page)).toEqual(["behavior", "other"]);
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  const outlineNames = () => page.locator(".panel-scroll > .outline-owner > summary").allTextContents();
  await page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Editor behavior"}).click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Move up"})).toBeDisabled();
  await menu(page).getByRole("menuitem", {name: "Move down"}).click();
  await expect.poll(() => calls(page, "project_reorder")).toEqual([{session: 7, path: null, behavior: "b1", target: "b2", after: true}]);
  await expect.poll(outlineNames).toEqual(["Other behavior", "Editor behavior"]);
  // Drag the first Outline behavior below the second.
  const editor = page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Editor behavior"});
  await drag(page, page.locator(".panel-scroll > .outline-owner > summary", {hasText: "Other behavior"}), editor, {x: 20, y: 20});
  await expect.poll(async () => (await calls(page, "project_reorder")).at(-1)).toEqual({session: 7, path: null, behavior: "b2", target: "b1", after: true});
  await expect.poll(outlineNames).toEqual(["Editor behavior", "Other behavior"]);

  await page.getByRole("tab", {name: "Project", exact: true}).click();
  // Upper edge of a folder: before it.
  await drag(page, folder(page, "other"), folder(page, "behavior"), {x: 30, y: 2});
  await expect.poll(async () => (await calls(page, "project_reorder")).at(-1)).toEqual({session: 7, path: "other", behavior: null, target: "behavior", after: false});
  await expect.poll(() => treeOrder(page)).toEqual(["other", "behavior"]);
  // Middle of a folder: into it.
  const target = folder(page, "behavior");
  const box = (await target.boundingBox())!;
  await drag(page, folder(page, "other"), target, {x: 30, y: box.height / 2});
  await expect.poll(() => calls(page, "project_move_path")).toEqual([{session: 7, from: "other", to: "behavior/other"}]);
  await expect(page.locator('button.tree-row[title="behavior/other/behavior.xar"]')).toBeVisible();
  await expect(page.locator(".notice")).toContainText("Scripts that start a behavior by its path are not updated");
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
