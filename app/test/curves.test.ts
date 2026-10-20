import { describe, expect, it } from "vitest";
import { autoTangents, curvePath } from "../src/timeline/Curves";
import type { CurveKey } from "../src/format/types";

const key = (frame: number, value: number, extra: Partial<CurveKey> = {}): CurveKey => ({
  node: frame,
  frame,
  value,
  smooth: false,
  symmetrical: false,
  ...extra,
});
const tangent = (interp: string, abscissa = 0, ordinate = 0) => ({ node: 0, interp, abscissa, ordinate });

describe("motion curves", () => {
  it("automatic tangents match the backend rule (flat at extrema)", () => {
    expect(autoTangents([0, 0], [3, 10], [6, 0])).toEqual([[-1, -0], [1, 0]]);
    expect(autoTangents([0, 0], [3, 3], [6, 6])).toEqual([[-1, -1], [1, 1]]);
    expect(autoTangents(undefined, [0, 5], [3, 0])).toEqual([[0, 0], [1, 0]]);
  });
  it("draws Bezier, linear and constant segments", () => {
    expect(curvePath([])).toBe("");
    expect(curvePath([key(0, 0), key(3, 3)])).toBe("M0 0 C1 0 2 3 3 3");
    expect(curvePath([key(0, 0, { right: tangent("linear") }), key(3, 3)])).toBe("M0 0 L3 3");
    expect(curvePath([key(0, 0, { right: tangent("constant") }), key(3, 3)])).toBe("M0 0 H3 V3");
    expect(curvePath([key(0, 0, { right: tangent("bezier", 2, 1) }), key(3, 3, { left: tangent("bezier", -1, 0) })])).toBe("M0 0 C2 1 2 3 3 3");
  });
});

import { motionCode, motionData } from "../src/timeline/motionExport";
import type { Timeline } from "../src/format/types";
it("clamps automatic handles and exports ALMotion units and frame-one time", () => {
  const [left, right] = autoTangents([0, 0], [3, 9], [12, 10]);
  expect(left[1]).toBeCloseTo(-1 / 3);
  expect(right[1]).toBeCloseTo(1);
  expect(autoTangents(undefined, [6, 5], [9, 0])).toEqual([[-2, -0], [1, 0]]);
  const t: Timeline = {enabled: true, fps: 25, size: 26, layers: [], curves: [
    {actuator: "HeadYaw", unit: 0, frames: [1,26], keys: [key(1, 90), key(26, 180)]},
    {actuator: "LHand", unit: 1, frames: [1], keys: [key(1, 0.5)]},
    {actuator: "Muted", mute: true, frames: [1], keys: [key(1, 20)]},
  ]};
  const data = motionData(t, false);
  expect(data.map(c => c.name)).toEqual(["HeadYaw", "LHand"]);
  expect(data[0].times).toEqual([0, 1]);
  expect(data[0].keys).toEqual([Math.PI / 2, Math.PI]);
  expect(data[1].keys).toEqual([0.5]);
  expect(motionCode(t, true)).toContain("angleInterpolationBezier(names, times, keys)");
  expect(motionCode(t, false)).toContain("motion.angleInterpolation(names, keys, times, true)");
});

import { selectedFrameRanges } from "../src/timeline/curveMath";
it("mirrors selected adjacent keys as ranges without covering unselected keys", () => {
  const keys = [key(30, 1), key(10, 1), key(20, 1), key(40, 1)];
  expect(selectedFrameRanges(keys, [])).toEqual([]);
  expect(selectedFrameRanges(keys, [10, 20])).toEqual([[10, 20]]);
  expect(selectedFrameRanges(keys, [10, 30, 40])).toEqual([[10, 10], [30, 40]]);
  expect(keys.map(k => k.frame)).toEqual([30, 10, 20, 40]);
});

import { playbackFrame } from "../src/timeline/curveMath";
it("robot frames stop at the playback end, not the editor viewport size", () => {
  const t: Timeline = { enabled: true, fps: 25, size: 500, layers: [], curves: [
    { actuator: "HeadYaw", frames: [10, 20] }, { actuator: "HeadPitch", mute: true, frames: [100] },
  ] };
  expect(playbackFrame(t, 20)).toBe(20);
  expect(playbackFrame(t, 21)).toBeUndefined();
  expect(playbackFrame(t, -1)).toBeUndefined();
  expect(playbackFrame(t, null)).toBeUndefined();
  expect(playbackFrame({ ...t, endFrame: 30 }, 25)).toBe(25);
  expect(playbackFrame({ ...t, curves: [] }, 800)).toBe(800);
});
