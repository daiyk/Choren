// Model of a Choregraphe 2.8 behavior (XAR v3).

export interface Port {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  id: number;
  name: string;
  type: number; // IO_TYPE index
  typeSize?: number;
  nature: number; // INPUT_NATURE / OUTPUT_NATURE index
  inner: boolean;
  tooltip: string;
  /** ALMemory key of an event input (nature 4; backend documents only). */
  stm?: string;
}

export interface Param {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  id: number;
  name: string;
  contentType: number; // CONTENT_TYPE index
  value: string;
  tooltip: string;
  // Backend documents only:
  defaultValue?: string;
  inheritsFromParent?: boolean;
  min?: string;
  max?: string;
  customChoice?: boolean;
  choices?: { node: number; value: string }[];
}

export interface Resource {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  name: string;
  type: string;
  timeout: string;
}

/** owner 0 = the box that contains this diagram (its ports sit on the rails). */
export interface Link {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  outOwner: number;
  outPort: number;
  inOwner: number;
  inPort: number;
}

export interface Diagram {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  scale: number;
  boxes: Box[];
  links: Link[];
}

export interface BehaviorKeyframe {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  name: string;
  index: number;
  diagram?: Diagram;
}

export interface BehaviorLayer {
  mute?: boolean;
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  name: string;
  keyframes: BehaviorKeyframe[];
}

export interface ActuatorCurve {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  actuator: string;
  frames: number[];
  /** Key values (backend documents only). */
  values?: number[];
  /** 0 degree, 1 ratio (hands), -1 legacy (backend documents only). */
  unit?: number;
  mute?: boolean;
  keys?: CurveKey[];
}

export interface Tangent {
  node: number;
  interp: string;
  abscissa: number;
  ordinate: number;
}

/** A curve key; no tangents means automatic Bezier interpolation. */
export interface CurveKey {
  node: number;
  frame: number;
  value: number;
  smooth: boolean;
  symmetrical: boolean;
  left?: Tangent;
  right?: Tangent;
}

export interface Timeline {
  startFrame?: number;
  endFrame?: number;
  resourcesAcquisition?: number;
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  enabled: boolean;
  fps: number;
  size: number;
  layers: BehaviorLayer[];
  curves: ActuatorCurve[];
}

export interface Box {
  /** Backend node handle used by edit commands (absent for preview-parsed documents). */
  node?: number;
  id: number;
  name: string;
  x: number;
  y: number;
  /** Optional presentation category; does not change execution or port semantics. */
  category?: string;
  tooltip: string;
  bitmaps: string[];
  script: string;
  plugin: string;
  /** Text Edit / Comment plugin payload, separate from its generated script. */
  pluginText?: string;
  dialogFile?: string;
  inputs: Port[];
  outputs: Port[];
  params: Param[];
  resources: Resource[];
  timeline?: Timeline;
}

export interface Behavior {
  id: string;
  name: string;
  source: string;
  root: Box;
}

export interface ProjectFile {
  path: string;
  kind:
    | "behavior"
    | "manifest"
    | "resource"
    | "dialog"
    | "topic"
    | "translation"
    | "project";
  behaviorId?: string;
  missing?: boolean;
}

export interface Diagnostic {
  id: string;
  severity: "warning" | "error";
  message: string;
  path?: string;
}

export interface ProjectDocument {
  /** Backend session handle for edit/undo/redo/save commands (absent for preview-parsed documents). */
  session?: number;
  id: string;
  source: string;
  name: string;
  format: "pml" | "crg" | "xar";
  behaviors: Behavior[];
  files: ProjectFile[];
  metadata: Record<string, string>;
  diagnostics: Diagnostic[];
}

// Enum values as stored in XAR attributes (index = stored number).
export const IO_TYPE = ["Dynamic", "Bang", "Number", "String"];
export const INPUT_NATURE = [
  "onLoad",
  "onEvent",
  "onStart",
  "onStop",
  "ALMemory",
];
export const OUTPUT_NATURE = [
  "Undefined",
  "onStopped",
  "punctual",
  "unknown",
  "recurrent",
];
export const CONTENT_TYPE = [
  "Bool",
  "Int",
  "Double",
  "String",
  "Resource",
  "Localized string",
];
