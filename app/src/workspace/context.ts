import { createContext, useContext } from "react";
import type { DockviewApi } from "dockview-react";
import type { Box, Diagnostic, ProjectFile } from "../format/types";
import type { GraphControls } from "../diagram/Diagram";
import type {
  GraphAddress,
  OutlineEntry,
  Selection,
  TimelineView,
  Viewport,
  WorkspaceState,
} from "./model";
import type { Command } from "./registry";
import type { Change, EditCommand, HistoryState } from "./authoring";
import type { GraphStyle } from "./storage";
export interface ScriptDocument {
  box: Box;
  address?: GraphAddress;
}
export interface CommandTarget { behaviorId?: string; address?: GraphAddress; boxId?: number; destination?: string; }
export interface WorkspaceContextValue {
  state: WorkspaceState;
  graphStyle: GraphStyle;
  commands: Command[];
  diagnostics: Diagnostic[];
  scripts: Record<string, ScriptDocument>;
  inspectedBoxes: Box[];
  inspectedEdges: string[];
  inspectedFile?: ProjectFile;
  loading: boolean;
  dock?: DockviewApi;
  execute: (id: string, target?: CommandTarget) => void;
  showPanel: (id: string) => void;
  openExample: () => void;
  openBehavior: (id: string) => void;
  navigate: (address: GraphAddress, select?: string) => void;
  reveal: (entry: OutlineEntry) => void;
  openBox: (box: Box, address: GraphAddress) => void;
  openScript: (box: Box, address?: GraphAddress, identity?: string) => void;
  inspectFile: (file: ProjectFile) => void;
  inspectLibrary: (box: Box, identity: string) => void;
  select: (key: string, selection: Selection) => void;
  saveViewport: (key: string, viewport: Viewport) => void;
  saveTimeline: (key: string, value: TimelineView) => void;
  registerGraph: (id: string, controls: GraphControls | undefined) => void;
  managePanel: (id: string) => void;
  /** True when the open project has a backend session (desktop app, not the bundled example). */
  editable: boolean;
  /** The Inspector shows boxes of the active diagram (not a library preview or script tab). */
  inspectorEditable: boolean;
  history?: HistoryState;
  /** Applies commands to the active behavior as one undo step. */
  edit: (
    label: string,
    commands: EditCommand[],
    behaviorId?: string,
  ) => Promise<Change | undefined>;
  /** Runs a backend change (undo, import…) and shows the returned document. */
  applyChange: (
    work: () => Promise<Change | null | undefined>,
  ) => Promise<Change | undefined>;
  /** Shows a notification with explicit severity. */
  notify: (message: string, kind?: "info" | "error") => void;
  insertLibraryBox: (path: string, target?: { address: GraphAddress; position: { x: number; y: number } }) => void;
  /** Unapplied script text per script tab; kept when the tab closes, counted by the guard. */
  setDraft: (scriptId: string, text: string | undefined) => void;
  subscribeDraft: (listener: () => void) => () => void;
  draftOf: (scriptId: string) => string | undefined;
}
export const WorkspaceContext = createContext<
  WorkspaceContextValue | undefined
>(undefined);
export const useWorkspace = () => {
  const context = useContext(WorkspaceContext);
  if (!context) throw new Error("Workspace context is missing");
  return context;
};
