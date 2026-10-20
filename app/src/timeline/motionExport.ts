import type { Timeline } from "../format/types";
import { autoTangents } from "./curveMath";

/** File units → ALMotion values; the first timeline frame is time zero. */
export function motionData(t: Timeline, bezier: boolean) {
  const curves = t.curves.filter(c => !c.mute && c.keys?.length).slice().sort((a, b) => a.actuator.localeCompare(b.actuator));
  return curves.map(c => {
    const convert = (value: number) => c.unit === 1 ? value : value * Math.PI / 180;
    const keys = c.keys!;
    return {
      name: c.actuator,
      times: keys.map(k => (k.frame - 1) / t.fps),
      keys: keys.map((k, i) => {
        const value = convert(k.value);
        if (!bezier) return value;
        const point = (index: number): [number, number] | undefined => keys[index] && [keys[index].frame, keys[index].value];
        const automatic = autoTangents(point(i - 1), point(i)!, point(i + 1));
        const handles = (["left", "right"] as const).map((side, index) => {
          const tangent = k[side];
          const type = tangent ? ({constant: 0, linear: 1, bezier: 2}[tangent.interp] ?? 1) : 3;
          return [type, (tangent?.abscissa ?? automatic[index][0]) / t.fps, convert(tangent?.ordinate ?? automatic[index][1])];
        });
        return [value, ...handles];
      }),
    };
  });
}

export function motionCode(t: Timeline, bezier: boolean) {
  const data = motionData(t, bezier);
  const lines = ["// Choren motion export. Configure the robot address before running.", "#include <alproxies/almotionproxy.h>", "#include <alvalue/alvalue.h>", "#include <string>", "#include <vector>", "", "int main() {", "  std::vector<std::string> names;", "  AL::ALValue times, keys;", `  times.arraySetSize(${data.length});`, `  keys.arraySetSize(${data.length});`];
  const assign = (target: string, value: unknown) => {
    if (Array.isArray(value)) {lines.push(`  ${target}.arraySetSize(${value.length});`); value.forEach((v, i) => assign(`${target}[${i}]`, v));}
    else lines.push(`  ${target} = ${JSON.stringify(value)};`);
  };
  data.forEach((c, i) => {lines.push(`  names.push_back(${JSON.stringify(c.name)});`); assign(`times[${i}]`, c.times); assign(`keys[${i}]`, c.keys);});
  lines.push('  AL::ALMotionProxy motion("127.0.0.1", 9559);', bezier ? "  motion.angleInterpolationBezier(names, times, keys);" : "  motion.angleInterpolation(names, keys, times, true);", "  return 0;", "}", "");
  return lines.join("\n");
}
