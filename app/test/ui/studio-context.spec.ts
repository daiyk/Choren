import { expect, test } from "@playwright/test";
import {editorProject, calls, lastEdit, menu} from "./editor-fixture";

test("Project actions export and remove the clicked inactive behavior", async ({page}) => {
  await editorProject(page, true);
  const otherFile = page.locator('button.tree-row[title="other/behavior.xar"]');
  await otherFile.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Export behavior…"}).click();
  await expect.poll(() => calls(page, "project_export_crg")).toEqual([{session: 7, behavior: "b2", path: "/p/export.crg"}]);
  await expect(page.getByRole("navigation", {name: "Diagram path"})).toContainText("Editor behavior");
  page.once("dialog", dialog => dialog.accept());
  await otherFile.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Remove behavior from project…"}).click();
  await expect.poll(() => calls(page, "project_remove_behavior")).toEqual([{session: 7, behavior: "b2"}]);
  await expect(otherFile).toHaveCount(0);
  await expect(page.getByRole("navigation", {name: "Diagram path"})).toContainText("Editor behavior");
});

test("Outline search actions target inactive boxes and grouping retains its captured target", async ({page}) => {
  await editorProject(page, true);
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  await page.getByRole("textbox", {name: "Find behaviors and boxes"}).fill("Other script");
  const result = page.getByRole("button", {name: /^Other script Other behavior/});
  await result.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Copy", exact: true}).click();
  await expect.poll(() => calls(page, "project_copy")).toEqual([{session: 7, behavior: "b2", boxes: [20]}]);
  await result.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Group into box…"}).click();
  await page.getByRole("textbox", {name: "Name of the new box"}).fill("Wrapped");
  await page.getByRole("button", {name: "Group", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b2", commands: [{type: "convertToBox", diagram: 10, boxes: [20], name: "Wrapped"}]});
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await result.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Delete box", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b2", commands: [{type: "remove", nodes: [20]}]});
});

test("an Outline cut waiting for copied content is canceled after the document changes", async ({page}) => {
  await editorProject(page, true);
  await page.evaluate(() => { (window as any).holdCopy = true; });
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  await page.getByRole("textbox", {name: "Find behaviors and boxes"}).fill("Other script");
  await page.getByRole("button", {name: /^Other script Other behavior/}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Cut", exact: true}).click();
  await expect.poll(() => calls(page, "project_copy")).toHaveLength(1);
  await page.getByRole("toolbar", {name: "Timeline editing"}).getByRole("button", {name: "Layer", exact: true}).click();
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page.evaluate(async () => { (window as any).releaseCopy(); await new Promise(resolve => setTimeout(resolve, 0)); });
  expect(await calls(page, "project_edit")).toHaveLength(1);
  expect((await lastEdit(page)).commands[0].type).toBe("insertXml");
});


test("Project background stays isolated and inactive behavior properties open its section", async ({page}) => {
  await editorProject(page, true);
  const file = page.locator('button.tree-row[title="other/behavior.xar"]');
  await file.click({button: "right"});
  await page.keyboard.press("Escape");
  await page.locator(".project-heading").click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "New behavior…"})).toBeVisible();
  await expect(menu(page).getByRole("menuitem", {name: "Remove behavior from project…"})).toHaveCount(0);
  await page.keyboard.press("Escape");
  await file.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Behavior properties…"}).click();
  const modal = page.getByRole("dialog", {name: "Project properties"});
  await expect(modal.locator('[data-path="other"]')).toHaveAttribute("open", "");
  await expect(modal.locator('[data-path="behavior"]')).not.toHaveAttribute("open", "");
  await expect(modal.locator('[data-path="other"] input').first()).toBeFocused();
});

test("nested Outline boxes with reused local IDs keep their parent diagram and behavior", async ({page}) => {
  await editorProject(page, true);
  await page.getByRole("tab", {name: "Outline", exact: true}).click();
  await page.getByRole("textbox", {name: "Find behaviors and boxes"}).fill("Deep other script");
  await page.locator(".search-result", {hasText: "Deep other script"}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Group into box…"}).click();
  await page.getByRole("textbox", {name: "Name of the new box"}).fill("Nested group");
  await page.getByRole("button", {name: "Group", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b2", commands: [{type: "convertToBox", diagram: 88, boxes: [120], name: "Nested group"}]});
});

test("inactive document tab actions retain the clicked panel", async ({page}) => {
  await editorProject(page, true);
  await page.locator('button.tree-row[title="other/behavior.xar"]').click();
  await page.getByRole("tab", {name: "Editor behavior", exact: true}).click();
  const other = page.getByRole("tab", {name: "Other behavior", exact: true});
  await other.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Move or resize…"}).click();
  await expect(page.getByRole("dialog", {name: "Panel: Other behavior"})).toBeVisible();
  await page.getByRole("button", {name: "Close dialog"}).click();
  await page.getByRole("tab", {name: "Editor behavior", exact: true}).click();
  await other.click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Close panel"}).click();
  await expect(other).toHaveCount(0);
  await expect(page.getByRole("tab", {name: "Editor behavior", exact: true})).toBeVisible();
});
