import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { boxCategory, durationSummary } from "../src/diagram/appearance";
import { mainDiagram, parseXar } from "../src/format/xar";

const box = (attrs = "", content = "") => parseXar(`<ChoregrapheBox><Box id="1" name="Anything" ${attrs}>${content}</Box></ChoregrapheBox>`);

describe("box appearance", () => {
  it("derives structural categories and recognizes renamed stock timers", () => {
    expect(boxCategory(box("", '<Timeline enable="1"/>'))).toBe("timeline");
    expect(boxCategory(box("", '<Timeline enable="0"><BehaviorLayer><BehaviorKeyframe><Diagram/></BehaviorKeyframe></BehaviorLayer></Timeline>'))).toBe("subgraph");
    const timer = box("", '<bitmap>media/images/box/wait.png</bitmap><Parameter name="Timeout (s)" value="2.5"/>');
    expect(boxCategory({ ...timer, name: "Renamed" })).toBe("timer");
    expect(durationSummary(timer)).toBe("Duration: 2.5 s");
  });

  it("uses explicit roles without guessing from a name, an unconnected output, or a delay parameter", () => {
    expect(boxCategory(box('choren_category="terminal"'))).toBe("terminal");
    expect(boxCategory({ ...box(), name: "Ending" })).toBe("script");
    expect(boxCategory({ ...box(), name: "Wait" })).toBe("script");
    expect(boxCategory(box("", '<Parameter name="Delay (s)" value="5"/>'))).toBe("script");
    expect(boxCategory(box('choren_category="unknown"', '<Timeline enable="1"/>'))).toBe("timeline");
    expect(boxCategory(box('choren_category="constructor"'))).toBe("script");
  });

  it("bundles an explicit timer category and reads its actual duration", () => {
    const timer = parseXar(readFileSync(new URL('../../library/choren/Programming/Time/Wait/box.xar', import.meta.url), "utf8"));
    expect(boxCategory(timer)).toBe("timer");
    timer.params[0].value = "18";
    expect(durationSummary(timer)).toBe("Duration: 18 s");
  });
});


it("loads separate comment payloads, preserving lines and ignoring XML indentation", () => {
  const root = parseXar(readFileSync(new URL('./fixtures/comments.xar', import.meta.url), "utf8"));
  const [first, second, empty] = mainDiagram(mainDiagram(root)!.boxes[0])!.boxes;
  expect(first.pluginText).toBe("Introduction\nAsk the child to copy the gesture.");
  expect(first.script).toBe("generated_script = True");
  expect(second.pluginText).toContain("Keep <b>this</b> & every line as plain text.\nLine 3");
  expect(second.pluginText).toMatch(/The final instruction\.$/);
  expect(empty.pluginText).toBe("");
  expect(boxCategory(first)).toBe("comment");
  expect(root.pluginText).toBeUndefined();
  expect(box('plugin="textedit_plugin"', '<pluginContent><text>A &amp; B&#10;C</text></pluginContent>').pluginText).toBe("A & B\nC");
});


const gestureDemo = join(homedir(), "Downloads/Robot_activities_demos/Gesture Imitation English/behavior.xar");
it.skipIf(!existsSync(gestureDemo))("loads Step 1 and Step 1.1 from the local Gesture (Anger) v2 demo", () => {
  const root = parseXar(readFileSync(gestureDemo, "utf8"));
  const boxes: typeof root[] = [];
  const walk = (box: typeof root) => {
    boxes.push(box);
    for (const layer of box.timeline?.layers ?? [])
      for (const key of layer.keyframes)
        for (const child of key.diagram?.boxes ?? []) walk(child);
  };
  walk(root);
  const gesture = boxes.find((box) => box.name === "Gesture (Anger) v2")!;
  const comments = mainDiagram(gesture)!.boxes;
  expect(comments.find((box) => box.name === "Step 1")?.pluginText).toBe("Copying");
  expect(comments.find((box) => box.name === "Step 1.1")?.pluginText).toBe("Kid didn't copy –> Second Try");
});
