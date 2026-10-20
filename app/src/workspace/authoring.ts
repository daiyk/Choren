import { invoke } from "@tauri-apps/api/core";
import type { ProjectDocument } from "../format/types";
import { native } from "../platform";

/** Backend edit command (crates/choren-core/src/edit.rs). */
export type EditCommand =
  | {
      type: "setAttributes";
      node: number;
      values: Record<string, string | null>;
    }
  | { type: "moveBoxes"; moves: [number, number, number][] }
  | { type: "setScript"; box: number; text: string }
  | { type: "setBoxContent"; box: number; bitmaps?: string[]; pluginText?: string }
  | { type: "setDialogFile"; box: number; file: string }
  | { type: "sortPorts"; box: number; ports: number[] }
  | { type: "translateKeyframes"; keyframes: number[]; delta: number }
  | { type: "pasteKeyframes"; layer: number; xml: string; index: number }
  | { type: "setTimelineRange" | "stretchTimelineRange"; timeline: number; start: number; end: number }
  | { type: "setCurveValues"; keys: number[]; scale: number; offset: number }
  | { type: "moveTangent"; tangent: number; abscissa: number; ordinate: number }
  | { type: "pasteCurveKeys"; timeline: number; xml: string; frame: number }
  | { type: "simplifyCurves"; timeline: number; curves: number[]; from: number; to: number; tolerance: number; mode?: SimplifyMode; advanced?: [number, number] }
  | {
      type: "newBox";
      diagram: number;
      kind: NewBoxKind;
      dialogFile?: string;
      name: string;
      x: number;
      y: number;
    }
  | {
      type: "addLink";
      diagram: number;
      outOwner: number;
      outPort: number;
      inOwner: number;
      inPort: number;
    }
  | { type: "remove"; nodes: number[] }
  | {
      type: "addPort";
      box: number;
      kind: "Input" | "Output" | "Parameter";
      name: string;
      values: Record<string, string>;
    }
  | { type: "insertXml"; parent: number; xml: string }
  | { type: "paste"; diagram: number; xml: string; dx: number; dy: number }
  | { type: "convertToBox"; diagram: number; boxes: number[]; name: string }
  | {
      type: "storeKeys";
      timeline: number;
      frame: number;
      group: string;
      angles: number[];
    }
  | { type: "removeKeys"; timeline: number; frame: number; group: string; to?: number }
  | { type: "moveKeys"; keys: number[]; delta: number }
  | { type: "setInterpolation"; keys: number[]; interp: Interp }
  | { type: "mirror"; timeline: number; mode: MirrorMode; frames?: [number, number] }
  | {
      /** Motion recording: keys in file units at `frame`. */
      type: "recordKeys";
      timeline: number;
      frame: number;
      /** [actuator, value in file units (degrees, hands 0–1), is hand] */
      values: [string, number, boolean][];
    };
export type Interp = "Constant" | "Linear" | "Bezier" | "BezierAuto";
export type MirrorMode = "Flip" | "LeftToRight" | "RightToLeft";
/** Choren's own simplifier (the default) or the Choregraphe-compatible one. */
export type SimplifyMode = "Choren" | "Choregraphe";
export type NewBoxKind = "Python" | "Diagram" | "Timeline" | "Dialog";
export type FilePolicy = "UseExisting" | "Replace" | "Rename";

export interface HistoryState {
  revision: number;
  dirty: boolean;
  undo?: string;
  redo?: string;
  created: number[];
  undoSteps?: string[];
  redoSteps?: string[];
}
export interface Change {
  history: HistoryState;
  document: ProjectDocument;
}
export interface Manifest {
  uuid: string;
  version: string;
  names: [string, string][];
  supportedLanguages: string[];
  descriptions?: [string, string][];
  /** One per `<behaviorContent>`; omitted or null when saving leaves them unchanged. */
  behaviors?: BehaviorManifest[] | null;
  requirements?: Requirements | null;
}

/** Choregraphe's behavior properties. Lists are [language, text]; conditions [purpose, expression]. */
export interface BehaviorManifest {
  path: string;
  nature: "interactive" | "solitary";
  userRequestable: boolean;
  names: [string, string][];
  descriptions: [string, string][];
  tags: [string, string][];
  triggerSentences: [string, string][];
  loadingResponses: [string, string][];
  conditions: [string, string][];
  permissions: ("canRunInSleep" | "canRunOnPod" | "canStandUp" | "canSitDown")[];
}

export interface Requirements {
  /** [minVersion, maxVersion]; an empty string leaves the bound out. */
  naoqi: [string, string][];
  robots: { model: string; minHeadVersion: string; maxHeadVersion: string; minBodyVersion: string; maxBodyVersion: string }[];
}

/** Editing needs a backend session; preview-parsed documents stay read-only. */
export const editable = (p?: ProjectDocument) => native && !!p?.session;

const withSession = (session: number, c: Change): Change => ({
  ...c,
  document: { ...c.document, session },
});
const imported = (session: number, r: Change | { conflicts: string[] }) =>
  "conflicts" in r ? r : withSession(session, r);

export const authoring = {
  edit: async (
    session: number,
    behavior: string,
    label: string,
    commands: EditCommand[],
  ) =>
    withSession(
      session,
      await invoke<Change>("project_edit", {
        session,
        behavior,
        label,
        commands,
      }),
    ),
  undo: async (session: number) => {
    const c = await invoke<Change | null>("project_undo", { session });
    return c && withSession(session, c);
  },
  redo: async (session: number) => {
    const c = await invoke<Change | null>("project_redo", { session });
    return c && withSession(session, c);
  },
  save: (session: number) => invoke<HistoryState>("project_save", { session }),
  copy: (session: number, behavior: string, boxes: number[]) =>
    invoke<string>("project_copy", { session, behavior, boxes }),
  addBehavior: async (session: number, name: string, root: NewBoxKind) =>
    withSession(
      session,
      await invoke<Change>("project_add_behavior", { session, name, root }),
    ),
  removeBehavior: async (session: number, behavior: string) =>
    withSession(
      session,
      await invoke<Change>("project_remove_behavior", { session, behavior }),
    ),
  /** Same-named content changes nothing and comes back as `conflicts`; retry with a `policy`. */
  importBehavior: async (session: number, path: string, name: string, policy?: FilePolicy) =>
    imported(session, await invoke<Change | { conflicts: string[] }>("project_import_behavior", { session, path, name, policy })),
  importFiles: async (session: number, paths: string[], destination: string, folder: boolean, policy?: FilePolicy) =>
    imported(session, await invoke<Change | { conflicts: string[] }>("project_import_files", { session, paths, destination, folder, policy })),
  exportMotionPython: (session: number, behavior: string, timeline: number, bezier: boolean) =>
    invoke<string>("project_export_motion_python", { session, behavior, timeline, bezier }),
  manifest: (session: number) =>
    invoke<Manifest | null>("project_manifest", { session }),
  setManifest: async (session: number, manifest: Manifest) =>
    withSession(
      session,
      await invoke<Change>("project_set_manifest", { session, manifest }),
    ),
  insertLibraryBox: async (
    session: number,
    args: {
      behavior: string;
      diagram: number;
      path: string;
      x: number;
      y: number;
      policy?: FilePolicy;
    },
  ) => {
    const r = await invoke<{
      insert:
        | { result: "done"; value: HistoryState }
        | { result: "conflicts"; value: string[] };
      document: ProjectDocument;
    }>("project_insert_library_box", { session, ...args });
    return { ...r, document: { ...r.document, session } };
  },
  newProject: (dir: string, name: string) =>
    invoke<ProjectDocument>("project_new", { dir, name }),
  updateTranslations: async (session: number) =>
    withSession(
      session,
      await invoke<Change>("project_update_translations", { session }),
    ),
  addDialog: async (session: number, name: string, languages: string[]) =>
    withSession(
      session,
      await invoke<Change>("project_add_dialog", { session, name, languages }),
    ),
  resource: async (session: number, path: string, target?: string) =>
    withSession(session, await invoke<Change>("project_resource", {session, path, target: target ?? null})),
  readText: (session: number, path: string) =>
    invoke<string>("project_read_text", { session, path }),
  writeText: async (session: number, path: string, text: string) =>
    withSession(
      session,
      await invoke<Change>("project_write_text", { session, path, text }),
    ),
  buildPackage: (session: number, path: string) => invoke<void>("project_build_package", {session, path}),
  exportCrg: (session: number, behavior: string, path: string) =>
    invoke<void>("project_export_crg", { session, behavior, path }),
  saveAsFolder: (session: number, dir: string, name: string) =>
    invoke<ProjectDocument>("project_save_as_folder", { session, dir, name }),
  exportQianim: (
    session: number,
    behavior: string,
    timeline: number,
    path: string,
  ) =>
    invoke<void>("project_export_qianim", {
      session,
      behavior,
      timeline,
      path,
    }),
  importQianim: async (
    session: number,
    behavior: string,
    timeline: number,
    path: string,
  ) =>
    withSession(
      session,
      await invoke<Change>("project_import_qianim", {
        session,
        behavior,
        timeline,
        path,
      }),
    ),
};
