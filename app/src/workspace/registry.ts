export interface PanelDefinition {
  id: string;
  title: string;
  region: "left" | "right" | "bottom" | "center";
  available: boolean;
  reason?: string;
}
export const panels: PanelDefinition[] = [
  { id: "project", title: "Project", region: "left", available: true },
  { id: "outline", title: "Outline", region: "left", available: true },
  { id: "libraries", title: "Libraries", region: "left", available: true },
  { id: "inspector", title: "Inspector", region: "right", available: true },
  { id: "timeline", title: "Timeline", region: "bottom", available: true },
  { id: "problems", title: "Problems", region: "bottom", available: true },
  { id: "robot", title: "Robot view", region: "right", available: true },
  { id: "robot-dialog", title: "Robot dialog", region: "bottom", available: true },
  { id: "logs", title: "Robot logs", region: "bottom", available: true },
  { id: "curves", title: "Motion curves", region: "center", available: true },
  { id: "dialog", title: "Dialog / QiChat", region: "center", available: true },
  { id: "translations", title: "Translations", region: "center", available: true },
  { id: "poses", title: "Pose library", region: "left", available: true },
  { id: "memory", title: "Memory watcher", region: "bottom", available: true },
  { id: "services", title: "Service inspector", region: "center", available: true },
  { id: "camera", title: "Camera", region: "right", available: true },
  { id: "applications", title: "Robot applications", region: "right", available: true },
  { id: "active-content", title: "Active content", region: "bottom", available: true },
  { id: "resource", title: "Resource editor", region: "center", available: true },
  { id: "history", title: "Undo history", region: "right", available: true },
  { id: "recording", title: "Motion recording", region: "bottom", available: true },
  { id: "files", title: "Robot files", region: "center", available: true },
  ...[
    ["debug", "Behavior debugger", "bottom", "Execution tracing and breakpoints are planned."],
    ["plugins", "Box plugin interfaces", "center", "Custom box interfaces require further behavior research."],
  ].map(([id, title, region, reason]) => ({
    id,
    title,
    region: region as PanelDefinition["region"],
    available: false,
    reason,
  })),
];
export interface Command {
  id: string;
  title: string;
  group: "File" | "Edit" | "View" | "Connection" | "Help";
  shortcut?: string;
  enabled: boolean;
  reason?: string;
  run: () => void;
}
/** Authoring commands (save, undo, insert…) live in App.tsx; these still need the robot bridge. */
export const unavailableActions = [
  [
    "deploy",
    "Deploy project",
    "Connection",
    "Packaging and robot deployment are planned.",
  ],
] as const;
