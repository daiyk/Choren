import type { ProjectDocument, ProjectFile } from "../format/types";
import { authoring } from "./authoring";
import type { WorkspaceContextValue } from "./context";

export interface FileTree {
  name: string;
  path: string;
  file?: ProjectFile;
  children: FileTree[];
  /** Lowest kind rank inside: entries reorder only among siblings of the same group. */
  group: number;
}

/** PML section order (choren-core `kind_rank`): the tree lists files as reopening the project does. */
const KINDS = ["project", "behavior", "resource", "dialog", "topic", "translation", "manifest"];
export const kindRank = (kind: string) => {
  const i = KINDS.indexOf(kind);
  return i < 0 ? 6 : i;
};

export const under = (path: string, prefix: string) => path === prefix || path.startsWith(`${prefix}/`);
export const parentOf = (path: string) => path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
export const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);
/** The folder holding a behavior's files (its `.xar` alone at the project root). */
export const behaviorFolder = (source: string) => source.includes("/") ? parentOf(source) : source;

export function fileTree(files: ProjectFile[]): FileTree[] {
  const root: FileTree[] = [];
  const sorted = files.map((file, i) => ({file, i})).sort((a, b) => kindRank(a.file.kind) - kindRank(b.file.kind) || a.i - b.i);
  for (const {file} of sorted) {
    let children = root;
    let path = "";
    const rank = kindRank(file.kind);
    const parts = file.path.split("/");
    parts.forEach((name, i) => {
      path += `${path ? "/" : ""}${name}`;
      let entry = children.find((child) => child.name === name);
      if (!entry) {
        entry = { name, path, children: [], group: rank };
        children.push(entry);
      }
      entry.group = Math.min(entry.group, rank);
      if (i === parts.length - 1) entry.file = file;
      children = entry.children;
    });
  }
  return root;
}

/** Moves a file or folder (files move on save) and carries file drafts and the inspected file along. */
export async function moveEntry(ws: WorkspaceContextValue, from: string, to: string) {
  const project = ws.state.project;
  const session = project?.session;
  if (!project || !session) return undefined;
  const result = await ws.applyChange(() => authoring.movePath(session, from, to));
  if (!result) return undefined;
  const moved = (path: string) => `${to}${path.slice(from.length)}`;
  for (const f of project.files.filter(f => under(f.path, from))) {
    const id = `file:${session}:${f.path}`;
    const draft = ws.draftOf(id);
    if (draft !== undefined) { ws.setDraft(`file:${session}:${moved(f.path)}`, draft); ws.setDraft(id, undefined); }
  }
  const inspected = ws.inspectedFile;
  if (inspected && under(inspected.path, from)) {
    const file = result.document.files.find(f => f.path === moved(inspected.path));
    if (file) ws.inspectFile(file);
  }
  const behaviors = project.behaviors.some(b => under(b.source, from));
  ws.notify(`${parentOf(from) === parentOf(to) ? "Renamed" : "Moved"} ${from} to ${to}. Files move when you save; Undo reverts it.${behaviors ? " Scripts that start a behavior by its path are not updated." : ""}`);
  return result;
}

/** Behaviors moved into the folder `path` (or the behavior itself). */
export const behaviorsUnder = (project: ProjectDocument, path: string) => project.behaviors.filter(b => under(b.source, path));
