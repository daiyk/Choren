import { expect, test } from "@playwright/test";
import {editorProject, calls} from "./editor-fixture";

test("the project renames from the Project panel and the File commands", async ({page}) => {
  await editorProject(page);
  const name = page.locator(".project-heading strong");
  await page.getByRole("toolbar", {name: "Project editing"}).getByRole("button", {name: "Rename"}).click();
  const rename = page.getByRole("dialog", {name: "Rename project"});
  await expect(rename.getByRole("textbox", {name: "New name"})).toHaveValue("Editor fixture");
  await expect(rename.getByRole("button", {name: "Rename", exact: true})).toBeDisabled();
  await expect(rename.locator(".import-hint")).toContainText("editor.pml");
  await rename.getByRole("textbox", {name: "New name"}).fill("  ");
  await expect(rename).toContainText("Enter a name.");
  await rename.getByRole("textbox", {name: "New name"}).fill("Show/Tell");
  await rename.getByRole("button", {name: "Rename", exact: true}).click();
  await expect.poll(() => calls(page, "project_rename")).toEqual([{session: 7, name: "Show/Tell"}]);
  await expect(rename).toHaveCount(0);
  await expect(name).toHaveText("Show/Tell");

  // The File menu and the command palette share the "rename-project" command.
  await page.getByRole("button", {name: /Search commands/}).click();
  await page.getByRole("textbox", {name: "Search commands"}).fill("Rename project");
  await page.getByRole("button", {name: "Rename project…", exact: false}).last().click();
  await expect(rename.getByRole("textbox", {name: "New name"})).toHaveValue("Show/Tell");
  await rename.getByRole("button", {name: "Cancel"}).click();

  await name.dblclick();
  await expect(rename).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator(".project-heading").click({button: "right"});
  await expect(page.getByRole("menuitem", {name: "Rename project…"})).toBeEnabled();
});
