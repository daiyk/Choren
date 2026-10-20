import { Code2, Film, Flag, StickyNote, Timer, Workflow } from "lucide-react";
import type { CSSProperties } from "react";
import type { Box } from "../format/types";

export const categories = {
  comment: { label: "Comment / Text", Icon: StickyNote },
  subgraph: { label: "Subgraph", Icon: Workflow },
  timeline: { label: "Timeline / Animation", Icon: Film },
  timer: { label: "Timer", Icon: Timer },
  terminal: { label: "Terminal", Icon: Flag },
  script: { label: "Script / Custom", Icon: Code2 },
};
export type BoxCategory = keyof typeof categories;
export const isBoxCategory = (value: string | undefined): value is BoxCategory =>
  !!value && Object.hasOwn(categories, value);

export function boxCategory(box: Box): BoxCategory {
  if (isBoxCategory(box.category)) return box.category;
  if (box.plugin === "textedit_plugin") return "comment";
  if (box.timeline?.enabled) return "timeline";
  if (box.timeline?.layers.some((layer) => layer.keyframes.some((key) => key.diagram)))
    return "subgraph";
  // The stock asset identifies imported Wait boxes even after the user renames them.
  if (box.bitmaps.includes("media/images/box/wait.png")) return "timer";
  return "script";
}

export const categoryColor = (category: BoxCategory) => `var(--category-${category})`;
export const categoryStyle = (category: BoxCategory) => ({
  "--box-color": categoryColor(category),
  "--box-ink": `var(--category-${category}-ink)`,
  "--box-label": `var(--category-${category}-label)`,
}) as CSSProperties;

export function CategoryBadge({ box }: { box: Box }) {
  const category = boxCategory(box);
  const { label, Icon } = categories[category];
  return <span className="category-badge" data-category={category} style={categoryStyle(category)}>
    <Icon size={13} aria-hidden="true" />{label}
  </span>;
}

export function durationSummary(box: Box) {
  if (boxCategory(box) !== "timer") return undefined;
  const duration = box.params.find((param) =>
    ["Delay (s)", "Timeout (s)", "Delay (seconds)"].includes(param.name),
  );
  return duration ? `Duration: ${duration.value} s` : undefined;
}
