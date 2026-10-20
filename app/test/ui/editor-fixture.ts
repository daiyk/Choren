import { expect, type Page } from "@playwright/test";

export async function editorProject(page: Page, extraBehavior = false) {
  await page.addInitScript(({extraBehavior}) => {
    let rid = 0, revision = 0;
    const calls: {command: string; args: any}[] = [];
    const box = (node: number, id: number, name: string, script = "") => ({node, id, name, script, x: 180, y: 120, tooltip: "", bitmaps: [], plugin: "", inputs: [], outputs: [], params: [], resources: []});
    const script = box(20, 1, "Script box", "value = 1");
    const diagram = (node: number, boxes: any[] = []) => ({node, scale: 100, boxes, links: []});
    const doc = {session: 7, id: "/p/editor.pml", source: "/p/editor.pml", name: "Editor fixture", format: "pml", metadata: {}, diagnostics: [], files: [{path: "behavior/behavior.xar", kind: "behavior", behaviorId: "b1"}], behaviors: [{id: "b1", name: "Editor behavior", source: "behavior/behavior.xar", root: {...box(1, -1, "root"), timeline: {
      node: 5, enabled: true, fps: 25, size: 500,
      layers: [{node: 6, name: "First layer", keyframes: [{node: 7, name: "Start", index: 1, diagram: diagram(10, [script])}, {node: 8, name: "Second", index: 20, diagram: diagram(11)}]}, {node: 16, name: "Other layer", keyframes: [{node: 17, name: "Only", index: 1, diagram: diagram(18)}]}],
      curves: [{node: 42, actuator: "HeadYaw", frames: [10, 20], values: [5, -5], unit: 0, mute: false, keys: [{node: 43, frame: 10, value: 5, smooth: false, symmetrical: false}, {node: 44, frame: 20, value: -5, smooth: false, symmetrical: false, left: {node: 45, interp: "bezier", abscissa: -3, ordinate: 0}, right: {node: 46, interp: "bezier", abscissa: 3, ordinate: 0}}]}],
    }}}]};
    if (extraBehavior) {
      const other = structuredClone(doc.behaviors[0]);
      other.id = "b2"; other.name = "Other behavior"; other.source = "other/behavior.xar";
      other.root.timeline.layers[0].keyframes[0].diagram.boxes[0].name = "Other script";
      other.root.timeline.layers[0].keyframes[0].diagram.boxes.push({...box(30, 2, "Nested container"), timeline: {node: 31, enabled: false, fps: 25, size: 0, curves: [], layers: [{node: 32, name: "Flow", keyframes: [{node: 33, name: "Start", index: 1, diagram: diagram(88, [box(120, 1, "Deep other script", "nested = 1")])}]}]}});
      doc.behaviors.push(other);
      doc.files.push({path: "other/behavior.xar", kind: "behavior", behaviorId: "b2"});
    }
    Object.assign(window, {calls, isTauri: true, __TAURI_INTERNALS__: {
      transformCallback: () => ++rid,
      invoke: async (command: string, args: any) => {
        calls.push({command, args});
        if (command === "plugin:menu|new") return [++rid, String(rid)];
        if (command === "plugin:dialog|open") {
          if (args.options?.filters?.some((f: any) => f.extensions.includes("qianim"))) return new Promise(resolve => { (window as any).releaseAnimation = () => resolve("/p/in.qianim"); });
          return "/p/editor.pml";
        }
        if (command === "plugin:dialog|save") return "/p/export.crg";
        if (command.startsWith("plugin:") || command === "authorize_project_root" || command === "project_close") return;
        if (command === "library_index" || command === "library_tree") return [];
        if (command === "robot_status") return null;
        if (command === "project_open") return structuredClone(doc);
        if (command === "project_manifest") return {uuid: "editor", version: "1", names: [], supportedLanguages: [], behaviors: ["behavior", "other"].map(path => ({path, nature: "interactive", userRequestable: false, names: [], descriptions: [], tags: [], triggerSentences: [], loadingResponses: [], conditions: [], permissions: []}))};
        if (command === "project_export_crg") return;
        if (command === "project_remove_behavior") {
          doc.behaviors = doc.behaviors.filter(b => b.id !== args.behavior);
          doc.files = doc.files.filter(f => f.behaviorId !== args.behavior);
          return {document: structuredClone(doc), history: {revision: ++revision, dirty: true, undo: "Remove behavior", created: []}};
        }
        if (command === "project_copy") {
          if ((window as any).holdCopy) await new Promise<void>(resolve => { (window as any).releaseCopy = resolve; });
          return '<Key frame="10" value="5"/>';
        }
        if (command === "project_edit") {
          for (const c of args.commands) if (c.type === "setScript") script.script = c.text;
          return {document: structuredClone(doc), history: {revision: ++revision, dirty: true, undo: args.label, created: []}};
        }
        throw new Error(`Unexpected IPC: ${command}`);
      },
    }});
  }, {extraBehavior});
  await page.goto("/");
  await page.getByRole("button", {name: "Open project", exact: true}).first().click();
  await expect(page.getByRole("button", {name: "Timeline keyframe actions"})).toBeVisible();
}
export const calls = (page: Page, command: string) => page.evaluate(c => (window as any).calls.filter((x: any) => x.command === c).map((x: any) => x.args), command);
export const lastEdit = async (page: Page) => (await calls(page, "project_edit")).at(-1);
export const menu = (page: Page) => page.getByRole("menu", {name: "Context actions"});
