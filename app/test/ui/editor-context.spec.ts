import { expect, test } from "@playwright/test";
import {editorProject, calls, lastEdit, menu} from "./editor-fixture";

test("timeline menus bind the clicked keyframe and layer and preserve deletion guards", async ({page}) => {
  await editorProject(page);
  await page.getByRole("button", {name: "Maximize Timeline", exact: true}).click();
  const canvas = page.locator(".timeline-viewport canvas");
  await canvas.click({button: "right", position: {x: 144 + 19 * 8, y: 41}});
  await menu(page).getByRole("menuitem", {name: "Copy keyframe"}).click();
  await expect.poll(() => calls(page, "project_copy")).toEqual([{session: 7, behavior: "b1", boxes: [8]}]);
  await canvas.click({button: "right", position: {x: 40, y: 41}});
  await expect(menu(page).getByRole("menuitem", {name: "Paste keyframe at frame 1"})).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.getByRole("button", {name: "Timeline zoom in", exact: true}).click();
  await page.getByRole("slider", {name: "Timeline horizontal position"}).fill("60");
  await canvas.click({button: "right", position: {x: 144 + 19 * 10 - 60, y: 41}});
  await menu(page).getByRole("menuitem", {name: "Copy keyframe"}).click();
  await expect.poll(async () => (await calls(page, "project_copy")).at(-1)).toEqual({session: 7, behavior: "b1", boxes: [8]});
  await canvas.click({button: "right", position: {x: 40, y: 71}});
  await menu(page).getByRole("menuitemcheckbox", {name: "Mute layer"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b1", commands: [{type: "setAttributes", node: 16, values: {mute: "1"}}]});
  await page.getByRole("combobox", {name: "Timeline layer", exact: true}).selectOption("1");
  await page.getByRole("button", {name: "Timeline keyframe actions"}).focus();
  await page.keyboard.press("Shift+F10");
  await expect(menu(page).getByRole("menuitem", {name: "Delete keyframe"})).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", {name: "Timeline keyframe actions"})).toBeFocused();
});

test("curve and dope-sheet menus preserve key selection without starting a drag", async ({page}) => {
  await editorProject(page);
  await page.getByRole("button", {name: "Motion curves", exact: true}).click();
  await page.getByRole("button", {name: "Maximize Motion curves", exact: true}).click();
  await page.getByRole("button", {name: "Select all keys", exact: true}).click();
  const key = page.getByRole("button", {name: "Select key at frame 10", exact: true});
  await key.click({button: "right"});
  await expect(menu(page).getByRole("menuitem", {name: "Delete 2 keys"})).toBeVisible();
  await menu(page).getByRole("menuitem", {name: "Linear interpolation", exact: true}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b1", commands: [{type: "setInterpolation", keys: [43, 44], interp: "Linear"}]});
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  expect((await calls(page, "project_edit")).some((c: any) => c.label === "Move curve keys")).toBe(false);
  await page.getByRole("button", {name: "HeadYaw key at frame 20", exact: true}).focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Copy 2 keys"}).click();
  await expect.poll(() => calls(page, "project_copy")).toEqual([{session: 7, behavior: "b1", boxes: [43, 44]}]);
  await page.getByRole("button", {name: "Edit key 20 right tangent"}).click({button: "right"});
  await menu(page).getByRole("menuitem", {name: "Edit tangent…"}).click();
  await expect(page.getByRole("spinbutton", {name: "Key 20 right tangent frames"})).toBeFocused();
});

test("script actions use draft history and leave native text menus intact", async ({page}) => {
  await editorProject(page);
  await page.locator(".react-flow__node-box").dblclick();
  const script = page.getByLabel("Script box Python script", {exact: true});
  await expect(script).toBeVisible();
  await script.click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("\nother = 2");
  await script.click({button: "right"});
  await expect(menu(page)).toHaveCount(0);
  const actions = page.getByLabel("Script editor actions", {exact: true});
  await actions.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Undo typing"}).click();
  await expect(script).not.toContainText("other = 2");
  expect(await calls(page, "project_edit")).toEqual([]);
  await actions.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Redo typing"}).click();
  await expect(script).toContainText("other = 2");
  await actions.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Apply to box"}).click();
  await expect.poll(() => lastEdit(page)).toMatchObject({behavior: "b1", commands: [{type: "setScript", box: 20, text: "value = 1\nother = 2"}]});
});


test("motion import ignores a chooser result after its document changes", async ({page}) => {
  await editorProject(page);
  await page.getByRole("button", {name: "Motion curves", exact: true}).click();
  await page.getByRole("button", {name: "Maximize Motion curves", exact: true}).click();
  const actions = page.getByLabel("Motion editor actions", {exact: true});
  await actions.focus();
  await page.keyboard.press("Shift+F10");
  await menu(page).getByRole("menuitem", {name: "Import .qianim…"}).click();
  await expect.poll(() => page.evaluate(() => typeof (window as any).releaseAnimation)).toBe("function");
  await page.getByRole("combobox", {name: "Key 10 interpolation", exact: true}).selectOption("Linear");
  await expect(page.locator(".status-mode")).toHaveText("Unsaved changes");
  await page.evaluate(async () => { (window as any).releaseAnimation(); await new Promise(resolve => setTimeout(resolve, 0)); });
  expect(await calls(page, "project_import_qianim")).toEqual([]);
});
