import { ServicesPanel, ApplicationsPanel, CameraPanel } from "../robot/services";
import { ActiveContentPanel } from "../robot/ActiveContent";
import { ResourcePanel, HistoryPanel, useResourceActions } from "./ResourcePanel";
import { authoring } from "./authoring";
import { createContext, lazy, Suspense, useCallback, useContext, useLayoutEffect, useMemo, useState } from "react";
import type { IDockviewPanelProps } from "dockview-react";
import {
  AlertTriangle,
  ChevronRight,
  Code2,
  File,
  FolderOpen,
  GitBranch,
  Search,
  Settings,
  SlidersHorizontal,
  Plus,
  Copy,
  ClipboardPaste,
  PenLine,
  Trash2,
  Workflow,
} from "lucide-react";
import type { Box, Port, ProjectDocument } from "../format/types";
import { Diagram } from "../diagram/Diagram";
import { Timeline, type TimelineMenuTarget } from "../timeline/Timeline";
import { useContextMenu, copyText, type MenuAction } from "./ContextMenu";
import { MotionPanel, useActiveTimeline } from "../timeline/Curves";
import { RobotDialogPanel } from "../robot/DialogPanel";
import { DialogPanel, TranslationsPanel } from "./projectText";
import { Inspector } from "../Inspector";
import { LogsPanel, RobotPanel } from "../robot/panels";
import { MemoryPanel, PosesPanel, RecordingPanel } from "../robot/tools";
import { FilesPanel } from "../robot/files";
import { inputActivationError, getRobot, parameterValue, robot, useRobot } from "../robot/store";
import { InputActivation } from "../robot/InputActivation";
import { LibrariesPanel } from "./LibrariesPanel";
import { native, PROJECT_FILE_MIME } from "../platform";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Modal } from "./Modal";
import { ImportMenu } from "./ImportContent";
import { AppMark } from "./WelcomeSetup";
import {
  emptySelection,
  graphId,
  outline,
  resolveAddress,
  rootAddress,
  scriptOpenable,
  type GraphAddress,
} from "./model";
import { useWorkspace } from "./context";
import { panels } from "./registry";
import { behaviorFolder, fileTree, moveEntry, moveInTree, nameOf, parentOf, under, type FileTree } from "./projectTree";
const ScriptViewer = lazy(() =>
  import("./ScriptViewer").then((module) => ({ default: module.ScriptViewer })),
);

export interface WorkspacePanelParams {
  kind: string;
  behaviorId?: string;
  scriptId?: string;
}
export function Empty({
  title,
  children,
  icon = <Workflow size={28} />,
}: {
  title: string;
  children?: React.ReactNode;
  icon?: React.ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon}
      <h2>{title}</h2>
      {children}
    </div>
  );
}
export function Welcome() {
  const ws = useWorkspace();
  return (
    <div className="welcome">
      <AppMark className="welcome-mark" />
      <span className="eyebrow">CHOREN STUDIO</span>
      <h1>
        A clearer view of
        <br />
        your robot behaviors.
      </h1>
      <p>
        Explore projects, follow connections, and inspect scripts in a workspace
        that adapts to you.
      </p>
      <div className="welcome-actions">
        <button
          className="primary"
          onClick={() => ws.execute("open")}
          disabled={ws.loading}
        >
          <FolderOpen size={16} />
          Open project
        </button>
        <button onClick={ws.openExample}>
          Explore example
          <ChevronRight size={16} />
        </button>
      </div>
      <div className="welcome-features">
        <span>
          <GitBranch size={16} />
          Nested flow diagrams
        </span>
        <span>
          <Code2 size={16} />
          Python script inspection
        </span>
        <span>
          <SlidersHorizontal size={16} />
          Your workspace, your layout
        </span>
      </div>
      <div className="welcome-note">
        {native ? "Open or create" : "Browse"} · .pml · .crg · .xar
      </div>
    </div>
  );
}
type Workspace = ReturnType<typeof useWorkspace>;
type DropZone = "before" | "after" | "into";
/** What is being dragged in the Project tree or the Outline (HTML drag data is unreadable during dragover). */
let dragging: { kind: "entry"; path: string; fixed: boolean } | { kind: "behavior"; id: string } | undefined;
/** The highlighted row of the Project tree or the Outline: what a click, a menu or a drag acts on. */
const Selection = createContext<{ selected?: string; select: (key: string) => void }>({ select: () => {} });
// Pointer down, not focus alone: WebKit does not focus buttons on click.
const pick = ({ selected, select }: { selected?: string; select: (key: string) => void }, key: string) =>
  ({ "data-selected": selected === key || undefined, onPointerDown: () => select(key), onFocus: () => select(key) });
const useSelectable = (key: string) => pick(useContext(Selection), key);
/** Outline rows: an owner (`address`), a layer, a keyframe or a box in a diagram (`at`). */
const outlineKey = (address: GraphAddress, part = "") => `outline:${JSON.stringify(address)}${part}`;
const clearDrop = () => document.querySelectorAll<HTMLElement>("[data-drop]").forEach(el => delete el.dataset.drop);
const zoneOf = (event: React.DragEvent<HTMLElement>, folder: boolean): DropZone => {
  const r = event.currentTarget.getBoundingClientRect();
  const f = (event.clientY - r.top) / Math.max(1, r.height);
  return folder ? f < 0.3 ? "before" : f > 0.7 ? "after" : "into" : f < 0.5 ? "before" : "after";
};
/** Behavior order (PML and manifest), shown by the Outline. */
const reorder = (ws: Workspace, behavior: string, target: string, after: boolean) => {
  const session = ws.state.project?.session;
  if (session) void ws.applyChange(() => authoring.reorder(session, behavior, target, after));
};
const structural = (ws: Workspace) => ws.editable && ws.state.project?.format === "pml";
/** `moves` replaces Move up/down: Project tree rows move in the tree, the Outline in behavior order. */
function behaviorActions(ws: Workspace, behaviorId: string, moves?: MenuAction[]): MenuAction[] {
  const editable = structural(ws);
  const behaviors = ws.state.project?.behaviors ?? [];
  const i = behaviors.findIndex(b => b.id === behaviorId);
  const behavior = behaviors[i];
  const reason = "An editable project folder is required.";
  return [
    {label: "Open behavior", run: () => ws.openBehavior(behaviorId)},
    {label: "Rename behavior…", run: () => ws.execute("rename-behavior", {behaviorId}), disabled: !editable, reason},
    {label: "Duplicate behavior", run: () => ws.execute("duplicate-behavior", {behaviorId}), disabled: !editable, reason},
    ...moves ?? [
      {label: "Move up", run: () => reorder(ws, behaviorId, behaviors[i - 1].id, false), disabled: !editable || i < 1, reason: editable ? "This is the first behavior." : reason},
      {label: "Move down", run: () => reorder(ws, behaviorId, behaviors[i + 1].id, true), disabled: !editable || i < 0 || i >= behaviors.length - 1, reason: editable ? "This is the last behavior." : reason},
    ],
    {label: "Copy name", run: () => copyText(behavior?.name ?? ""), disabled: !behavior},
    {label: "Copy path", run: () => copyText(behaviorFolder(behavior?.source ?? "")), disabled: !behavior},
    {label: "Behavior properties…", run: () => ws.execute("properties", {behaviorId}), disabled: !editable, reason},
    {label: "Export behavior…", run: () => ws.execute("export-crg", {behaviorId}), disabled: !ws.editable, reason: "Open this project in the desktop app."},
    {label: "Delete behavior…", run: () => ws.execute("remove-behavior", {behaviorId}), disabled: !editable, reason},
  ];
}
function boxActions(ws: Workspace, box: Box, address: GraphAddress): MenuAction[] {
  const target = {address, boxId: box.id};
  const reveal = () => ws.navigate(address, String(box.id));
  return [
    {label: "Reveal in diagram", run: reveal},
    {label: box.timeline ? "Open contents" : "Open script", run: () => ws.openBox(box, address), disabled: !box.timeline && !scriptOpenable(box), reason: "This box has no editable contents."},
    {label: "Properties", run: () => {reveal(); ws.showPanel("inspector");}},
    {label: "Rename box…", run: () => ws.execute("rename-box", target), disabled: !ws.editable || box.node === undefined, reason: "This box is read-only."},
    {label: "Copy name", run: () => copyText(box.name)},
    ...[["cut", "Cut"], ["copy", "Copy"], ["convert-to-box", "Group into box…"], ["delete", "Delete box"]].map(([id, label]) => ({label, run: () => ws.execute(id, target), disabled: !ws.editable || box.node === undefined, reason: "This box is read-only."})),
  ];
}
const containsProjectFile = (entry: FileTree): boolean =>
  entry.file?.kind === "project" || entry.file?.kind === "manifest" || entry.children.some(containsProjectFile);
/** Drag and drop in the Project tree: the upper or lower edge of a sibling reorders (the tree's
 * own order); the middle of a folder moves the entry into it. */
function treeDrop(entry: FileTree, zone: DropZone) {
  const d = dragging;
  if (d?.kind !== "entry" || d.path === entry.path) return undefined;
  if (zone === "into") return entry.children.length && !d.fixed && parentOf(d.path) !== entry.path && !under(entry.path, d.path) && !containsProjectFile(entry) ? "into" : undefined;
  return parentOf(d.path) === parentOf(entry.path) ? "reorder" : undefined;
}
function useTreeDrag(entry: FileTree, dragPath: string, fixed: boolean, siblings: FileTree[]) {
  const ws = useWorkspace();
  const folder = entry.children.length > 0;
  // Read-only projects keep their order; resources still drag onto boxes.
  const source = ws.editable && ws.state.project?.format === "pml" ? {
    draggable: true,
    onDragStart: (event: React.DragEvent<HTMLElement>) => {
      event.stopPropagation();
      dragging = {kind: "entry", path: dragPath, fixed};
      event.dataTransfer.setData("text/plain", dragPath);
      if (entry.file?.kind === "resource") { event.dataTransfer.setData(PROJECT_FILE_MIME, entry.path); event.dataTransfer.effectAllowed = "linkMove"; }
      else event.dataTransfer.effectAllowed = "move";
    },
    onDragEnd: () => { dragging = undefined; clearDrop(); },
  } : entry.file?.kind === "resource" ? {
    draggable: true,
    onDragStart: (event: React.DragEvent<HTMLElement>) => { event.dataTransfer.setData(PROJECT_FILE_MIME, entry.path); event.dataTransfer.effectAllowed = "link"; },
  } : {};
  const target = {
    onDragOver: (event: React.DragEvent<HTMLElement>) => {
      if (dragging?.kind !== "entry") return;
      event.stopPropagation();
      const zone = zoneOf(event, folder);
      const valid = treeDrop(entry, zone);
      if (!valid) { delete event.currentTarget.dataset.drop; return; }
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      if (event.currentTarget.dataset.drop !== zone) { clearDrop(); event.currentTarget.dataset.drop = zone; }
    },
    onDragLeave: (event: React.DragEvent<HTMLElement>) => { delete event.currentTarget.dataset.drop; },
    onDrop: (event: React.DragEvent<HTMLElement>) => {
      const d = dragging, zone = zoneOf(event, folder), action = treeDrop(entry, zone);
      clearDrop();
      dragging = undefined;
      if (d?.kind !== "entry" || !action) return;
      event.preventDefault();
      event.stopPropagation();
      if (action === "into") void moveEntry(ws, d.path, `${entry.path}/${nameOf(d.path)}`);
      else moveInTree(ws, siblings.map(s => s.name), d.path, entry.path, zone === "after");
    },
  };
  return {...source, ...target};
}
function FileBranch({entry, siblings}: {entry: FileTree; siblings: FileTree[]}) {
  const ws = useWorkspace();
  const resource = useResourceActions(entry.file);
  const menu = useContextMenu([ws.state.project, resource.busy], message => ws.notify(message, "error"));
  const open = () => entry.file?.behaviorId ? ws.openBehavior(entry.file.behaviorId) : entry.file && ws.inspectFile(entry.file);
  const editable = structural(ws);
  const fixed = containsProjectFile(entry);
  const behavior = entry.file?.behaviorId ? ws.state.project?.behaviors.find(b => b.id === entry.file!.behaviorId) : undefined;
  // A behavior row drags its folder; the project file and manifest only reorder.
  const drag = useTreeDrag(entry, behavior ? behaviorFolder(behavior.source) : entry.path, fixed || !editable, siblings);
  const selectable = useSelectable(`tree:${entry.path}`);
  const at = siblings.indexOf(entry);
  const names = siblings.map(s => s.name);
  const reason = "An editable project folder is required.";
  // The tree order is saved in the project's .chorenproj, not in the PML.
  const moves: MenuAction[] = [
    {label: "Move up", run: () => moveInTree(ws, names, entry.path, siblings[at - 1].path, false), disabled: !editable || at < 1, reason: editable ? "It is already first in its folder." : reason},
    {label: "Move down", run: () => moveInTree(ws, names, entry.path, siblings[at + 1].path, true), disabled: !editable || at < 0 || at >= siblings.length - 1, reason: editable ? "It is already last in its folder." : reason},
  ];
  return <li>{menu.element}{resource.element}{entry.children.length ? <details open>
    <summary {...drag} {...selectable} {...menu.trigger(event => {
      const details = event.currentTarget.parentElement!;
      return [
        {label: "Expand subtree", run: () => {details.querySelectorAll("details").forEach(d => d.open = true); (details as HTMLDetailsElement).open = true;}},
        {label: "Collapse subtree", run: () => {details.querySelectorAll("details").forEach(d => d.open = false); (details as HTMLDetailsElement).open = false;}},
        {label: "Rename folder…", run: () => ws.execute("rename-folder", {folder: entry.path}), disabled: !editable || fixed, reason: fixed ? "This folder holds the project file or its manifest." : reason},
        ...moves,
        {label: "Copy path", run: () => copyText(entry.path)},
        ...["files", "folder"].map(kind => ({label: `Import ${kind} here…`, run: () => ws.execute(`import-${kind}`, {destination: entry.path}), disabled: !editable, reason})),
        {label: "Delete folder…", run: () => ws.execute("remove-folder", {folder: entry.path}), disabled: !editable || fixed, reason: fixed ? "This folder holds the project file or its manifest." : reason},
      ];
    })}><FolderOpen size={14}/><span>{entry.name}</span></summary>
    <FileBranches entries={entry.children}/>
  </details> : <button className="tree-row" title={entry.path} {...drag} {...selectable}
    onClick={open} {...menu.trigger(() => entry.file?.behaviorId ? behaviorActions(ws, entry.file.behaviorId, moves) : [
      {label: "Open / Preview", run: open}, ...resource.actions, ...moves, {label: "Copy path", run: () => copyText(entry.path)},
    ])}>
    {entry.file?.kind === "behavior" ? <Workflow size={14}/> : <File size={14}/>}<span>{entry.name}</span>
    {entry.file?.missing && <AlertTriangle size={13} className="warning"/>}
  </button>}</li>;
}
function FileBranches({ entries }: { entries: FileTree[] }) {
  return <ul className="file-tree">{entries.map(entry => <FileBranch key={entry.path} entry={entry} siblings={entries}/>)}</ul>;
}
function ProjectPanel() {
  const ws = useWorkspace();
  const project = ws.state.project;
  const [query, setQuery] = useState("");
  const [selected, select] = useState<string>();
  const menu = useContextMenu([project, query], message => ws.notify(message, "error"));
  const actions = () => [["new", "New behavior…"], ["import-behavior", "Import behavior…"], ["import-files", "Import files…"], ["import-folder", "Import folder…"], ["new-dialog", "New dialog topic…"], ["rename-project", "Rename project…"], ["properties", "Project properties"]].map(([id, label]) => {
    const command = ws.commands.find(c => c.id === id);
    return {label, run: () => ws.execute(id), disabled: !command?.enabled, reason: command?.reason};
  });
  if (!project)
    return (
      <Empty title="No project open" icon={<FolderOpen size={25} />}>
        <p>Open a project to browse its behaviors and files.</p>
        <button onClick={() => ws.execute("open")}>Open project</button>
      </Empty>
    );
  return (
    <div className="panel-column" tabIndex={0} {...menu.trigger(actions)}>{menu.element}
      <div className="project-heading">
        <AppMark className="project-avatar" />
        <div className="project-info">
          <strong title={project.name} onDoubleClick={() => ws.execute("rename-project")}>{project.name}</strong>
          <small>
            {project.behaviors.length} behaviors ·{" "}
            {ws.editable
              ? ws.history?.dirty
                ? "Unsaved changes"
                : "Editable"
              : "Read-only"}
          </small>
        </div>
        {ws.editable && project.format === "pml" && <ImportMenu />}
      </div>
      {ws.editable && project.format === "pml" && (
        <div
          className="panel-toolbar project-actions"
          role="toolbar"
          aria-label="Project editing"
        >
          <button onClick={() => ws.execute("new")}>
            <Plus size={13} />
            New behavior
          </button>
          <button onClick={() => ws.execute("rename-project")}>
            <PenLine size={13} />
            Rename
          </button>
          <button onClick={() => ws.execute("properties")}>
            <Settings size={13} />
            Properties
          </button>
        </div>
      )}
      <SearchField
        label="Filter project files"
        value={query}
        onChange={setQuery}
      />
      <div className="panel-scroll"
        onDragOver={event => {
          if (dragging?.kind !== "entry" || !parentOf(dragging.path)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          if (event.currentTarget.dataset.drop !== "root") { clearDrop(); event.currentTarget.dataset.drop = "root"; }
        }}
        onDragLeave={event => { if (event.target === event.currentTarget) delete event.currentTarget.dataset.drop; }}
        onDrop={event => {
          const d = dragging;
          clearDrop();
          dragging = undefined;
          if (d?.kind !== "entry" || !parentOf(d.path)) return;
          event.preventDefault();
          void moveEntry(ws, d.path, nameOf(d.path));
        }}>
          <Selection.Provider value={{selected, select}}>
            <FileBranches
              entries={fileTree(
                project.files.filter((f) =>
                  f.path.toLowerCase().includes(query.toLowerCase()),
                ),
                project.choren?.treeOrder,
              )}
            />
          </Selection.Provider>
        {!project.behaviors.length && (
          <p className="panel-message">This project has no behaviors.</p>
        )}
      </div>
    </div>
  );
}
export function SearchField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className="search-field">
      <Search size={14} />
      <input
        aria-label={label}
        placeholder={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}
/** Outline behaviors reorder by drag: drop on the upper or lower half of another behavior. */
function outlineDrag(ws: Workspace, behaviorId: string) {
  if (!structural(ws)) return {};
  return {
    draggable: true,
    onDragStart: (event: React.DragEvent<HTMLElement>) => {
      event.stopPropagation();
      dragging = {kind: "behavior", id: behaviorId};
      event.dataTransfer.setData("text/plain", behaviorId);
      event.dataTransfer.effectAllowed = "move";
    },
    onDragEnd: () => { dragging = undefined; clearDrop(); },
    onDragOver: (event: React.DragEvent<HTMLElement>) => {
      if (dragging?.kind !== "behavior" || dragging.id === behaviorId) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = "move";
      const zone = zoneOf(event, false);
      if (event.currentTarget.dataset.drop !== zone) { clearDrop(); event.currentTarget.dataset.drop = zone; }
    },
    onDragLeave: (event: React.DragEvent<HTMLElement>) => { delete event.currentTarget.dataset.drop; },
    onDrop: (event: React.DragEvent<HTMLElement>) => {
      const d = dragging, zone = zoneOf(event, false);
      clearDrop();
      dragging = undefined;
      if (d?.kind !== "behavior" || d.id === behaviorId) return;
      event.preventDefault();
      event.stopPropagation();
      reorder(ws, d.id, behaviorId, zone === "after");
    },
  };
}
function OutlineOwner({
  owner,
  address,
  name,
  root = false,
}: {
  owner: Box;
  address: GraphAddress;
  name: string;
  root?: boolean;
}) {
  const ws = useWorkspace();
  const menu = useContextMenu([ws.state.project, address.behaviorId, JSON.stringify(address)], message => ws.notify(message, "error"));
  const ownerActions = () => root ? behaviorActions(ws, address.behaviorId) : boxActions(ws, owner, {...address, levels: address.levels.slice(0, -1)});
  const hasChildren = owner.timeline?.layers.some((l) => l.keyframes.length);
  const drag = root ? outlineDrag(ws, address.behaviorId) : {};
  const selection = useContext(Selection);
  const entry = (
    <button
      className="tree-row"
      {...(hasChildren ? {} : drag)}
      {...pick(selection, outlineKey(address, hasChildren ? ":open" : ""))}
      {...menu.trigger(ownerActions)}
      onClick={() =>
        root
          ? ws.navigate(address)
          : ws.navigate(
              { ...address, levels: address.levels.slice(0, -1) },
              String(owner.id),
            )
      }
    >
      <Workflow size={14} />
      <span>{name}</span>
    </button>
  );
  if (!hasChildren) return <>{entry}{menu.element}</>;
  return (
    <details className="outline-owner" open={root}>{menu.element}
      <summary {...drag} {...pick(selection, outlineKey(address))} {...menu.trigger(ownerActions)}>
        <span>{name}</span>
      </summary>
      {root && entry}
      {owner.timeline!.layers.map((layer, li) => (
        <div key={li}>
          {owner.timeline!.enabled && (
            <div className="tree-section" tabIndex={0} {...pick(selection, outlineKey(address, `@${li}`))} {...menu.trigger(event => {
              const section = event.currentTarget.parentElement!;
              return [
                {label: "Reveal in timeline", run: () => {ws.navigate({...address, levels: [...address.levels.slice(0, -1), {...address.levels.at(-1)!, layer: li, keyframe: 0}]}); ws.showPanel("timeline");}},
                {label: "Expand subtree", run: () => section.querySelectorAll("details").forEach(d => d.open = true)},
                {label: "Collapse subtree", run: () => section.querySelectorAll("details").forEach(d => d.open = false)},
              ];
            })}>{layer.name}</div>
          )}
          {layer.keyframes.map((kf, ki) => {
            const at = {
              ...address,
              levels: [
                ...address.levels.slice(0, -1),
                { ...address.levels.at(-1)!, layer: li, keyframe: ki },
              ],
            };
            return (
              <div key={ki}>
                {owner.timeline!.enabled && (
                  <button
                    className="tree-row keyframe-row"
                    {...pick(selection, outlineKey(at))}
                    {...menu.trigger(event => {
                      const section = event.currentTarget.parentElement!;
                      return [
                        {label: "Reveal in diagram", run: () => ws.navigate(at)},
                        {label: "Expand subtree", run: () => section.querySelectorAll("details").forEach(d => d.open = true)},
                        {label: "Collapse subtree", run: () => section.querySelectorAll("details").forEach(d => d.open = false)},
                      ];
                    })}
                    onClick={() => ws.navigate(at)}
                  >
                    <span className="diamond">◇</span>
                    {kf.name || `Frame ${kf.index}`}
                    <small>{kf.index}</small>
                  </button>
                )}
                {kf.diagram?.boxes.map((box) => (
                  <div className="outline-child" key={box.id}>
                    <button
                      className="tree-row"
                      onClick={() => ws.navigate(at, String(box.id))}
                      onDoubleClick={() => ws.openBox(box, at)}
                      {...pick(selection, outlineKey(at, `#${box.id}`))}
                      {...menu.trigger(() => boxActions(ws, box, at))}
                    >
                      <Workflow size={13} />
                      <span>{box.name}</span>
                    </button>
                    {box.timeline?.layers.length ? (
                      <OutlineOwner
                        owner={box}
                        address={{
                          ...at,
                          levels: [
                            ...at.levels,
                            { boxId: box.id, layer: 0, keyframe: 0 },
                          ],
                        }}
                        name="Contents"
                      />
                    ) : null}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      ))}
    </details>
  );
}
function OutlinePanel() {
  const ws = useWorkspace();
  const [query, setQuery] = useState("");
  const [selected, select] = useState<string>();
  const menu = useContextMenu([ws.state.project, query], message => ws.notify(message, "error"));
  const entries = useMemo(
    () => (ws.state.project ? outline(ws.state.project) : []),
    [ws.state.project],
  );
  return (
    <div className="panel-column">{menu.element}
      <SearchField
        label="Find behaviors and boxes"
        value={query}
        onChange={setQuery}
      />
      <div className="panel-scroll">
        {query
          ? entries
              .filter((e) => e.path.toLowerCase().includes(query.toLowerCase()))
              .map((entry, i) => (
                <button
                  className="search-result"
                  key={i}
                  onClick={() => ws.reveal(entry)}
                  {...pick({selected, select}, entry.kind === "behavior" ? outlineKey(entry.address) : outlineKey(entry.address, `#${entry.box.id}`))}
                  {...menu.trigger(() => entry.kind === "behavior" ? behaviorActions(ws, entry.address.behaviorId) : boxActions(ws, entry.box, entry.address))}
                >
                  <Workflow size={14} />
                  <span>
                    {entry.box.name}
                    <small>{entry.path}</small>
                  </span>
                </button>
              ))
          : <Selection.Provider value={{selected, select}}>{ws.state.project?.behaviors.map((b) => (
              <OutlineOwner
                key={b.id}
                owner={b.root}
                address={rootAddress(b.id, b.root.id)}
                name={b.name}
                root
              />
            ))}</Selection.Provider>}
        {!entries.length && (
          <p className="panel-message">
            The project's nested structure will appear here.
          </p>
        )}
      </div>
    </div>
  );
}
function GraphDocument({ behaviorId, api }: { behaviorId: string; api: IDockviewPanelProps["api"] }) {
  const [visible, setVisible] = useState(api.isVisible);
  useLayoutEffect(() => {
    setVisible(api.isVisible);
    const listener = api.onDidVisibilityChange((event) => setVisible(event.isVisible));
    return () => listener.dispose();
  }, [api]);
  const ws = useWorkspace();
  const address = ws.state.addresses[behaviorId];
  const resolved = resolveAddress(ws.state.project, address);
  const register = useCallback(
    (controls: import("../diagram/Diagram").GraphControls | undefined) =>
      ws.registerGraph(behaviorId, controls),
    [behaviorId, ws.registerGraph],
  );
  const stimulated = useRobot((s) => s.stimulated);
  const [activation, setActivation] = useState<{ key: string; box: Box; port: Port; path: number[][] }>();
  if (!resolved || !address || !ws.state.project) return <Welcome />;
  const key = graphId(ws.state.project.id, address);
  // Boxes the robot stimulated in the last 0.6 s, for the runtime glow.
  const folder = resolved.behavior.source.replace(/\/behavior\.xar$/, "");
  const prefix = address.levels
    .slice(1)
    .map((l) => l.boxId)
    .join("/");
  const now = Date.now();
  const glowing = new Set(
    Object.entries(stimulated)
      .filter(
        ([k, t]) => now - t < 600 && k.split("|")[0].endsWith(`/${folder}`),
      )
      .map(([k]) => k.split("|")[1])
      .filter(
        (path) =>
          path.startsWith(prefix) &&
          path.slice(prefix.length).replace(/^\//, "").split("/").length === 1,
      )
      .map((path) => path.slice(prefix.length).replace(/^\//, "")),
  );
  return (
    // React Flow's explicit node visibility overrides Dockview's inherited visibility.
    <div className="graph-document" style={{ display: visible ? undefined : "none" }}>
      {activation?.key === `${ws.state.session}:${key}` && (
        <InputActivation
          session={ws.state.project.session!}
          behavior={behaviorId}
          revision={ws.history?.revision}
          boxName={activation.box.name}
          port={activation.port}
          path={activation.path}
          onClose={() => setActivation(undefined)}
        />
      )}
      <nav className="breadcrumbs" aria-label="Diagram path">
        {resolved.owners.map((box, i) => (
          <span key={i}>
            {i > 0 && <ChevronRight size={13} />}
            <button
              title={box.name}
              onClick={() =>
                ws.navigate({
                  ...address,
                  levels: address.levels.slice(0, i + 1),
                })
              }
            >
              {i === 0 ? resolved.behavior.name : box.name}
            </button>
          </span>
        ))}
        <span className="spacer" />
        <button
          className="icon-button"
          aria-label="Search graph"
          title="Find behaviors and boxes"
          onClick={() => ws.showPanel("outline")}
        >
          <Search size={15} />
        </button>
      </nav>
      {ws.editable && resolved.diagram && (
        <div
          className="panel-toolbar diagram-actions"
          role="toolbar"
          aria-label="Diagram editing"
        >
          {(
            [
              ["insert", "New box", <Plus size={13} />],
              ["copy", "Copy", <Copy size={13} />],
              ["paste", "Paste", <ClipboardPaste size={13} />],
              ["delete", "Delete", <Trash2 size={13} />],
              ["convert-to-box", "Group into box…", <Workflow size={13} />],
            ] as const
          ).map(([id, title, icon]) => {
            const command = ws.commands.find((c) => c.id === id);
            return (
              <button
                key={id}
                disabled={!command?.enabled}
                title={!command?.enabled ? command?.reason : command.title}
                onClick={() => ws.execute(id)}
              >
                {icon}
                {title}
              </button>
            );
          })}
          <span className="spacer" />
          <button onClick={() => ws.showPanel("libraries")}>Libraries</button>
          {scriptOpenable(resolved.owner) && (
            <button onClick={() => ws.openScript(resolved.owner, address)}>
              <Code2 size={13} />
              Box script
            </button>
          )}
        </div>
      )}
      {resolved.diagram ? (
        <Diagram
          key={`${ws.state.session}:${key}`}
          graphStyle={ws.graphStyle}
          scrolling={ws.graphScrolling}
          diagram={resolved.diagram}
          owner={resolved.owner}
          selection={ws.state.selections[key] ?? emptySelection}
          viewport={ws.state.viewports[key]}
          onViewport={(viewport) => ws.saveViewport(key, viewport)}
          onSelect={(selection) => ws.select(key, selection)}
          onOpen={(box) => ws.openBox(box, address)}
          onInsertLibrary={ws.editable ? (path, position) => ws.insertLibraryBox(path, { address, position }) : undefined}
          onActivateInput={ws.editable ? (box, port) => {
            const session = ws.state.project!.session!;
            const error = inputActivationError(session, behaviorId, ws.history?.revision);
            if (error) { ws.notify(error, "error"); return; }
            const path = address.levels.slice(1).map((level, i) => [address.levels[i].layer, address.levels[i].keyframe, level.boxId]);
            if (box !== resolved.owner) path.push([resolved.top.layer, resolved.top.keyframe, box.id]);
            if (port.type === 1) {
              void robot.activateInput(session, behaviorId, ws.history?.revision, path, port.id, null).catch((e) => ws.notify(String(e), "error"));
            } else {
              setActivation({ key: `${ws.state.session}:${key}`, box, port, path });
            }
          } : undefined}
          onParameters={() => ws.showPanel("inspector")}
          onControls={register}
          onEdit={
            ws.editable && ws.state.activeBehavior === behaviorId
              ? (label, commands) => ws.edit(label, commands)
              : undefined
          }
          onCommand={ws.execute}
          commands={ws.commands}
          glowing={glowing}
        />
      ) : (
        <Empty
          title={
            resolved.owner.timeline?.enabled
              ? "Motion timeline"
              : "No flow diagram"
          }
        >
          <p>
            {resolved.owner.timeline?.enabled
              ? "Use Timeline to inspect this box's motion keys and behavior layers."
              : "This box does not contain a flow diagram."}
          </p>
          {scriptOpenable(resolved.owner) && (
            <button onClick={() => ws.openScript(resolved.owner, address)}>
              Open script
            </button>
          )}
        </Empty>
      )}
    </div>
  );
}
function TimelinePanel() {
  const ws = useWorkspace();
  const active = useActiveTimeline();
  const [properties, setProperties] = useState<{identity: string; layer?: number; keyframe?: number}>();
  const [keyframeClipboard, setKeyframeClipboard] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  if (!active)
    return (
      <Empty title="No timeline selected">
        <p>Open a timeline box to inspect its layers and keyframes.</p>
      </Empty>
    );
  const { address, resolved, key, value } = active;
  const t = active.t;
  const layer = t.layers[resolved.top.layer];
  const keyframe = layer?.keyframes[resolved.top.keyframe];
  const edit = ws.editable && t.node !== undefined;
  let nextLayer = t.layers.length + 1;
  while (t.layers.some((l) => l.name === `behavior_layer${nextLayer}`))
    nextLayer++;
  let nextKeyframe = value.frame;
  while (layer?.keyframes.some((k) => k.name === `keyframe${nextKeyframe}`))
    nextKeyframe++;
  let lastFrame = 1;
  for (const l of t.layers)
    for (const k of l.keyframes) lastFrame = Math.max(lastFrame, k.index);
  for (const c of t.curves)
    for (const frame of c.frames) lastFrame = Math.max(lastFrame, frame);
  const timelineEdit = edit && (
    <div
      className="panel-toolbar timeline-edit"
      role="toolbar"
      aria-label="Timeline editing"
    >
      <button
        onClick={() =>
          void ws.edit("Add new layer", [
            {
              type: "insertXml",
              parent: t.node!,
              xml: `<BehaviorLayer name="behavior_layer${nextLayer}"><BehaviorKeyframe name="keyframe1" index="1"><Diagram /></BehaviorKeyframe></BehaviorLayer>`,
            },
          ])
        }
      >
        <Plus size={13} />
        Layer
      </button>
      <button
        disabled={
          layer?.node === undefined ||
          layer.keyframes.some((k) => k.index === value.frame)
        }
        title={`Insert a keyframe at frame ${value.frame}`}
        onClick={() =>
          void ws.edit(`Add keyframe${value.frame}`, [
            {
              type: "insertXml",
              parent: layer!.node!,
              xml: `<BehaviorKeyframe name="keyframe${nextKeyframe}" index="${value.frame}"><Diagram /></BehaviorKeyframe>`,
            },
          ])
        }
      >
        <Plus size={13} />
        Keyframe at {value.frame}
      </button>
      <button
        disabled={
          keyframe?.node === undefined || (layer?.keyframes.length ?? 0) < 2
        }
        title="Delete the open keyframe (a layer keeps at least one)"
        onClick={() =>
          void ws.edit(`Delete ${keyframe!.name}`, [
            { type: "remove", nodes: [keyframe!.node!] },
          ])
        }
      >
        Delete keyframe
      </button>
      <button
        disabled={layer?.node === undefined || t.layers.length < 2}
        title="Delete the open layer and its boxes; Undo restores them"
        onClick={() =>
          void ws.edit(`Delete layer ${layer!.name}`, [
            { type: "remove", nodes: [layer!.node!] },
          ])
        }
      >
        Delete layer
      </button>
      <button
        onClick={() => {
          setError("");
          setProperties({identity: `${ws.state.session}:${key}:${ws.history?.revision}`, layer: layer?.node, keyframe: keyframe?.node});
        }}
      >
        Timeline properties…
      </button>
      <button disabled={keyframe?.node === undefined} onClick={() => {
        void authoring.copy(ws.state.project!.session!, address.behaviorId, [keyframe!.node!])
          .then(setKeyframeClipboard).catch(e => ws.notify(String(e), "error"));
      }}>Copy keyframe</button>
      <button disabled={!keyframeClipboard || layer?.node === undefined || layer.keyframes.some(k => k.index === value.frame)} title={!keyframeClipboard ? "Copy a keyframe first." : "Choose an unoccupied frame."} onClick={() => void ws.edit("Paste keyframe", [
        {type: "pasteKeyframes", layer: layer!.node!, xml: keyframeClipboard, index: value.frame},
      ])}>Paste keyframe</button>
      <label className="checkbox"><input type="checkbox" checked={!!layer?.mute} disabled={layer?.node === undefined}
        onChange={e => void ws.edit("Mute layer", [{type: "setAttributes", node: layer!.node!, values: {mute: e.target.checked ? "1" : "0"}}])} />Mute layer</label>
      <button
        title="Edit motion keys, tangents and joints"
        onClick={() => ws.showPanel("curves")}
      >
        Motion curves
      </button>
      <button onClick={() => ws.showPanel("recording")}>Record motion</button>
      <span className="spacer" />
      <label>
        FPS{" "}
        <input
          type="number"
          min={1}
          max={100}
          aria-label="Frames per second"
          defaultValue={t.fps}
          key={t.fps}
          onBlur={(e) => {
            const fps = Number(e.target.value);
            if (
              Number.isInteger(fps) &&
              fps >= 1 &&
              fps <= 100 &&
              fps !== t.fps
            )
              void ws.edit("Edit timeline", [
                {
                  type: "setAttributes",
                  node: t.node!,
                  values: { fps: String(fps) },
                },
              ]);
            else e.currentTarget.value = String(t.fps);
          }}
        />
      </label>
    </div>
  );
  const menuActions = (target: TimelineMenuTarget): MenuAction[] => {
    const targetLayer = target.layer === undefined ? undefined : t.layers[target.layer];
    const targetKey = target.keyframe === undefined ? undefined : targetLayer?.keyframes[target.keyframe];
    const write = (label: string, commands: Parameters<typeof ws.edit>[1]) => ws.edit(label, commands, address.behaviorId);
    const reason = !edit ? "Open an editable project to change the timeline." : undefined;
    const propertiesAction: MenuAction = {label: targetKey ? "Keyframe properties…" : targetLayer ? "Layer properties…" : "Timeline properties…",
      disabled: !edit, reason, run: () => {setError(""); setProperties({identity: `${ws.state.session}:${key}:${ws.history?.revision}`, layer: targetLayer?.node, keyframe: targetKey?.node});}};
    if (targetKey) return [
      {label: "Open diagram", run: () => ws.navigate({...address, levels: [...address.levels.slice(0, -1), {...address.levels.at(-1)!, layer: target.layer!, keyframe: target.keyframe!}]})},
      {label: "Copy keyframe", disabled: !edit || targetKey.node === undefined, reason, run: () => authoring.copy(ws.state.project!.session!, address.behaviorId, [targetKey.node!]).then(setKeyframeClipboard)},
      propertiesAction,
      {label: "Delete keyframe", disabled: !edit || targetKey.node === undefined || targetLayer!.keyframes.length < 2,
        reason: reason ?? "A layer must keep at least one keyframe.", run: () => write("Delete keyframe", [{type: "remove", nodes: [targetKey.node!]}])},
    ];
    if (targetLayer) return [
      {label: `Move playhead to frame ${target.frame}`, run: () => ws.saveTimeline(key, {...value, frame: target.frame})},
      {label: `New keyframe at frame ${target.frame}`, disabled: !edit || targetLayer.node === undefined || targetLayer.keyframes.some(k => k.index === target.frame), reason: reason ?? "A keyframe already exists at this frame.", run: () => {
        let name = `keyframe${target.frame}`, suffix = 2;
        while (targetLayer.keyframes.some(k => k.name === name)) name = `keyframe${target.frame}_${suffix++}`;
        return write("Add keyframe", [{type: "insertXml", parent: targetLayer.node!, xml: `<BehaviorKeyframe name="${name}" index="${target.frame}"><Diagram /></BehaviorKeyframe>`}]);
      }},
      {label: `Paste keyframe at frame ${target.frame}`, disabled: !edit || targetLayer.node === undefined || !keyframeClipboard || targetLayer.keyframes.some(k => k.index === target.frame), reason: reason ?? (!keyframeClipboard ? "Copy a keyframe first." : "Choose an unoccupied frame."), run: () => write("Paste keyframe", [{type: "pasteKeyframes", layer: targetLayer.node!, xml: keyframeClipboard, index: target.frame}])},
      {label: targetLayer.mute ? "Unmute layer" : "Mute layer", checked: !!targetLayer.mute, disabled: !edit || targetLayer.node === undefined, reason, run: () => write("Mute layer", [{type: "setAttributes", node: targetLayer.node!, values: {mute: targetLayer.mute ? "0" : "1"}}])},
      propertiesAction,
      {label: "Delete layer", disabled: !edit || targetLayer.node === undefined || t.layers.length < 2, reason: reason ?? "A timeline must keep at least one layer.", run: () => write("Delete layer", [{type: "remove", nodes: [targetLayer.node!]}])},
    ];
    return [
      {label: "New layer", disabled: !edit, reason, run: () => write("Add new layer", [{type: "insertXml", parent: t.node!, xml: `<BehaviorLayer name="behavior_layer${nextLayer}"><BehaviorKeyframe name="keyframe1" index="1"><Diagram /></BehaviorKeyframe></BehaviorLayer>`}])},
      propertiesAction,
      {label: "Motion curves", run: () => ws.showPanel("curves")},
      {label: "Record motion", run: () => ws.showPanel("recording")},
    ];
  };
  const propertiesLayer = properties ? t.layers.find(l => l.node === properties.layer) : undefined;
  const propertiesKeyframe = propertiesLayer?.keyframes.find(k => k.node === properties?.keyframe);
  return (
    <div className="panel-column">
      {timelineEdit}
      <Timeline
        menuIdentity={`${ws.state.session}:${key}:${ws.history?.revision}`}
        menuActions={menuActions}
        onMenuError={message => ws.notify(message, "error")}
        robotFrame={active.robotFrame}
        t={t}
        onMoveKeyframe={edit ? async (node, delta) => {
          const result = await ws.edit("Move keyframe", [{type: "translateKeyframes", keyframes: [node], delta}], address.behaviorId);
          const updated = result && resolveAddress(result.document, address)?.owner.timeline;
          const li = updated ? updated.layers.findIndex(l => l.keyframes.some(k => k.node === node)) : -1;
          if (updated && li >= 0) {
            const ki = updated.layers[li].keyframes.findIndex(k => k.node === node);
            ws.saveTimeline(key, {...value, frame: updated.layers[li].keyframes[ki].index});
            ws.navigate({...address, levels: [...address.levels.slice(0, -1), {...address.levels.at(-1)!, layer: li, keyframe: ki}]});
          }
        } : undefined}
        active={resolved.top}
        value={value}
        onChange={(value) => ws.saveTimeline(key, value)}
        onPick={(layer, keyframe, frame) => {
          ws.saveTimeline(key, {
            ...value,
            frame:
              frame ??
              resolved.owner.timeline!.layers[layer]?.keyframes[keyframe]
                ?.index ??
              1,
          });
          ws.navigate({
            ...address,
            levels: [
              ...address.levels.slice(0, -1),
              { ...address.levels.at(-1)!, layer, keyframe },
            ],
          });
        }}
      />
      {properties?.identity === `${ws.state.session}:${key}:${ws.history?.revision}` && (
        <Modal
          title="Timeline properties"
          onClose={() => !saving && setProperties(undefined)}
        >
          <form
            className="settings-content"
            key={`${key}:${propertiesLayer?.node}:${propertiesKeyframe?.node}`}
            onSubmit={async (e) => {
              e.preventDefault();
              if (saving) return;
              const data = new FormData(e.currentTarget);
              const commands: import("./authoring").EditCommand[] = [];
              const size = Math.max(
                Number(data.get("size")),
                Number(data.get("frame") ?? 1),
              );
              if (size !== t.size)
                commands.push({
                  type: "setAttributes",
                  node: t.node!,
                  values: { size: String(size) },
                });
              if (propertiesLayer?.node !== undefined && data.get("layer") !== propertiesLayer.name)
                commands.push({
                  type: "setAttributes",
                  node: propertiesLayer.node,
                  values: { name: String(data.get("layer")).trim() },
                });
              if (
                propertiesKeyframe?.node !== undefined &&
                (data.get("keyframe") !== propertiesKeyframe.name ||
                  Number(data.get("frame")) !== propertiesKeyframe.index)
              )
                commands.push({
                  type: "setAttributes",
                  node: propertiesKeyframe.node,
                  values: {
                    name: String(data.get("keyframe")).trim(),
                    index: String(data.get("frame")),
                  },
                });
              const start = Number(data.get("rangeStart"));
              const end = Number(data.get("rangeEnd"));
              if (start !== (t.startFrame ?? 1) || end !== (t.endFrame ?? -1)) commands.push({
                type: data.get("stretch") ? "stretchTimelineRange" : "setTimelineRange", timeline: t.node!, start, end,
              });
              const acquisition = String(data.get("acquisition"));
              if (acquisition !== String(t.resourcesAcquisition ?? 0)) commands.push({type: "setAttributes", node: t.node!, values: {resources_acquisition: acquisition}});
              if (!commands.length) {
                setProperties(undefined);
                return;
              }
              setSaving(true);
              const result = await ws.edit(
                "Edit timeline properties",
                commands,
                address.behaviorId,
              );
              setSaving(false);
              if (!result) {
                setError(
                  "Could not apply the changes. Names and frame positions must be unique within their layer or timeline.",
                );
                return;
              }
              // Moving a keyframe sorts it; follow its node identity, not the old array index.
              const updated = resolveAddress(result.document, address)?.owner
                .timeline;
              const li =
                updated?.layers.findIndex((l) => l.node === propertiesLayer?.node) ?? -1;
              const ki =
                li < 0
                  ? -1
                  : updated!.layers[li].keyframes.findIndex(
                      (k) => k.node === propertiesKeyframe?.node,
                    );
              if (li >= 0 && ki >= 0)
                ws.navigate({
                  ...address,
                  levels: [
                    ...address.levels.slice(0, -1),
                    { ...address.levels.at(-1)!, layer: li, keyframe: ki },
                  ],
                });
              setProperties(undefined);
            }}
          >
            <label className="setting">
              Timeline length (frames)
              <input
                name="size"
                type="number"
                min={t.enabled ? lastFrame : 0}
                step={1}
                required
                defaultValue={Math.max(t.size, t.enabled ? lastFrame : 0)}
              />
            </label>
            <label className="setting">Playback start<input name="rangeStart" type="number" min={1} step={1} required defaultValue={t.startFrame ?? 1} /></label>
            <label className="setting">Playback end (−1 = timeline end)<input name="rangeEnd" type="number" min={-1} step={1} required defaultValue={t.endFrame ?? -1} /></label>
            <label className="checkbox"><input name="stretch" type="checkbox" />Stretch keys to the new range</label>
            <label className="setting">Resource acquisition<select name="acquisition" defaultValue={t.resourcesAcquisition ?? 0}>
              {["Passive", "Waiting", "Aggressive"].map((name, i) => <option key={name} value={i}>{name}</option>)}
            </select></label>
            {propertiesLayer && (
              <label className="setting">
                Layer name
                <input name="layer" required defaultValue={propertiesLayer.name} />
              </label>
            )}
            {propertiesKeyframe && (
              <>
                <label className="setting">
                  Keyframe name
                  <input
                    name="keyframe"
                    required
                    defaultValue={propertiesKeyframe.name}
                  />
                </label>
                <label className="setting">
                  Keyframe position
                  <input
                    name="frame"
                    type="number"
                    min={1}
                    step={1}
                    required
                    defaultValue={propertiesKeyframe.index}
                  />
                </label>
              </>
            )}
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <button type="submit" disabled={saving}>
              {saving ? "Applying…" : "Apply"}
            </button>
          </form>
        </Modal>
      )}
    </div>
  );
}
function ProblemsPanel() {
  const ws = useWorkspace();
  const menu = useContextMenu([ws.diagnostics], message => ws.notify(message, "error"));
  return ws.diagnostics.length ? (
    <div className="problems panel-scroll">{menu.element}
      {ws.diagnostics.map((d) => (
        <div className="problem" key={d.id} tabIndex={0} {...menu.trigger(() => [
          {label: "Copy message", run: () => copyText(d.message)},
          ...(d.path ? [{label: "Copy path", run: () => copyText(d.path!)}] : []),
        ])}>
          <AlertTriangle size={15} />
          <div>
            <strong>{d.message}</strong>
            {d.path && <small>{d.path}</small>}
          </div>
          <span className="badge">{d.severity}</span>
        </div>
      ))}
    </div>
  ) : (
    <Empty title="No problems reported" icon={<SlidersHorizontal size={24} />}>
      <p>Project loading and link diagnostics appear here.</p>
    </Empty>
  );
}
/** The box with `snapshot`'s node in the current document, so script tabs follow Undo/Redo. */
function currentBox(
  project: ProjectDocument | undefined,
  behaviorId: string | undefined,
  snapshot: Box,
): Box {
  const behavior = project?.behaviors.find((b) => b.id === behaviorId);
  if (!behavior || snapshot.node === undefined) return snapshot;
  const find = (box: Box): Box | undefined =>
    box.node === snapshot.node
      ? box
      : box.timeline?.layers
          .flatMap((l) => l.keyframes)
          .flatMap((k) => k.diagram?.boxes ?? [])
          .reduce<Box | undefined>(
            (found, child) => found ?? find(child),
            undefined,
          );
  return find(behavior.root) ?? snapshot;
}

export function WorkspacePanel({
  params,
  api,
}: IDockviewPanelProps<WorkspacePanelParams>) {
  const ws = useWorkspace();
  switch (params.kind) {
    case "project":
      return <ProjectPanel />;
    case "outline":
      return <OutlinePanel />;
    case "libraries":
      return <LibrariesPanel />;
    case "services": return <ServicesPanel />;
    case "camera": return <CameraPanel />;
    case "applications": return <ApplicationsPanel />;
    case "active-content": return <ActiveContentPanel />;
    case "resource": return <ResourcePanel />;
    case "history": return <HistoryPanel />;
    case "inspector":
      if (ws.inspectedFile) return <ResourcePanel />;
      return (
        <Inspector
          boxes={ws.inspectedBoxes}
          edges={ws.inspectedEdges}
          file={ws.inspectedFile}
          project={ws.state.project}
          behavior={ws.state.project?.behaviors.find(b => b.id === ws.state.activeBehavior)?.source}
          onEdit={
            ws.inspectorEditable
              ? (label, commands) => ws.edit(label, commands)
              : undefined
          }
          onParameterEdit={ws.inspectorEditable ? async (p, value) => {
            const session = ws.state.project!.session!;
            const behavior = ws.state.activeBehavior!;
            const revision = ws.history?.revision;
            const played = getRobot().played;
            const live = getRobot().autoUpdateParameters && !inputActivationError(session, behavior, revision);
            const address = ws.state.addresses[behavior];
            const resolved = resolveAddress(ws.state.project, address);
            const box = ws.inspectedBoxes[0];
            const path = address.levels.slice(1).map((level, i) => [address.levels[i].layer, address.levels[i].keyframe, level.boxId]);
            if (box.node !== resolved?.owner.node) path.push([resolved!.top.layer, resolved!.top.keyframe, box.id]);
            const result = await ws.edit(`Set parameter: ${p.name}`, [{ type: "setAttributes", node: p.node!, values: { value } }], behavior);
            if (live && result && getRobot().played === played) {
              try { await robot.setParameters(session, behavior, path, [[p.name, parameterValue(p.contentType, value)]], revision, result.history.revision); }
              catch (e) { ws.notify(String(e), "error"); }
            }
          } : undefined}
          onImportResource={ws.inspectorEditable && native ? async () => {
            const picked = await openDialog({ title: "Choose a file for this parameter", multiple: false });
            if (typeof picked !== "string") return;
            const project = ws.state.project!;
            const source = project.behaviors.find(b => b.id === ws.state.activeBehavior)?.source ?? "";
            // Imported next to the behavior, like a library box's attached files.
            const folder = source.includes("/") ? source.slice(0, source.lastIndexOf("/")) : "";
            const name = picked.split(/[\\/]/).pop()!;
            const target = folder ? `${folder}/${name}` : name;
            if (project.files.some(f => f.path === target)) return target;
            // The target was checked above, so this import never reports a conflict.
            const result = await ws.applyChange(async () => {
              const r = await authoring.importFiles(project.session!, [picked], folder, false);
              return "conflicts" in r ? undefined : r;
            });
            return result ? target : undefined;
          } : undefined}
          onScript={(box) =>
            ws.openScript(
              box,
              ws.state.activeBehavior
                ? ws.state.addresses[ws.state.activeBehavior]
                : undefined,
            )
          }
        />
      );
    case "timeline":
      return <TimelinePanel />;
    case "curves":
      return <MotionPanel />;
    case "robot-dialog":
      return <RobotDialogPanel />;
    case "dialog":
      return <DialogPanel />;
    case "translations":
      return <TranslationsPanel />;
    case "robot":
      return <RobotPanel />;
    case "logs":
      return <LogsPanel />;
    case "poses":
      return <PosesPanel />;
    case "memory":
      return <MemoryPanel />;
    case "recording":
      return <RecordingPanel />;
    case "files":
      return <FilesPanel />;
    case "problems":
      return <ProblemsPanel />;
    case "graph":
      return <GraphDocument behaviorId={params.behaviorId!} api={api} />;
    case "script": {
      const script = ws.scripts[params.scriptId!];
      if (!script) return <Welcome />;
      const box = currentBox(
        ws.state.project,
        script.address?.behaviorId,
        script.box,
      );
      return (
        <Suspense fallback={<Empty title="Opening script…" />}>
          <ScriptViewer
            box={box}
            initialDraft={ws.draftOf(params.scriptId!)}
            onDraft={(text) => ws.setDraft(params.scriptId!, text)}
            onSave={
              ws.editable && script.address && box.node !== undefined
                ? (text) =>
                    void ws.edit(
                      "Change box script",
                      [{ type: "setScript", box: box.node!, text }],
                      script.address!.behaviorId,
                    )
                : undefined
            }
          />
        </Suspense>
      );
    }
    case "welcome":
      return <Welcome />;
    default: {
      const panel = panels.find((p) => p.id === params.kind);
      return (
        <Empty title={panel?.title ?? "Tool unavailable"}>
          <p>
            {panel?.reason ?? "This tool is not available in this version."}
          </p>
          <span className="badge">Planned capability</span>
        </Empty>
      );
    }
  }
}
