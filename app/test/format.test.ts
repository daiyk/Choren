import { describe, expect, it } from "vitest";
import { existsSync, globSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { openProject } from "../src/format/project";
import { mainDiagram, resolveLink } from "../src/format/xar";
import { behaviorRelative } from "../src/format/xml";
import type { Box } from "../src/format/types";
import { buildLink, classicBoxSize, classicInputSlot, classicOutputSlot, inputAnchor, outputAnchor } from "../src/diagram/geometry";

// Corpora are not copied into the repo (SoftBank content); point these at local copies.
const SHARE =
  process.env.CHOREGRAPHE_SHARE ??
  resolve(__dirname, "../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe");
const REF =
  process.env.REF_PROJECT ??
  join(homedir(), "Downloads/German_picture_description_v2_300926/German_picture_description_v2_300926.pml");

const read = async (p: string) => new Uint8Array(readFileSync(p));

/** Walks every diagram under a box, counting boxes, links and links whose ends do not resolve. */
function stats(root: Box) {
  const s = { boxes: 1, links: 0, unresolved: 0 };
  const walk = (b: Box) => {
    for (const layer of b.timeline?.layers ?? [])
      for (const kf of layer.keyframes) {
        if (!kf.diagram) continue;
        s.links += kf.diagram.links.length;
        s.unresolved += kf.diagram.links.filter((l) => !resolveLink(l, kf.diagram!, b)).length;
        for (const child of kf.diagram.boxes) {
          s.boxes++;
          walk(child);
        }
      }
  };
  walk(root);
  return s;
}

describe.skipIf(!existsSync(SHARE))("shipped Choregraphe corpus", () => {
  it("parses all 358 .xar files with every link resolved", async () => {
    const files = globSync("**/*.xar", { cwd: SHARE });
    expect(files.length).toBe(358);
    let unresolved = 0;
    for (const f of files) {
      const { behaviors: [b] } = await openProject(join(SHARE, f), read);
      unresolved += stats(b.root).unresolved;
    }
    expect(unresolved).toBe(0);
  });

  it("opens all 5 .crg archives", async () => {
    const files = globSync("**/*.crg", { cwd: SHARE });
    expect(files.length).toBe(5);
    for (const f of files) {
      const { behaviors } = await openProject(join(SHARE, f), read);
      expect(behaviors.length).toBe(1);
      expect(behaviors[0].root.name).toBe("root");
    }
  });

  it("reads the Happy timeline (53 frames @25 fps with motion curves)", async () => {
    const { behaviors: [b] } = await openProject(join(SHARE, "libraries/box/Animation/Moods/Positive/NAO/Happy/box.xar"), read);
    const t = b.root.timeline!;
    expect([t.enabled, t.fps, t.size]).toEqual([true, 25, 53]);
    expect(t.curves.length).toBeGreaterThan(0);
  });
});

describe.skipIf(!existsSync(REF))("reference project", () => {
  it("loads 2 behaviors, 173 boxes, 151 links, 0 unresolved", async () => {
    const { behaviors } = await openProject(REF, read);
    expect(behaviors.map((b) => b.name).sort()).toEqual(["Picture Description", "behavior_1"]);
    const total = behaviors.map((b) => stats(b.root)).reduce((a, s) => ({
      boxes: a.boxes + s.boxes,
      links: a.links + s.links,
      unresolved: a.unresolved + s.unresolved,
    }));
    expect(total).toEqual({ boxes: 173, links: 151, unresolved: 0 });
    const root = behaviors.find((b) => b.name === "behavior_1")!.root;
    expect(mainDiagram(root)!.boxes.find((b) => b.name === "Wait")).toMatchObject({ x: 99, y: 78 });
  });
});

describe("behaviorRelative", () => {
  it("names a project file from the behavior folder as Choregraphe's topic picker does", () => {
    expect(behaviorRelative("behavior_1/behavior.xar", "ExampleDialog/ExampleDialog.dlg")).toBe("../ExampleDialog/ExampleDialog.dlg");
    expect(behaviorRelative("behavior_1/behavior.xar", "behavior_1/talk/talk.dlg")).toBe("talk/talk.dlg");
    expect(behaviorRelative("behavior.xar", "talk/talk.dlg")).toBe("talk/talk.dlg");
  });
});

describe("geometry", () => {
  it("sizes classic boxes to their ports and anchors links at the port tabs", () => {
    expect(classicBoxSize(2, 1)).toEqual({ w: 90, h: 96 });
    expect(classicBoxSize(6, 1)).toEqual({ w: 90, h: 150 });
    expect(classicBoxSize(1, 7)).toEqual({ w: 90, h: 154 });
    expect(inputAnchor(classicInputSlot(0))).toEqual({ x: -5, y: 34 });
    expect(outputAnchor(classicOutputSlot(90, 0))).toEqual({ x: 111, y: 12 });
  });
  it("draws links as one cubic whose handles leave each port horizontally", () => {
    expect(buildLink({ x: 0, y: 0 }, { x: 100, y: 50 })).toBe("M100,50 C50,50 50,0 0,0");
    expect(buildLink({ x: 100, y: 0 }, { x: 0, y: 40 })).toBe("M0,40 C-50,40 150,0 100,0");
    expect(buildLink({ x: 0, y: 0 }, { x: 20, y: 0 })).toBe("M20,0 C-20,0 40,0 0,0");
  });

});
