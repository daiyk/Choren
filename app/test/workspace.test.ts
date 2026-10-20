import { describe, expect, it } from "vitest";
import { parseXar } from "../src/format/xar";
import { openProject } from "../src/format/project";
import { relativePath } from "../src/format/xml";
import { exampleProject } from "../src/workspace/example";
import {
  graphId,
  initialWorkspace,
  OpenRequests,
  outline,
  resolveAddress,
  workspaceReducer,
} from "../src/workspace/model";
import { validLayout } from "../src/workspace/storage";

const bytes = (s: string) => new TextEncoder().encode(s);
const xar =
  '<ChoregrapheProject><Box id="-1" name="root"/></ChoregrapheProject>';
const reader = (files: Record<string, string>) => async (path: string) => {
  if (!(path in files)) throw new Error(`Missing ${path}`);
  return bytes(files[path]);
};

describe("project import", () => {
  it("decodes XML references once while preserving CDATA, whitespace and script literals", () => {
    const box = parseXar(
      `<ChoregrapheProject><Box id="-1" name="A &amp; B" tooltip="First&#x0A;Second&#10;Third &amp;#10; &lt;text&gt;"><script><content><![CDATA[\n  value = "&#10; <tag> &amp;"\n]]></content></script></Box></ChoregrapheProject>`,
    );
    expect(box.name).toBe("A & B");
    expect(box.tooltip).toBe("First\nSecond\nThird &#10; <text>");
    expect(box.script).toBe('\n  value = "&#10; <tag> &amp;"\n');
  });
  it("returns metadata and file references, including missing resources on Windows-style paths", async () => {
    const read = reader({
      "D:\\Projects\\中文 Demo\\demo.pml":
        '<Package name="Demo" format_version="5"><Manifest src="manifest.xml"/><BehaviorDescriptions><BehaviorDescription src="Hello" xar="behavior.xar"/></BehaviorDescriptions><Resources><File src="missing.wav"/></Resources></Package>',
      "D:/Projects/中文 Demo/Hello/behavior.xar": xar,
      "D:/Projects/中文 Demo/manifest.xml":
        '<package uuid="demo" version="1.0"><names><name lang="en_US">Demo title</name></names></package>',
    });
    const project = await openProject(
      "D:\\Projects\\中文 Demo\\demo.pml",
      read,
      async () => false,
    );
    expect(project.behaviors[0]).toMatchObject({
      name: "Hello",
      source: "Hello/behavior.xar",
    });
    expect(project.metadata).toMatchObject({
      Title: "Demo title",
      Version: "1.0",
    });
    expect(project.files.find((f) => f.path === "missing.wav")?.missing).toBe(
      true,
    );
    expect(project.diagnostics).toHaveLength(1);
  });
  it("accepts an empty project without dereferencing a behavior", async () => {
    const project = await openProject(
      "/empty.pml",
      reader({
        "/empty.pml": '<Package name="Empty"><BehaviorDescriptions/></Package>',
      }),
    );
    const state = workspaceReducer(initialWorkspace, { type: "load", project });
    expect(state.project?.name).toBe("Empty");
    expect(state.activeBehavior).toBeUndefined();
  });
  it("does not accept malformed projects or references outside the selected root", async () => {
    await expect(
      openProject("/bad.pml", reader({ "/bad.pml": "<NotAPackage/>" })),
    ).rejects.toThrow("no Package");
    for (const path of [
      "../secret",
      "/etc/passwd",
      "C:\\outside",
      "folder/../../outside",
    ])
      expect(() => relativePath(path)).toThrow();
    expect(relativePath("./resources\\image.png")).toBe("resources/image.png");
  });
});

describe("workspace identity and transitions", () => {
  it("keeps viewports independent for repeated local box IDs and clears stale selection on navigation", () => {
    const project = exampleProject();
    let state = workspaceReducer(initialWorkspace, { type: "load", project });
    const root = state.addresses.welcome;
    const rootKey = graphId(project.id, root);
    state = workspaceReducer(state, {
      type: "select",
      key: rootKey,
      selection: { nodes: ["1"], edges: [] },
    });
    state = workspaceReducer(state, {
      type: "viewport",
      key: rootKey,
      viewport: { x: 80, y: 90, zoom: 1.4 },
    });
    const nested = {
      ...root,
      levels: [...root.levels, { boxId: 2, layer: 0, keyframe: 0 }],
    };
    const nestedKey = graphId(project.id, nested);
    state = workspaceReducer(state, { type: "navigate", address: nested });
    expect(nestedKey).not.toBe(rootKey);
    expect(state.selections[nestedKey]).toEqual({ nodes: [], edges: [] });
    expect(resolveAddress(project, nested)?.diagram?.boxes[0].name).toBe(
      "Nested step",
    );
    state = workspaceReducer(state, { type: "navigate", address: root });
    expect(state.viewports[rootKey]).toEqual({ x: 80, y: 90, zoom: 1.4 });
    expect(state.selections[rootKey].nodes).toEqual([]);
  });
  it("reveals nested search results at their containing diagram", () => {
    const project = exampleProject();
    const nested = outline(project).find(
      (entry) => entry.box.name === "Nested step",
    )!;
    expect(nested.address.levels).toHaveLength(2);
    expect(resolveAddress(project, nested.address)?.owner.name).toBe(
      "Sequence",
    );
    const finish = outline(project).find(
      (entry) => entry.box.name === "Finish pose",
    )!;
    expect(finish.address.levels.at(-1)?.keyframe).toBe(1);
  });
  it("ignores invalid stale addresses and distinguishes timeline keyframes", () => {
    const project = exampleProject();
    const state = workspaceReducer(initialWorkspace, { type: "load", project });
    const address = {
      ...state.addresses.welcome,
      levels: [
        { boxId: -1, layer: 0, keyframe: 0 },
        { boxId: 999, layer: 0, keyframe: 0 },
      ],
    };
    expect(workspaceReducer(state, { type: "navigate", address })).toBe(state);
    const a = {
      ...state.addresses.welcome,
      levels: [
        { boxId: -1, layer: 0, keyframe: 0 },
        { boxId: 3, layer: 0, keyframe: 0 },
      ],
    };
    const b = {
      ...a,
      levels: [...a.levels.slice(0, -1), { ...a.levels.at(-1)!, keyframe: 1 }],
    };
    expect(graphId(project.id, a)).not.toBe(graphId(project.id, b));
  });
  it("invalidates an older request when a newer open or cancellation occurs", async () => {
    const requests = new OpenRequests();
    const old = requests.begin();
    const latest = requests.begin();
    expect(requests.current(old)).toBe(false);
    expect(requests.current(latest)).toBe(true);
    requests.begin();
    expect(requests.current(latest)).toBe(false);
  });
});

describe("saved layouts", () => {
  it("rejects invalid versions, unknown renderers and external window layouts", () => {
    expect(validLayout(null)).toBe(false);
    expect(validLayout({ version: 2, layout: {} })).toBe(false);
    expect(
      validLayout({
        version: 1,
        layout: { grid: {}, panels: { x: { contentComponent: "unknown" } } },
      }),
    ).toBe(false);
    expect(
      validLayout({
        version: 1,
        layout: { grid: {}, panels: {}, popoutGroups: [{}] },
      }),
    ).toBe(false);
    expect(
      validLayout({
        version: 1,
        layout: { grid: {}, panels: { x: { contentComponent: "workspace" } } },
      }),
    ).toBe(true);
  });
});
