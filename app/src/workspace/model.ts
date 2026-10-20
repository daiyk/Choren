import type { Box, ProjectDocument } from "../format/types";

export interface Level {
  boxId: number;
  layer: number;
  keyframe: number;
}
export interface GraphAddress {
  behaviorId: string;
  levels: Level[];
}
export interface Viewport {
  x: number;
  y: number;
  zoom: number;
}
export interface Selection {
  nodes: string[];
  edges: string[];
}
export interface TimelineView {
  frame: number;
  scale: number;
  scroll: number;
}
export interface WorkspaceState {
  session: number;
  project?: ProjectDocument;
  activeBehavior?: string;
  addresses: Record<string, GraphAddress>;
  selections: Record<string, Selection>;
  viewports: Record<string, Viewport>;
  timelines: Record<string, TimelineView>;
}
export const emptySelection: Selection = { nodes: [], edges: [] };
export const initialWorkspace: WorkspaceState = {
  session: 0,
  addresses: {},
  selections: {},
  viewports: {},
  timelines: {},
};
/** Backend boxes can edit empty scripts, including diagrams and timelines; previews need text. */
export const scriptOpenable = (box: Box) =>
  box.script !== "" || box.node !== undefined;
export const graphId = (projectId: string, address: GraphAddress) =>
  JSON.stringify([projectId, address.behaviorId, address.levels]);
export const rootAddress = (
  behaviorId: string,
  rootId: number,
): GraphAddress => ({
  behaviorId,
  levels: [{ boxId: rootId, layer: 0, keyframe: 0 }],
});

export function resolveAddress(
  project: ProjectDocument | undefined,
  address: GraphAddress | undefined,
) {
  const behavior = project?.behaviors.find((b) => b.id === address?.behaviorId);
  if (!behavior || !address?.levels.length) return undefined;
  let owner = behavior.root;
  const owners: Box[] = [owner];
  for (let i = 1; i < address.levels.length; i++) {
    const previous = address.levels[i - 1];
    const child = owner.timeline?.layers[previous.layer]?.keyframes[
      previous.keyframe
    ]?.diagram?.boxes.find((b) => b.id === address.levels[i].boxId);
    if (!child) return undefined;
    owner = child;
    owners.push(owner);
  }
  const top = address.levels.at(-1)!;
  return {
    behavior,
    owner,
    owners,
    top,
    diagram:
      owner.timeline?.layers[top.layer]?.keyframes[top.keyframe]?.diagram,
  };
}

export type WorkspaceAction =
  | { type: "close" }
  | { type: "load"; project: ProjectDocument }
  /** An edited version of the same project: keeps navigation, selection and views. */
  | { type: "replace"; project: ProjectDocument }
  | { type: "navigate"; address: GraphAddress; select?: string }
  | { type: "activate"; behaviorId: string }
  | { type: "select"; key: string; selection: Selection }
  | { type: "viewport"; key: string; viewport: Viewport }
  | { type: "timeline"; key: string; value: TimelineView };

export function workspaceReducer(
  state: WorkspaceState,
  action: WorkspaceAction,
): WorkspaceState {
  if (action.type === "close") return {...initialWorkspace, session: state.session + 1};
  if (action.type === "activate")
    return state.addresses[action.behaviorId]
      ? { ...state, activeBehavior: action.behaviorId }
      : state;
  if (action.type === "load") {
    const addresses = Object.fromEntries(
      action.project.behaviors.map((b) => [b.id, rootAddress(b.id, b.root.id)]),
    );
    return {
      ...initialWorkspace,
      session: state.session + 1,
      project: action.project,
      activeBehavior: action.project.behaviors[0]?.id,
      addresses,
    };
  }
  if (action.type === "replace") {
    const project = action.project;
    const addresses = Object.fromEntries(
      project.behaviors.map((b) => {
        const kept = state.addresses[b.id];
        return [
          b.id,
          kept && resolveAddress(project, kept)
            ? kept
            : rootAddress(b.id, b.root.id),
        ];
      }),
    );
    const activeBehavior =
      state.activeBehavior && addresses[state.activeBehavior]
        ? state.activeBehavior
        : project.behaviors[0]?.id;
    return { ...state, project, addresses, activeBehavior };
  }
  if (action.type === "navigate") {
    if (!state.project || !resolveAddress(state.project, action.address))
      return state;
    const key = graphId(state.project.id, action.address);
    return {
      ...state,
      activeBehavior: action.address.behaviorId,
      addresses: {
        ...state.addresses,
        [action.address.behaviorId]: action.address,
      },
      selections: {
        ...state.selections,
        [key]: action.select
          ? { nodes: [action.select], edges: [] }
          : emptySelection,
      },
    };
  }
  if (action.type === "select")
    return {
      ...state,
      selections: { ...state.selections, [action.key]: action.selection },
    };
  if (action.type === "viewport")
    return {
      ...state,
      viewports: { ...state.viewports, [action.key]: action.viewport },
    };
  return {
    ...state,
    timelines: { ...state.timelines, [action.key]: action.value },
  };
}

export interface OutlineEntry {
  box: Box;
  address: GraphAddress;
  path: string;
  depth: number;
  kind: "behavior" | "box";
}
/** Every box carries the address of its containing diagram, so repeated local IDs are safe. */
export function outline(project: ProjectDocument): OutlineEntry[] {
  const entries: OutlineEntry[] = [];
  for (const b of project.behaviors) {
    const root = rootAddress(b.id, b.root.id);
    entries.push({
      box: b.root,
      address: root,
      path: b.name,
      depth: 0,
      kind: "behavior",
    });
    const walk = (
      owner: Box,
      address: GraphAddress,
      path: string,
      depth: number,
    ) => {
      owner.timeline?.layers.forEach((layer, li) =>
        layer.keyframes.forEach((kf, ki) => {
          const at = {
            ...address,
            levels: [
              ...address.levels.slice(0, -1),
              { ...address.levels.at(-1)!, layer: li, keyframe: ki },
            ],
          };
          const prefix = owner.timeline?.enabled
            ? `${path} / ${layer.name} / ${kf.name}`
            : path;
          for (const box of kf.diagram?.boxes ?? []) {
            entries.push({
              box,
              address: at,
              path: `${prefix} / ${box.name}`,
              depth,
              kind: "box",
            });
            walk(
              box,
              {
                ...at,
                levels: [
                  ...at.levels,
                  { boxId: box.id, layer: 0, keyframe: 0 },
                ],
              },
              `${prefix} / ${box.name}`,
              depth + 1,
            );
          }
        }),
      );
    };
    walk(b.root, root, b.name, 1);
  }
  return entries;
}

/** Newer open requests always win, including cancellation of an older slow read. */
export class OpenRequests {
  private generation = 0;
  begin() {
    return ++this.generation;
  }
  current(ticket: number) {
    return ticket === this.generation;
  }
}
