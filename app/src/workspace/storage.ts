export type ColorMode = "system" | "light" | "dark";
export type GraphStyle = "choren" | "classic";
export const LAYOUT_KEY = "choren.layout.v1";
export const THEME_KEY = "choren.theme";
export const GRAPH_STYLE_KEY = "choren.graph-style";
export const GRAPH_SCROLLING_KEY = "choren.graph-scrolling";
export const TEXT_SCALE_KEY = "choren.text-scale";
/** "done" once the first-run welcome window was closed. */
export const WELCOME_KEY = "choren.welcome";
export function readPreference(key: string): unknown {
  try {
    return JSON.parse(localStorage.getItem(key) ?? "null");
  } catch {
    return null;
  }
}
export function savePreference(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* A full storage quota must not break the current session. */
  }
}
export function validLayout(
  value: unknown,
): value is {
  version: 1;
  layout: import("dockview-react").SerializedDockview;
} {
  if (!value || typeof value !== "object") return false;
  const record = value as {
    version?: unknown;
    layout?: {
      grid?: unknown;
      panels?: unknown;
      floatingGroups?: unknown[];
      popoutGroups?: unknown[];
    };
  };
  if (
    record.version !== 1 ||
    !record.layout?.grid ||
    !record.layout.panels ||
    typeof record.layout.panels !== "object"
  )
    return false;
  if (
    record.layout.floatingGroups?.length ||
    record.layout.popoutGroups?.length
  )
    return false;
  return Object.values(record.layout.panels).every(
    (p) =>
      p &&
      typeof p === "object" &&
      (p as { contentComponent?: string }).contentComponent === "workspace",
  );
}
