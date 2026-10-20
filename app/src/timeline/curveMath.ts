import type { CurveKey, Timeline } from "../format/types";

type Point = [number, number];

/** Automatic Bezier handles; mirrors crates/choren-core/src/motion.rs `auto_tangents`. */
export function autoTangents(
  prev: Point | undefined,
  cur: Point,
  next: Point | undefined,
): [Point, Point] {
  let slope = prev && next && (cur[1] - prev[1]) * (next[1] - cur[1]) > 0
    ? (next[1] - prev[1]) / (next[0] - prev[0]) : 0;
  if (prev && next && slope) {
    slope *= Math.min(1, Math.abs((next[1] - cur[1]) / ((next[0] - cur[0]) / 3 * slope)));
    slope *= Math.min(1, Math.abs((cur[1] - prev[1]) / ((cur[0] - prev[0]) / 3 * slope)));
  }
  const left = -(cur[0] - (prev?.[0] ?? 0)) / 3;
  const right = next ? (next[0] - cur[0]) / 3 : 0;
  return [[left || 0, left * slope || (left < 0 ? -0 : 0)], [right, right * slope]];
}

/** SVG path of a curve in (frame, value) space: Bezier, linear or constant segments. */
export function curvePath(keys: CurveKey[]) {
  if (!keys.length) return "";
  const pt = (k: CurveKey): Point => [k.frame, k.value];
  const handles = keys.map((k, i) =>
    autoTangents(
      keys[i - 1] && pt(keys[i - 1]),
      pt(k),
      keys[i + 1] && pt(keys[i + 1]),
    ),
  );
  let d = `M${keys[0].frame} ${keys[0].value}`;
  for (let i = 1; i < keys.length; i++) {
    const [a, b] = [keys[i - 1], keys[i]];
    // Playback uses the same precedence (curve_keys in crates/choren-naoqi/src/runtime.rs).
    const out = a.right?.interp ?? "auto";
    if (out === "constant") d += ` H${b.frame} V${b.value}`;
    else if (out === "linear" || b.left?.interp === "linear")
      d += ` L${b.frame} ${b.value}`;
    else {
      const r: Point = a.right
        ? [a.right.abscissa, a.right.ordinate]
        : handles[i - 1][1];
      const l: Point = b.left
        ? [b.left.abscissa, b.left.ordinate]
        : handles[i][0];
      d += ` C${a.frame + r[0]} ${a.value + r[1]} ${b.frame + l[0]} ${b.value + l[1]} ${b.frame} ${b.value}`;
    }
  }
  return d;
}

/** Adjacent selected keys form a range, even when their frame numbers have gaps. */
export function selectedFrameRanges(keys: { node: number; frame: number }[], selected: number[]): [number, number][] {
  const ranges: [number, number][] = [];
  const chosen = new Set(selected);
  let range: [number, number] | undefined;
  for (const key of [...keys].sort((a, b) => a.frame - b.frame)) {
    if (!chosen.has(key.node)) { range = undefined; continue; }
    if (range) range[1] = key.frame;
    else { range = [key.frame, key.frame]; ranges.push(range); }
  }
  return ranges;
}

/** NAOqi ends at end_frame or the last unmuted motion key; layer-only timelines keep playing. */
export function playbackFrame(t: Timeline, frame: number | null | undefined): number | undefined {
  const end = (t.endFrame ?? -1) >= 0 ? t.endFrame! : Math.max(0, ...t.curves.filter(c => !c.mute).flatMap(c => c.frames));
  return typeof frame === "number" && frame >= 1 && (end === 0 || frame <= end) ? frame : undefined;
}
