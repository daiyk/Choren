import { useContextMenu } from "./workspace/ContextMenu";
import type { CommandTarget } from "./workspace/context";
import { Layouts } from "./workspace/Layouts";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  DockviewReact,
  DockviewDefaultTab,
  type IDockviewPanelHeaderProps,
  type DockviewApi,
  type IDockviewHeaderActionsProps,
  type DockviewReadyEvent,
} from "dockview-react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Circle,
  FolderOpen,
  LayoutDashboard,
  LoaderCircle,
  Maximize2,
  MoreHorizontal,
  Play,
  Redo2,
  Save,
  Search,
  Settings,
  Square,
  Undo2,
  Workflow,
  X,
} from "lucide-react";
import type {
  Box,
  Diagnostic,
  ProjectDocument,
  ProjectFile,
} from "./format/types";
import { resolveLink } from "./format/xar";
import { behaviorRelative } from "./format/xml";
import type { GraphControls } from "./diagram/Diagram";
import {
  chooseLibrary,
  chooseProject,
  mac,
  mod,
  native,
  releaseProject,
} from "./platform";
import { getShareDir, setShareDir, useShareDir } from "./icons";
import {
  WorkspaceContext,
  useWorkspace,
  type ScriptDocument,
  type WorkspaceContextValue,
} from "./workspace/context";
import {
  emptySelection,
  graphId,
  initialWorkspace,
  OpenRequests,
  outline,
  resolveAddress,
  scriptOpenable,
  workspaceReducer,
  type GraphAddress,
  type Selection,
  type TimelineView,
  type Viewport,
} from "./workspace/model";
import { panels, unavailableActions, type Command } from "./workspace/registry";
import {
  WorkspacePanel,
  Welcome,
  type WorkspacePanelParams,
} from "./workspace/panels";
import {
  LAYOUT_KEY,
  THEME_KEY,
  GRAPH_STYLE_KEY,
  TEXT_SCALE_KEY,
  WELCOME_KEY,
  readPreference,
  savePreference,
  validLayout,
  type ColorMode,
  type GraphStyle,
} from "./workspace/storage";
import { exampleProject } from "./workspace/example";
import {
  installNativeMenu,
  type NativeMenuHandle,
} from "./workspace/nativeMenu";
import { Modal } from "./workspace/Modal";
import {
  authoring,
  editable as canEdit,
  type Change,
  type EditCommand,
  type FilePolicy,
  type HistoryState,
  type Manifest,
  type NewBoxKind,
} from "./workspace/authoring";
import {
  open as pickFolder,
  save as saveFile,
} from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { robot, startRobotEvents, useRobot } from "./robot/store";
import { ProjectProperties } from "./workspace/ProjectProperties";
import { WelcomeSetup } from "./workspace/WelcomeSetup";
import { AboutDialog } from "./workspace/AboutDialog";
import { Connection } from "./robot/Connection";
import { VirtualMovementSpeed } from "./robot/panels";
import { ImportContent, type ImportKind } from "./workspace/ImportContent";
import { RemoveContent, type RemovalTarget } from "./workspace/RemoveContent";
import { RenameContent, type RenameTarget } from "./workspace/RenameContent";
import { UpdateDialog } from "./workspace/UpdateDialog";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { flushSync } from "react-dom";

const components = { workspace: WorkspacePanel };
const studioTheme = { name: "choren", className: "dockview-theme-choren" };
const documentKinds = new Set(["welcome", "graph", "script"]);
const centerPanel = (api: DockviewApi) =>
  api.panels.find((p) => documentKinds.has(p.params?.kind));
function defaultLayout(api: DockviewApi) {
  api.clear();
  const center = api.addPanel({
    id: "welcome",
    component: "workspace",
    title: "Welcome",
    params: { kind: "welcome" },
    minimumWidth: 320,
    minimumHeight: 220,
    renderer: "always",
  });
  const project = api.addPanel({
    id: "project",
    component: "workspace",
    title: "Project",
    params: { kind: "project" },
    position: { referencePanel: center.id, direction: "left" },
    initialWidth: 248,
    minimumWidth: 180,
  });
  api.addPanel({
    id: "outline",
    component: "workspace",
    title: "Outline",
    params: { kind: "outline" },
    position: { referencePanel: project.id, direction: "within" },
    inactive: true,
  });
  api.addPanel({
    id: "libraries",
    component: "workspace",
    title: "Libraries",
    params: { kind: "libraries" },
    position: { referencePanel: project.id, direction: "below" },
    initialHeight: 265,
    minimumHeight: 140,
    inactive: true,
  });
  api.addPanel({
    id: "inspector",
    component: "workspace",
    title: "Inspector",
    params: { kind: "inspector" },
    position: { referencePanel: center.id, direction: "right" },
    initialWidth: 304,
    minimumWidth: 240,
    inactive: true,
  });
  center.api.setActive();
}
function PanelTab(props: IDockviewPanelHeaderProps) {
  const ws = useWorkspace();
  const menu = useContextMenu([props.api.id, ws.state.activeBehavior, ws.state.project], message => ws.notify(message, "error"));
  // The React default tab names every close button "Close tab"; name it after the panel, as
  // dockview's own tab does, so each one is distinct ("Close Inspector").
  const tab = useRef<HTMLDivElement>(null);
  const [title, setTitle] = useState(props.api.title);
  useEffect(() => {
    const listener = props.api.onDidTitleChange(event => setTitle(event.title));
    return () => listener.dispose();
  }, [props.api]);
  useLayoutEffect(() => {
    tab.current?.querySelector(".dv-default-tab-action")?.setAttribute("aria-label", `Close ${title ?? props.api.id}`);
  });
  // React 19 passes `ref` through to the tab's root element.
  return <><DockviewDefaultTab {...props} {...{ref: tab}} tabIndex={0} {...menu.trigger(() => [
    {label: props.api.isMaximized() ? "Restore panel" : "Maximize panel", run: () => props.api.isMaximized() ? props.api.exitMaximized() : props.api.maximize()},
    {label: "Move or resize…", run: () => ws.managePanel(props.api.id)},
    {label: "Close panel", run: () => props.api.close()},
  ])} />{menu.element}</>;
}
function HeaderActions({ activePanel }: IDockviewHeaderActionsProps) {
  const ws = useWorkspace();
  const menu = useContextMenu([activePanel?.id, ws.state.activeBehavior], message => ws.notify(message, "error"));
  if (!activePanel) return null;
  return (
    <div className="panel-actions" tabIndex={0} {...menu.trigger(() => [
      {label: activePanel.api.isMaximized() ? "Restore panel" : "Maximize panel", run: () => activePanel.api.isMaximized() ? activePanel.api.exitMaximized() : activePanel.api.maximize()},
      {label: "Move or resize…", run: () => ws.managePanel(activePanel.id)},
      {label: "Close panel", run: () => activePanel.api.close()},
    ])}>{menu.element}
      <button
        className="icon-button"
        aria-label={`Maximize ${activePanel.title}`}
        title="Maximize / restore panel"
        onClick={() =>
          activePanel.api.isMaximized()
            ? activePanel.api.exitMaximized()
            : activePanel.api.maximize()
        }
      >
        <Maximize2 size={12} />
      </button>
      <button
        className="icon-button"
        aria-label={`Actions for ${activePanel.title}`}
        title="Move, resize or close panel"
        onClick={() => ws.managePanel(activePanel.id)}
      >
        <MoreHorizontal size={15} />
      </button>
    </div>
  );
}

export default function App() {
  const [state, dispatch] = useReducer(workspaceReducer, initialWorkspace);
  const stateRef = useRef(state);
  stateRef.current = state;
  const [dock, setDock] = useState<DockviewApi>();
  const dockRef = useRef<DockviewApi | undefined>(undefined);
  const [scripts, setScripts] = useState<Record<string, ScriptDocument>>({});
  const [focused, setFocused] = useState<{ kind: string; scriptId?: string }>({
    kind: "welcome",
  });
  const [inspection, setInspection] = useState<{
    box?: Box;
    file?: ProjectFile;
    identity?: string;
  }>();
  const [loading, setLoading] = useState(false);
  const [notification, setNotification] = useState<{ message: string; kind: "info" | "error" }>();
  const notice = notification?.message ?? "";
  const noticeIsError = notification?.kind === "error";
  const setNotice = useCallback((message: string, kind: "info" | "error" = "info") => {
    setNotification(message ? { message, kind } : undefined);
    if (message && kind === "error") robot.log("error", message);
  }, []);
  // The desktop app greets a first launch until Choregraphe is linked or the window is closed.
  const [modal, setModal] = useState<
    | "commands"
    | "settings"
    | "panel"
    | "new-box"
    | "properties"
    | "convert"
    | "new-dialog"
    | "new-behavior"
    | "connection"
    | "import"
    | "remove"
    | "rename"
    | "welcome"
    | "about"
    | undefined
  >(() => (native && !getShareDir() && readPreference(WELCOME_KEY) !== "done" ? "welcome" : undefined));
  const [importKind, setImportKind] = useState<ImportKind>("behavior");
  const [importDestination, setImportDestination] = useState("");
  const [propertiesBehavior, setPropertiesBehavior] = useState<string>();
  const [removal, setRemoval] = useState<{target: RemovalTarget; warning?: string}>();
  const [renaming, setRenaming] = useState<RenameTarget>();
  const [groupTarget, setGroupTarget] = useState<{project: ProjectDocument; address: GraphAddress; nodes: number[]}>();
  const [convertName, setConvertName] = useState("Box");
  const [behaviorForm, setBehaviorForm] = useState<{
    name: string;
    root: "Diagram" | "Timeline";
  }>({ name: "", root: "Diagram" });
  const [creatingBehavior, setCreatingBehavior] = useState(false);
  const [dialogForm, setDialogForm] = useState<{
    name: string;
    languages: string[];
    available: string[];
  }>({ name: "", languages: [], available: [] });
  const [history, setHistory] = useState<HistoryState>();
  const [newBox, setNewBox] = useState<{ kind: NewBoxKind; name: string; dialogFile?: string }>({
    kind: "Python",
    name: "My box",
  });
  const [manifest, setManifest] = useState<Manifest>();
  const [conflict, setConflict] = useState<{
    paths: string[];
    retry: (policy: FilePolicy) => void;
  }>();
  const [clipboard, setClipboard] = useState("");
  // Unsaved work guard: document changes plus script tabs with unapplied text.
  const drafts = useRef(new Map<string, string>());
  const draftListeners = useRef(new Set<() => void>());
  const [unsaved, setUnsaved] = useState<{
    drafts: number;
    /** Only the drafts are at stake (Save as folder keeps the document but not draft keys). */
    draftsOnly?: boolean;
    resolve: (proceed: boolean) => void;
  }>();
  const historyRef = useRef(history);
  historyRef.current = history;
  /** Resolves true when the current work may be replaced (saved, discarded or clean). */
  const confirmReplace = useCallback(
    () =>
      historyRef.current?.dirty || drafts.current.size
        ? new Promise<boolean>((resolve) =>
            setUnsaved({ drafts: drafts.current.size, resolve }),
          )
        : Promise.resolve(true),
    [],
  );
  const [commandQuery, setCommandQuery] = useState("");
  const [commandGroup, setCommandGroup] = useState<string>();
  const [panelTarget, setPanelTarget] = useState("");
  const [destination, setDestination] = useState("");
  const [placement, setPlacement] = useState<
    "center" | "left" | "right" | "top" | "bottom"
  >("center");
  const [sizeAnnouncement, setSizeAnnouncement] = useState("");
  const [colorMode, setColorMode] = useState<ColorMode>(() => {
    const saved = readPreference(THEME_KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  });
  const [graphStyle, setGraphStyle] = useState<GraphStyle>(() =>
    ["classic", "choregraphe"].includes(readPreference(GRAPH_STYLE_KEY) as string) ? "classic" : "choren",
  );
  useEffect(() => savePreference(GRAPH_STYLE_KEY, graphStyle), [graphStyle]);
  const [textScale, setTextScale] = useState(() => Number(readPreference(TEXT_SCALE_KEY)) || 1);
  useEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(textScale));
    savePreference(TEXT_SCALE_KEY, textScale);
  }, [textScale]);
  const shareDir = useShareDir();
  const autoUpdateParameters = useRobot(s => s.autoUpdateParameters);
  const robotStatus = useRobot((s) => s.status);
  const robotBusy = useRobot((s) => s.busy);
  const robotRunning = useRobot((s) => s.running.length > 0);
  const robotStarting = useRobot((s) => s.starting);
  useEffect(startRobotEvents, []);
  useEffect(() => {
    if (!robotStatus) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      await robot.refreshRunning().catch(() => {});
      if (live) timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [robotStatus]);
  const [update, setUpdate] = useState<Update>();
  /** Looks for a newer release; only a manual check reports "up to date" or errors. */
  const checkForUpdates = useCallback(async (manual: boolean) => {
    try {
      const found = await check();
      if (found) setUpdate(found);
      else if (manual) setNotice("Choren is up to date.");
    } catch (e) {
      if (manual) setNotice(`Cannot check for updates: ${e}`, "error");
    }
  }, [setNotice]);
  useEffect(() => {
    if (native && import.meta.env.PROD) void checkForUpdates(false);
  }, [checkForUpdates]);
  // Closing the window goes through the same guard (Tauri closes once the handler allows it).
  useEffect(() => {
    if (!native) return;
    let unlisten: (() => void) | undefined;
    try {
      getCurrentWindow()
        .onCloseRequested(async (e) => {
          if (!(await confirmReplace())) e.preventDefault();
        })
        .then(
          (f) => (unlisten = f),
          () => {},
        );
    } catch {
      // No window bridge (tests); nothing to guard.
    }
    return () => unlisten?.();
  }, [confirmReplace]);
  const openRequests = useRef(new OpenRequests());
  const graphControls = useRef(new Map<string, GraphControls>());
  const commandRef = useRef<Command[]>([]);
  const menuRef = useRef<NativeMenuHandle | undefined>(undefined);
  const previousTimeline = useRef("");
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => {
      document.documentElement.dataset.theme =
        colorMode === "system" ? (media.matches ? "dark" : "light") : colorMode;
    };
    update();
    media.addEventListener("change", update);
    savePreference(THEME_KEY, colorMode);
    return () => media.removeEventListener("change", update);
  }, [colorMode]);
  useEffect(() => {
    for (const panel of dockRef.current?.panels ?? []) {
      if (panel.params?.kind !== "graph") continue;
      const behavior = state.project?.behaviors.find(b => b.id === panel.params?.behaviorId);
      if (behavior && panel.title !== behavior.name) panel.api.setTitle(behavior.name);
    }
  }, [state.project]);
  const targetedCommand = useRef<(id: string, target: CommandTarget) => void>(() => {});
  const execute = useCallback((id: string, target?: CommandTarget) => {
    if (target) { targetedCommand.current(id, target); return; }
    const command = commandRef.current.find((c) => c.id === id);
    if (command?.enabled) command.run();
  }, []);
  const showPanel = useCallback((id: string) => {
    const api = dockRef.current;
    if (!api) return;
    const existing = api.getPanel(id);
    if (existing) {
      existing.api.setActive();
      return;
    }
    const definition = panels.find((p) => p.id === id);
    if (!definition) return;
    const center = centerPanel(api);
    const peer = api.panels.find(
      (p) =>
        panels.find((d) => d.id === p.params?.kind)?.region ===
        definition.region,
    );
    const reference = peer ?? center;
    const direction = peer
      ? "within"
      : definition.region === "left"
        ? "left"
        : definition.region === "right"
          ? "right"
          : definition.region === "bottom"
            ? "below"
            : "within";
    api.addPanel({
      id,
      title: definition.title,
      component: "workspace",
      params: { kind: id },
      position: reference
        ? { referencePanel: reference.id, direction }
        : undefined,
      // A document joins the center group; a width would shrink that whole group.
      initialWidth:
        !peer && definition.region === "right"
          ? 304
          : !peer && definition.region === "left"
            ? 248
            : undefined,
      initialHeight: !peer && definition.region === "bottom" ? 230 : undefined,
      // The timeline's toolbars take ~120 px; keep the ruler and a layer row visible when the
      // window shrinks to its 640 px minimum.
      minimumHeight: id === "timeline" ? 190 : 100,
      minimumWidth: 180,
      // Documents share the center group with graphs, which use the "always" renderer.
      renderer: definition.region === "center" ? "always" : undefined,
    });
  }, []);
  const openBehavior = useCallback((id: string) => {
    const api = dockRef.current;
    const behavior = stateRef.current.project?.behaviors.find(
      (b) => b.id === id,
    );
    if (!api || !behavior) return;
    setInspection(undefined);
    dispatch({ type: "activate", behaviorId: id });
    const existing = api.panels.find(
      (p) => p.params?.kind === "graph" && p.params?.behaviorId === id,
    );
    if (existing) {
      existing.api.setActive();
      return;
    }
    const welcome = api.panels.find((p) => p.params?.kind === "welcome");
    if (welcome) {
      welcome.api.updateParameters({ kind: "graph", behaviorId: id });
      welcome.api.setTitle(behavior.name);
      welcome.api.setActive();
      setFocused({ kind: "graph" });
      return;
    }
    const center = centerPanel(api);
    api.addPanel({
      id: `graph:${id}`,
      title: behavior.name,
      component: "workspace",
      params: { kind: "graph", behaviorId: id },
      position: center
        ? { referencePanel: center.id, direction: "within" }
        : undefined,
      minimumWidth: 320,
      minimumHeight: 180,
      renderer: "always",
    });
  }, []);
  const navigate = useCallback(
    (address: GraphAddress, select?: string) => {
      setInspection(undefined);
      dispatch({ type: "navigate", address, select });
      openBehavior(address.behaviorId);
    },
    [openBehavior],
  );
  const openScript = useCallback(
    (box: Box, address?: GraphAddress, identity?: string) => {
      const api = dockRef.current;
      if (!api || !scriptOpenable(box)) return;
      // Node ids survive Undo within a behavior; removing the behavior retires its tabs and drafts.
      const where = address
        ? graphId(stateRef.current.project?.id ?? "", address)
        : "";
      const id = `script:${identity ?? (box.node !== undefined && address ? `${address.behaviorId}:node${box.node}` : address ? `${where}:${box.id}` : box.name)}`;
      setScripts((old) => ({ ...old, [id]: { box, address } }));
      setInspection(undefined);
      setFocused({ kind: "script", scriptId: id });
      const existing = api.getPanel(id);
      if (existing) {
        existing.api.setActive();
        return;
      }
      const center = centerPanel(api);
      api.addPanel({
        id,
        title: `${box.name}.py`,
        component: "workspace",
        params: { kind: "script", scriptId: id },
        position: center
          ? { referencePanel: center.id, direction: "within" }
          : undefined,
        minimumWidth: 320,
        renderer: "always",
      });
    },
    [],
  );
  const openBox = useCallback(
    (box: Box, address: GraphAddress) => {
      if (
        box.timeline &&
        (box.timeline.enabled ||
          box.timeline.layers.length ||
          box.timeline.curves.length)
      )
        navigate({
          ...address,
          levels: [...address.levels, { boxId: box.id, layer: 0, keyframe: 0 }],
        });
      else if (scriptOpenable(box)) openScript(box, address);
    },
    [navigate, openScript],
  );
  const acceptProject = useCallback((project: ProjectDocument) => {
    // Save as project folder reopens the project inside the same backend session.
    if (stateRef.current.project?.session !== project.session)
      releaseProject(stateRef.current.project);
    // Another project discards drafts (the guard asked first); Save as folder keeps them.
    if (stateRef.current.project?.session !== project.session)
      drafts.current.clear();
    dispatch({ type: "load", project });
    setHistory(undefined);
    setScripts({});
    setInspection(undefined);
    setNotice("");
    previousTimeline.current = "";
    const api = dockRef.current;
    if (!api) return;
    const oldDocuments = api.panels.filter((p) =>
      documentKinds.has(p.params?.kind),
    );
    let keeper = oldDocuments[0];
    for (const panel of oldDocuments.slice(1)) api.removePanel(panel);
    if (!keeper)
      keeper = api.addPanel({
        id: "welcome",
        component: "workspace",
        title: "Welcome",
        params: { kind: "welcome" },
        renderer: "always",
      });
    const first = project.behaviors[0];
    keeper.api.updateParameters(
      first ? { kind: "graph", behaviorId: first.id } : { kind: "welcome" },
    );
    keeper.api.setTitle(first?.name ?? "Empty project");
    keeper.api.setActive();
    setFocused({ kind: first ? "graph" : "welcome" });
    const timeline = api.getPanel("timeline");
    if (timeline) api.removePanel(timeline);
    if (!first)
      setNotice("Project opened successfully. It contains no behaviors.");
  }, []);
  const openFile = useCallback(async () => {
    if (!(await confirmReplace())) return;
    const ticket = openRequests.current.begin();
    setLoading(true);
    try {
      const project = await chooseProject();
      if (project && openRequests.current.current(ticket))
        acceptProject(project);
      else releaseProject(project);
    } catch (e) {
      if (openRequests.current.current(ticket)) {
        setNotice(String(e), "error");
        showPanel("problems");
      }
    } finally {
      if (openRequests.current.current(ticket)) setLoading(false);
    }
  }, [acceptProject, showPanel, confirmReplace]);
  const openExample = useCallback(async () => {
    if (!(await confirmReplace())) return;
    openRequests.current.begin();
    setLoading(false);
    acceptProject(exampleProject());
  }, [acceptProject, confirmReplace]);
  const managePanel = useCallback((id: string) => {
    setPanelTarget(id);
    setSizeAnnouncement("");
    setDestination("");
    setPlacement("center");
    setModal("panel");
  }, []);
  const palette = useCallback((group?: string) => {
    setCommandGroup(group);
    setCommandQuery("");
    setModal("commands");
  }, []);
  const registerGraph = useCallback(
    (id: string, controls: GraphControls | undefined) => {
      if (controls) graphControls.current.set(id, controls);
      else graphControls.current.delete(id);
    },
    [],
  );
  const saveViewport = useCallback(
    (key: string, viewport: Viewport) =>
      dispatch({ type: "viewport", key, viewport }),
    [],
  );
  const saveTimeline = useCallback(
    (key: string, value: TimelineView) =>
      dispatch({ type: "timeline", key, value }),
    [],
  );
  const select = useCallback((key: string, selection: Selection) => {
    setInspection(undefined);
    dispatch({ type: "select", key, selection });
  }, []);
  const address = state.activeBehavior
    ? state.addresses[state.activeBehavior]
    : undefined;
  const resolved = resolveAddress(state.project, address);
  const currentKey =
    state.project && address ? graphId(state.project.id, address) : "";
  const selection = state.selections[currentKey] ?? emptySelection;
  const inspectedBoxes = inspection?.box
    ? [inspection.box]
    : focused.kind === "script" && scripts[focused.scriptId ?? ""]
      ? [scripts[focused.scriptId!].box]
      : focused.kind === "graph"
        ? (resolved?.diagram?.boxes.filter((b) =>
            selection.nodes.includes(String(b.id)),
          ) ?? [])
        : [];
  const inspectedEdges =
    inspection || focused.kind !== "graph"
      ? []
      : selection.edges.map((id) => {
          const link = resolved?.diagram?.links[Number(id.slice(1))];
          if (!link || !resolved?.diagram) return id;
          const endpoints = resolveLink(link, resolved.diagram, resolved.owner);
          return endpoints
            ? `${endpoints.from.port.name} → ${endpoints.to.port.name}`
            : id;
        });
  const diagnostics = useMemo(() => {
    const result: Diagnostic[] = [...(state.project?.diagnostics ?? [])];
    if (state.project)
      for (const entry of outline(state.project)) {
        entry.box.timeline?.layers.forEach((layer, li) =>
          layer.keyframes.forEach((kf, ki) => {
            if (!kf.diagram) return;
            kf.diagram.links.forEach((link, i) => {
              if (!resolveLink(link, kf.diagram!, entry.box))
                result.push({
                  id: `${entry.path}:${li}:${ki}:${i}`,
                  severity: "warning",
                  message: "Connection references a missing box or port.",
                  path: entry.path,
                });
            });
          }),
        );
      }
    if (noticeIsError)
      result.unshift({ id: "operation-error", severity: "error", message: notice });
    return result;
  }, [state.project, notice, noticeIsError]);
  const isEditable = canEdit(state.project);
  const session = state.project?.session ?? 0;
  const readOnlyReason = native
    ? "The bundled example is read-only. Open or create a project to edit."
    : "Editing needs the desktop app; this browser preview is read-only.";
  /** Runs a backend change and shows its document; errors are reported, never thrown. */
  const change = useCallback(
    async (work: () => Promise<Change | null | undefined>) => {
      const expectedSession = stateRef.current.project?.session;
      try {
        const result = await work();
        if (stateRef.current.project?.session !== expectedSession)
          return undefined;
        if (result) {
          // Dockview follow-up actions need new behaviors to exist in the rendered workspace.
          flushSync(() => {
            dispatch({ type: "replace", project: result.document });
            setHistory(result.history);
          });
        }
        return result ?? undefined;
      } catch (e) {
        setNotice(String(e), "error");
        return undefined;
      }
    },
    [],
  );
  const edit = useCallback(
    (label: string, commands: EditCommand[], behaviorId?: string) => {
      const behavior = behaviorId ?? stateRef.current.activeBehavior;
      const id = stateRef.current.project?.session;
      if (!behavior || !id) return Promise.resolve(undefined);
      return change(() => authoring.edit(id, behavior, label, commands));
    },
    [change],
  );
  const currentDiagram = resolved?.diagram;
  const insertLibraryBox = useCallback(
    (path: string, target?: { address: GraphAddress; position: { x: number; y: number } }) => {
      const state = stateRef.current;
      const address = target?.address ?? state.addresses[state.activeBehavior ?? ""];
      const destination = resolveAddress(state.project, address)?.diagram;
      const session = state.project?.session;
      if (!address || destination?.node === undefined || !session) {
        setNotice("Open a flow diagram to insert the box into.", "error");
        return;
      }
      const at = 40 + 20 * (destination.boxes.length % 10);
      const position = target?.position ?? { x: at, y: at };
      const args = { behavior: address.behaviorId, diagram: destination.node, path, ...position };
      const insert = async (policy?: FilePolicy) => {
        if (stateRef.current.project?.session !== session) return;
        try {
          const r = await authoring.insertLibraryBox(session, { ...args, policy });
          if (stateRef.current.project?.session !== session) return;
          if (r.insert.result === "conflicts") {
            setConflict({ paths: r.insert.value, retry: (p) => void insert(p) });
            return;
          }
          dispatch({ type: "replace", project: r.document });
          setHistory(r.insert.value);
        } catch (e) {
          setNotice(String(e), "error");
        }
      };
      void insert();
    },
    [],
  );
  const saveProject = useCallback(async () => {
    try {
      setHistory(await authoring.save(session));
      setNotice("Project saved.");
    } catch (e) {
      setNotice(String(e), "error");
    }
  }, [session]);
  const newProject = useCallback(async () => {
    if (!(await confirmReplace())) return;
    try {
      const dir = await pickFolder({
        directory: true,
        title: "Choose an empty folder for the new project",
      });
      if (dir)
        acceptProject(
          await authoring.newProject(
            dir,
            dir.split(/[\\/]/).pop() || "project",
          ),
        );
    } catch (e) {
      setNotice(String(e), "error");
    }
  }, [acceptProject, confirmReplace]);
  const play = useCallback(async () => {
    const behavior = stateRef.current.activeBehavior;
    if (!behavior || !session) return;
    try {
      // Choregraphe plays what is on disk: save first.
      const saved = !history || history.dirty ? await authoring.save(session) : history;
      if (saved) setHistory(saved);
      showPanel("robot");
      await robot.play(session, behavior, saved?.revision);
    } catch (e) {
      setNotice(String(e), "error");
    }
  }, [session, history, showPanel]);
  const selectedNodes = useCallback(() => {
    const boxes =
      currentDiagram?.boxes.filter((b) =>
        selection.nodes.includes(String(b.id)),
      ) ?? [];
    const links = selection.edges
      .map((id) => currentDiagram?.links[Number(id.slice(1))]?.node)
      .filter((n): n is number => n !== undefined);
    return { boxes, links };
  }, [currentDiagram, selection]);
  const resetLayout = useCallback(() => {
    if (dockRef.current) {
      const active = stateRef.current.activeBehavior;
      defaultLayout(dockRef.current);
      if (active) openBehavior(active);
    }
  }, [openBehavior]);
  const removeContent = async ({project, behaviorId, folder}: RemovalTarget, deleteFiles: boolean) => {
    if (stateRef.current.project !== project) { setNotice("The project changed. Choose what to remove again.", "error"); return false; }
    const result = await change(() => behaviorId ? authoring.removeBehavior(session, behaviorId, deleteFiles) : authoring.removeFolder(session, folder!, deleteFiles));
    if (!result) return false;
    const gone = project.behaviors.filter(b => !result.document.behaviors.some(r => r.id === b.id)).map(b => b.id);
    const goneFiles = new Set(project.files.filter(f => !result.document.files.some(r => r.path === f.path)).map(f => `file:${session}:${f.path}`));
    const removed = (key?: string) => !!key && gone.some(id => key.startsWith(`script:${id}:node`));
    for (const key of [...drafts.current.keys()]) if (removed(key) || goneFiles.has(key)) drafts.current.delete(key);
    draftListeners.current.forEach(listener => listener());
    for (const panel of dockRef.current?.panels ?? [])
      if ((panel.params?.kind === "graph" && gone.includes(panel.params.behaviorId)) || (panel.params?.kind === "script" && removed(panel.params.scriptId))) panel.api.close();
    if (deleteFiles) setNotice("Removed. The files are deleted from disk when you save.");
    return true;
  };
  const runTargeted = (id: string, target: CommandTarget) => {
    const project = stateRef.current.project;
    if (!project) return;
    const behaviorId = target.behaviorId ?? target.address?.behaviorId;
    const behavior = project.behaviors.find(b => b.id === behaviorId);
    const valid = () => stateRef.current.project === project;
    const fail = (error: unknown) => setNotice(String(error), "error");
    if (id.startsWith("import-")) {
      if (!isEditable || project.format !== "pml") return;
      setImportDestination(target.destination ?? "");
      setImportKind(id.slice(7) as ImportKind); setModal("import"); return;
    }
    if (!isEditable) return;
    if (id === "properties") {
      if (project.format !== "pml") return;
      void authoring.manifest(session).then(m => {
        if (!m || !valid()) return;
        setPropertiesBehavior(behavior?.source.replace(/\/behavior\.xar$/, ""));
        setManifest(m); setModal("properties");
      }, fail); return;
    }
    if ((id === "remove-behavior" && behavior) || (id === "remove-folder" && target.folder)) {
      if (project.format !== "pml") return;
      const folder = behavior ? undefined : target.folder;
      const behaviors = behavior ? [behavior] : project.behaviors.filter(b => b.source.startsWith(`${folder}/`));
      const files = project.files.filter(f => folder && f.path.startsWith(`${folder}/`)).map(f => `file:${session}:${f.path}`);
      const hasDrafts = [...drafts.current.keys()].some(key => files.includes(key) || behaviors.some(b => key.startsWith(`script:${b.id}:node`)));
      setRemoval({target: {project, behaviorId: behavior?.id, folder}, warning: hasDrafts ? "Unapplied script and file changes in it will be discarded." : undefined});
      setModal("remove"); return;
    }
    if (id === "rename-project") {
      if (project.format !== "pml") return;
      setRenaming({kind: "project"}); setModal("rename"); return;
    }
    if (id === "rename-folder" && target.folder) {
      if (project.format !== "pml") return;
      setRenaming({kind: "folder", path: target.folder}); setModal("rename"); return;
    }
    if (id === "rename-box" && target.address && target.boxId !== undefined) {
      setRenaming({kind: "box", address: target.address, boxId: target.boxId}); setModal("rename"); return;
    }
    if (!behavior) return;
    if (id === "rename-behavior") {
      if (project.format !== "pml") return;
      setRenaming({kind: "behavior", behaviorId: behavior.id}); setModal("rename"); return;
    }
    if (id === "duplicate-behavior") {
      if (project.format !== "pml") return;
      void change(() => authoring.duplicateBehavior(session, behavior.id)).then(result => {
        const added = result?.document.behaviors.find(b => !project.behaviors.some(o => o.id === b.id));
        if (added) { openBehavior(added.id); setNotice(`Duplicated as ${added.name}. Save the project to write it.`); }
      }); return;
    }
    if (id === "export-crg") {
      void (async () => {
        const path = await saveFile({filters: [{name: "Behavior archive", extensions: ["crg"]}], defaultPath: `${behavior.name}.crg`});
        if (!path || !valid()) return;
        await authoring.exportCrg(session, behavior.id, path); setNotice(`Exported ${path}`);
      })().catch(fail); return;
    }
    if (!target.address) return;
    const diagram = resolveAddress(project, target.address)?.diagram;
    const boxes = diagram?.boxes.filter(b => target.boxId === undefined
      ? (stateRef.current.selections[graphId(project.id, target.address!)]?.nodes ?? []).includes(String(b.id))
      : b.id === target.boxId) ?? [];
    const nodes = boxes.flatMap(b => b.node === undefined ? [] : [b.node]);
    if (!nodes.length) return;
    if (id === "convert-to-box") {
      setGroupTarget({project, address: target.address, nodes}); setModal("convert");
    } else if (id === "delete") void edit("Delete box", [{type: "remove", nodes}], behavior.id);
    else if (id === "copy" || id === "cut") void authoring.copy(session, behavior.id, nodes).then(xml => {
      if (!valid()) return;
      setClipboard(xml);
      if (id === "cut") void edit("Cut boxes", [{type: "remove", nodes}], behavior.id);
    }, fail);
  };
  targetedCommand.current = runTargeted;
  const commands: Command[] = [
    ...(["behavior", "files", "folder"] as const).map((kind): Command => ({
      id: `import-${kind}`,
      title: `Import ${kind}…`,
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable ? "Imports need a .pml project. Use Save as project folder first." : readOnlyReason,
      run: () => runTargeted(`import-${kind}`, {}),
    })),
    {
      id: "open",
      title: "Open project…",
      group: "File",
      shortcut: "CmdOrCtrl+O",
      enabled: !loading,
      run: () => {
        void openFile();
      },
    },
    {
      id: "example",
      title: "Explore example project",
      group: "File",
      enabled: true,
      run: openExample,
    },
    {
      id: "new-project",
      title: "New project…",
      group: "File",
      enabled: native,
      reason: native ? undefined : readOnlyReason,
      run: () => void newProject(),
    },
    {
      id: "save",
      title: "Save project",
      group: "File",
      shortcut: "CmdOrCtrl+S",
      enabled: isEditable && !!history?.dirty,
      reason: isEditable ? "There are no unsaved changes." : readOnlyReason,
      run: () => void saveProject(),
    },
    {
      id: "new",
      title: "New behavior",
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable
        ? "Behaviors can be added to .pml projects."
        : readOnlyReason,
      run: () => {
        setNotice("");
        setBehaviorForm({ name: "", root: "Diagram" });
        setModal("new-behavior");
      },
    },
    {
      id: "remove-behavior",
      title: "Remove behavior from project…",
      group: "File",
      enabled:
        isEditable && state.project?.format === "pml" && !!state.activeBehavior,
      reason: isEditable
        ? "Select a behavior of a .pml project."
        : readOnlyReason,
      run: () => runTargeted("remove-behavior", {behaviorId: stateRef.current.activeBehavior}),
    },
    ...([["rename-behavior", "Rename behavior…"], ["duplicate-behavior", "Duplicate behavior"]] as const).map(([id, title]): Command => ({
      id,
      title,
      group: "File",
      enabled: isEditable && state.project?.format === "pml" && !!state.activeBehavior,
      reason: isEditable ? "Select a behavior of a .pml project." : readOnlyReason,
      run: () => runTargeted(id, {behaviorId: stateRef.current.activeBehavior}),
    })),
    {
      id: "rename-project",
      title: "Rename project…",
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable ? "Only .pml projects can be renamed." : readOnlyReason,
      run: () => runTargeted("rename-project", {}),
    },
    {
      id: "properties",
      title: "Edit project properties…",
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable
        ? "Project properties belong to .pml projects."
        : readOnlyReason,
      run: () => runTargeted("properties", {}),
    },
    {
      id: "save-as-folder",
      title: "Save as project folder…",
      group: "File",
      enabled: isEditable,
      reason: isEditable
        ? "Save a copy with the current edits in an empty folder."
        : readOnlyReason,
      run: () =>
        void (async () => {
          if (drafts.current.size) {
            const discard = await new Promise<boolean>((resolve) =>
              setUnsaved({
                drafts: drafts.current.size,
                draftsOnly: true,
                resolve,
              }),
            );
            if (!discard) return;
          }
          const dir = await pickFolder({
            directory: true,
            title: "Choose an empty folder for the project",
          });
          if (!dir) return;
          const name = (stateRef.current.project?.name ?? "project").replace(
            /[\\/<>:"|?*]/g,
            "_",
          );
          const saved = await authoring.saveAsFolder(session, dir, name);
          drafts.current.clear();
          acceptProject(saved);
          setNotice(`Saved as ${dir}/${name}.pml`);
        })().catch((e) => setNotice(String(e), "error")),
    },
    {
      id: "close-project", title: "Close project", group: "File", enabled: !!state.project,
      run: async () => {
        if (!await confirmReplace()) return;
        releaseProject(stateRef.current.project);
        drafts.current.clear(); setHistory(undefined); setScripts({}); setInspection(undefined);
        dispatch({type: "close"});
        const api = dockRef.current;
        for (const panel of api?.panels ?? []) if (documentKinds.has(panel.params?.kind)) api!.removePanel(panel);
        api?.addPanel({id: "welcome", component: "workspace", params: {kind: "welcome"}, title: "Welcome"});
      },
    },
    {
      id: "close-document", title: "Close active tab", group: "File", shortcut: "CmdOrCtrl+W",
      enabled: !!dockRef.current?.activePanel,
      run: () => dockRef.current?.activePanel?.api.close(),
    },
    {
      id: "build-package", title: "Build application package…", group: "File", enabled: isEditable && state.project?.format === "pml",
      run: () => void (async () => {
        if (drafts.current.size) throw new Error("Apply file and script drafts before building a package.");
        const path = await saveFile({defaultPath: `${state.project?.name ?? "application"}.pkg`, filters: [{name: "Robot application package", extensions: ["pkg"]}]});
        if (!path) return;
        await authoring.buildPackage(session, path);
        setNotice(`Built ${path}`);
      })().catch(e => setNotice(String(e), "error")),
    },
    {
      id: "export-crg",
      title: "Export to CRG…",
      group: "File",
      enabled: isEditable && !!state.activeBehavior,
      reason: isEditable ? "Open a behavior first." : readOnlyReason,
      run: () => runTargeted("export-crg", {behaviorId: stateRef.current.activeBehavior}),
    },
    {
      id: "update-translations",
      title: "Update translations",
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable
        ? "Translations belong to .pml projects."
        : readOnlyReason,
      run: () =>
        void change(() => authoring.updateTranslations(session)).then(
          (r) => r && showPanel("translations"),
        ),
    },
    {
      id: "new-dialog",
      title: "New dialog topic…",
      group: "File",
      enabled: isEditable && state.project?.format === "pml",
      reason: isEditable ? "Dialogs belong to .pml projects." : readOnlyReason,
      run: () =>
        void authoring.manifest(session).then(
          (m) => {
            const available = m?.supportedLanguages.length
              ? m.supportedLanguages
              : ["en_US"];
            setDialogForm({ name: "", languages: available, available });
            setModal("new-dialog");
          },
          (e) => setNotice(String(e), "error"),
        ),
    },
    {
      id: "undo",
      title: history?.undo ? `Undo ${history.undo}` : "Undo",
      group: "Edit",
      enabled: isEditable && !!history?.undo,
      reason: isEditable ? "Nothing to undo." : readOnlyReason,
      run: () => void change(() => authoring.undo(session)),
    },
    {
      id: "redo",
      title: history?.redo ? `Redo ${history.redo}` : "Redo",
      group: "Edit",
      enabled: isEditable && !!history?.redo,
      reason: isEditable ? "Nothing to redo." : readOnlyReason,
      run: () => void change(() => authoring.redo(session)),
    },
    {
      id: "insert",
      title: "Create a new box…",
      group: "Edit",
      enabled: isEditable && !!currentDiagram,
      reason: isEditable ? "Open a flow diagram first." : readOnlyReason,
      run: () => setModal("new-box"),
    },
    {
      id: "delete",
      title: "Delete selection",
      group: "Edit",
      enabled:
        isEditable &&
        focused.kind === "graph" &&
        !!(selection.nodes.length || selection.edges.length),
      reason: isEditable
        ? "Select boxes or connections in a diagram."
        : readOnlyReason,
      run: () => {
        const { boxes, links } = selectedNodes();
        void edit("Delete selection in diagram", [
          { type: "remove", nodes: [...links, ...boxes.map((b) => b.node!)] },
        ]);
      },
    },
    {
      id: "copy",
      title: "Copy boxes",
      group: "Edit",
      enabled:
        isEditable && focused.kind === "graph" && !!selection.nodes.length,
      reason: isEditable ? "Select boxes in a diagram." : readOnlyReason,
      run: () => {
        const behavior = stateRef.current.activeBehavior;
        if (behavior)
          void authoring
            .copy(
              session,
              behavior,
              selectedNodes().boxes.map((b) => b.node!),
            )
            .then(setClipboard, (e) => setNotice(String(e), "error"));
      },
    },
    {
      id: "cut",
      title: "Cut boxes",
      group: "Edit",
      enabled: isEditable && focused.kind === "graph" && !!selection.nodes.length,
      run: async () => {
        const behavior = stateRef.current.activeBehavior;
        const nodes = selectedNodes().boxes.map(b => b.node!);
        if (!behavior) return;
        try {
          const xml = await authoring.copy(session, behavior, nodes);
          setClipboard(xml);
          await edit("Cut boxes", [{type: "remove", nodes}], behavior);
        } catch (e) { setNotice(String(e), "error"); }
      },
    },
    {
      id: "paste",
      title: "Paste boxes",
      group: "Edit",
      enabled: isEditable && !!currentDiagram && !!clipboard,
      reason: isEditable ? "Copy boxes first." : readOnlyReason,
      run: () =>
        currentDiagram?.node !== undefined &&
        void edit("Paste selection in diagram", [
          {
            type: "paste",
            diagram: currentDiagram.node,
            xml: clipboard,
            dx: 30,
            dy: 30,
          },
        ]),
    },
    {
      id: "convert-to-box",
      title: "Group into box…",
      group: "Edit",
      enabled:
        isEditable &&
        focused.kind === "graph" &&
        !!currentDiagram &&
        !!selection.nodes.length,
      reason: isEditable ? "Select boxes in a diagram." : readOnlyReason,
      run: () => { if (address) runTargeted("convert-to-box", {address}); },
    },
    {
      id: "connect",
      title: "Start virtual NAO 6",
      group: "Connection",
      enabled: native && !robotStatus && !robotBusy,
      reason: !native
        ? readOnlyReason
        : robotBusy ? "A connection operation is in progress." : robotStatus
          ? "Already connected."
          : undefined,
      run: () => {
        showPanel("robot");
        void robot.startVirtual("nao6");
      },
    },
    {
      id: "connect-robot",
      title: "Robot connection…",
      group: "Connection",
      shortcut: "CmdOrCtrl+Shift+C",
      enabled: native,
      reason: !native ? readOnlyReason : undefined,
      run: () => setModal("connection"),
    },
    {
      id: "disconnect",
      title: "Disconnect",
      group: "Connection",
      shortcut: "CmdOrCtrl+Shift+D",
      enabled: !!robotStatus && !robotBusy,
      reason: robotBusy ? "A connection operation is in progress." : "Not connected to a robot.",
      run: () => void robot.disconnect().catch((e) => setNotice(String(e), "error")),
    },
    {
      id: "run",
      title: "Play behavior",
      group: "Connection",
      shortcut: "F5",
      enabled:
        !!robotStatus &&
        !robotBusy &&
        !robotStarting &&
        isEditable &&
        state.project?.format === "pml" &&
        !!state.activeBehavior,
      reason: !robotStatus
        ? "Connect to a robot first (Connection › Start virtual NAO 6)."
        : robotStarting
          ? "Uploading the behavior and starting it on the robot…"
          : "Play needs a saved .pml project opened in the desktop app.",
      run: () => void play(),
    },
    {
      id: "stop",
      title: "Stop behavior",
      group: "Connection",
      // Stop also cancels a Play that is still uploading.
      enabled: !!robotStatus && !robotBusy && (robotRunning || robotStarting),
      reason: "No behavior is running.",
      run: () => void robot.stop().catch((e) => setNotice(String(e), "error")),
    },
    {
      id: "quit",
      title: "Quit Choren",
      group: "File",
      enabled: native,
      reason: native ? undefined : readOnlyReason,
      // The predefined macOS Quit would terminate without asking; this goes through the guard.
      run: () =>
        void confirmReplace().then(
          (ok) => ok && void getCurrentWindow().destroy(),
        ),
    },
    {
      id: "settings",
      title: "Settings…",
      group: "File",
      shortcut: mac ? undefined : "CmdOrCtrl+,",
      enabled: true,
      run: () => setModal("settings"),
    },
    {
      id: "commands",
      title: "Command search…",
      group: "View",
      shortcut: "CmdOrCtrl+Shift+P",
      enabled: true,
      run: () => palette(),
    },
    {
      id: "find",
      title: "Find behaviors and boxes",
      group: "View",
      shortcut: "CmdOrCtrl+Shift+F",
      enabled: !!state.project,
      run: () => showPanel("outline"),
    },
    {
      id: "fit",
      title: "Fit graph",
      group: "View",
      enabled: !!resolved?.diagram && focused.kind === "graph",
      run: () =>
        graphControls.current.get(stateRef.current.activeBehavior ?? "")?.fit(),
    },
    {
      id: "fit-selection",
      title: "Fit selection",
      group: "View",
      enabled: !!selection.nodes.length && focused.kind === "graph",
      run: () =>
        graphControls.current
          .get(stateRef.current.activeBehavior ?? "")
          ?.fitSelection(),
    },
    {
      id: "up",
      title: "Go to parent diagram",
      group: "View",
      shortcut: "Alt+Left",
      enabled: (address?.levels.length ?? 0) > 1,
      run: () => {
        if (address)
          navigate({ ...address, levels: address.levels.slice(0, -1) });
      },
    },
    {
      id: "panel-actions",
      title: "Move / resize active panel…",
      group: "View",
      enabled: !!dock?.activePanel,
      run: () => {
        const panel = dockRef.current?.activePanel;
        if (panel) managePanel(panel.id);
      },
    },
    {
      id: "reset-layout",
      title: "Restore default layout",
      group: "View",
      enabled: !!dock,
      run: resetLayout,
    },
    ...panels.map((panel): Command => ({
      id: `panel.${panel.id}`,
      title: `Show ${panel.title}`,
      group: "View",
      enabled: true,
      reason: panel.available ? undefined : panel.reason,
      run: () => showPanel(panel.id),
    })),
    ...unavailableActions.map(([id, title, group, reason]): Command => ({
      id,
      title,
      group,
      reason,
      enabled: false,
      run: () => {},
    })),
    {
      id: "welcome",
      title: "Welcome and setup…",
      group: "Help",
      enabled: true,
      run: () => setModal("welcome"),
    },
    {
      id: "check-updates",
      title: "Check for updates…",
      group: "Help",
      enabled: native,
      reason: native ? undefined : readOnlyReason,
      run: () => void checkForUpdates(true),
    },
    {
      id: "help",
      title: "Workspace help",
      group: "Help",
      enabled: true,
      run: () => {
        setNotice(
          "Drag panel tabs to arrange the workspace. Space + drag pans the graph; double-click opens a box. F6 cycles panel focus. Use panel actions for keyboard movement and resizing.",
        );
      },
    },
    {
      id: "about",
      title: "About Choren",
      group: "Help",
      enabled: true,
      run: () => setModal("about"),
    },
  ];
  commandRef.current = commands;
  const commandSignature = commands
    .map((c) => `${c.id}:${c.enabled}`)
    .join("|");
  useEffect(() => {
    let handle: NativeMenuHandle | undefined,
      cancelled = false;
    void installNativeMenu(commandRef.current, execute).then(
      (menu) => {
        if (cancelled) {
          menu.dispose();
          return;
        }
        handle = menu;
        menuRef.current = menu;
        void menu
          .update(commandRef.current)
          .catch((e) => setNotice(`Menu update failed: ${String(e)}`, "error"));
      },
      (e) => setNotice(`Native menu could not load: ${String(e)}`, "error"),
    );
    return () => {
      cancelled = true;
      handle?.dispose();
      menuRef.current = undefined;
    };
  }, [execute]);
  useEffect(() => {
    void menuRef.current
      ?.update(commandRef.current)
      .catch((e) => setNotice(`Menu update failed: ${String(e)}`, "error"));
  }, [commandSignature]);
  useEffect(() => {
    const keyboard = (event: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      if (event.key === "F6") {
        event.preventDefault();
        if (event.shiftKey)
          dockRef.current?.activatePrevious({ includePanel: false });
        else dockRef.current?.activateNext({ includePanel: false });
        dockRef.current?.focus();
        return;
      }
      if (native) return;
      if (event.metaKey || event.ctrlKey) {
        const key = event.key.toLowerCase();
        if (key === "o") {
          event.preventDefault();
          execute("open");
        }
        if (key === "p" && event.shiftKey) {
          event.preventDefault();
          execute("commands");
        }
        if (key === "f" && event.shiftKey) {
          event.preventDefault();
          execute("find");
        }
        if (key === ",") {
          event.preventDefault();
          execute("settings");
        }
      }
      if (event.altKey && event.key === "ArrowLeft") {
        event.preventDefault();
        execute("up");
      }
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [execute]);
  useEffect(() => {
    if (!dock) return;
    const active = dock.onDidActivePanelChange(({ panel }) => {
      window.dispatchEvent(new Event("studio-context-dismiss"));
      if (!panel) return;
      const params = panel.params as WorkspacePanelParams;
      if (params.kind === "graph" && params.behaviorId) {
        dispatch({ type: "activate", behaviorId: params.behaviorId });
        setInspection(undefined);
        setFocused({ kind: "graph" });
      } else if (params.kind === "script") {
        setInspection(undefined);
        setFocused({ kind: "script", scriptId: params.scriptId });
      }
    });
    const removed = dock.onDidRemovePanel((panel) => {
      if (panel.params?.kind === "script")
        setScripts((previous) => {
          const next = { ...previous };
          delete next[panel.params!.scriptId];
          return next;
        });
      if (
        !dock.panels.some(
          (p) => p.params?.kind === "graph" || p.params?.kind === "script",
        )
      ) {
        setFocused({ kind: "welcome" });
        setInspection(undefined);
      }
    });
    let timer: ReturnType<typeof setTimeout>;
    const persist = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const layout = dock.toJSON();
        for (const panel of Object.values(layout.panels))
          if (documentKinds.has(panel.params?.kind)) {
            panel.params = { kind: "welcome" };
            panel.title = "Workspace";
          }
        savePreference(LAYOUT_KEY, { version: 1, layout });
      }, 250);
    };
    const layout = dock.onDidLayoutChange(persist);
    return () => {
      clearTimeout(timer);
      active.dispose();
      removed.dispose();
      layout.dispose();
    };
  }, [dock]);
  const timelineIdentity =
    resolved?.owner.timeline?.enabled && address
      ? JSON.stringify([address.behaviorId, address.levels.map((l) => l.boxId)])
      : "";
  useEffect(() => {
    if (timelineIdentity && timelineIdentity !== previousTimeline.current)
      showPanel("timeline");
    previousTimeline.current = timelineIdentity;
  }, [timelineIdentity, showPanel]);
  const onReady = useCallback(({ api }: DockviewReadyEvent) => {
    dockRef.current = api;
    const saved = readPreference(LAYOUT_KEY);
    let restored = false;
    if (validLayout(saved))
      try {
        api.fromJSON(saved.layout);
        const documents = api.panels.filter((p) =>
          documentKinds.has(p.params?.kind),
        );
        for (const extra of documents.slice(1)) api.removePanel(extra);
        if (!api.panels.length) throw new Error("Empty layout");
        restored = true;
      } catch {
        setNotice(
          "The saved layout could not be restored. The default layout has been restored.",
        );
      }
    if (!restored) defaultLayout(api);
    setDock(api);
  }, []);
  const value: WorkspaceContextValue = {
    editable: isEditable,
    inspectorEditable: isEditable && focused.kind === "graph" && !inspection,
    history,
    edit,
    applyChange: change,
    notify: setNotice,
    insertLibraryBox,
    subscribeDraft: (listener) => {
      draftListeners.current.add(listener);
      return () => {draftListeners.current.delete(listener);};
    },
    setDraft: (id, text) => {
      if (text === undefined) drafts.current.delete(id);
      else drafts.current.set(id, text);
      draftListeners.current.forEach(listener => listener());
    },
    draftOf: (id) => drafts.current.get(id),
    state,
    commands,
    diagnostics,
    scripts,
    inspectedBoxes,
    inspectedEdges,
    inspectedFile: inspection?.file,
    loading,
    dock,
    execute,
    showPanel,
    openExample,
    openBehavior,
    navigate,
    openBox,
    openScript: (box, at, identity) =>
      openScript(
        box,
        inspection?.box === box ? undefined : at,
        identity ?? (inspection?.box === box ? inspection.identity : undefined),
      ),
    reveal: (entry) => {
      navigate(
        entry.address,
        entry.kind === "box" ? String(entry.box.id) : undefined,
      );
      if (entry.kind === "box")
        requestAnimationFrame(() =>
          requestAnimationFrame(() =>
            graphControls.current.get(entry.address.behaviorId)?.fitSelection(),
          ),
        );
    },
    inspectFile: (file) => {
      setInspection({ file });
      showPanel("inspector");
    },
    inspectLibrary: (box, identity) => {
      setInspection({ box, identity });
      showPanel("inspector");
    },
    select,
    saveViewport,
    saveTimeline,
    registerGraph,
    managePanel,
    graphStyle,
  };
  const targetPanel = dock?.getPanel(panelTarget);
  const groups =
    dock?.groups.filter(
      (g) => g.id !== targetPanel?.group.id || g.panels.length > 1,
    ) ?? [];
  const resizePanel = (axis: "width" | "height", change: number) => {
    if (!targetPanel) return;
    targetPanel.api.setSize({ [axis]: targetPanel.api[axis] + change });
    setSizeAnnouncement(
      `${Math.round(targetPanel.api.width)} × ${Math.round(targetPanel.api.height)} pixels. Sizes are limited by the available space.`,
    );
  };
  return (
    <WorkspaceContext.Provider value={value}>
      <div className="app">
        {!native && (
          <div
            className="app-menu"
            role="navigation"
            aria-label="Application menu"
          >
            <strong>
              <Workflow size={15} />
              Choren
            </strong>
            {["File", "Edit", "View", "Connection", "Help"].map((group) => (
              <button key={group} onClick={() => palette(group)}>
                {group}
              </button>
            ))}
            <span className="spacer" />
            <span className="preview-label">Studio preview</span>
          </div>
        )}
        <header className="main-toolbar" aria-label="Main toolbar">
          <button
            className="toolbar-open"
            onClick={() => execute("open")}
            disabled={loading}
            title={`Open project (${mod}O)`}
          >
            <FolderOpen size={16} />
            <span>{loading ? "Opening…" : "Open project"}</span>
          </button>
          {isEditable && (
            <>
              <button
                className="icon-button"
                aria-label="Save project"
                title={`Save project (${mod}S)`}
                disabled={!history?.dirty}
                onClick={() => execute("save")}
              >
                <Save size={16} />
              </button>
              <button
                className="icon-button"
                aria-label="Undo"
                title={
                  history?.undo ? `Undo ${history.undo}` : "Nothing to undo"
                }
                disabled={!history?.undo}
                onClick={() => execute("undo")}
              >
                <Undo2 size={16} />
              </button>
              <button
                className="icon-button"
                aria-label="Redo"
                title={
                  history?.redo ? `Redo ${history.redo}` : "Nothing to redo"
                }
                disabled={!history?.redo}
                onClick={() => execute("redo")}
              >
                <Redo2 size={16} />
              </button>
            </>
          )}
          <span className="separator" />
          <button className="command-search" onClick={() => palette()}>
            <Search size={14} />
            <span>Search commands</span>
            <kbd>{mod}⇧P</kbd>
          </button>
          <span className="spacer" />
          <span className="project-title" title={state.project?.source}>
            {state.project?.name ?? "Behavior workspace"}
          </span>
          <label className="checkbox" title="Send parameter changes to the running behavior">
            <input type="checkbox" aria-label="Auto-update parameters on robot" checked={autoUpdateParameters} onChange={e => robot.setAutoUpdateParameters(e.target.checked)} />Live parameters
          </label>
          <button title="Connect to a virtual or real robot, or disconnect" disabled={!native} onClick={() => setModal("connection")}>
            Connection…
          </button>
          <span className="spacer" />
          <button
            className="connection-status"
            title={robotStatus ? robotStatus.url : "Connect to a robot"}
            onClick={() => showPanel("robot")}
          >
            <Circle size={8} fill="currentColor" />
            {robotStatus
              ? `${robotStatus.virtual ? "Virtual" : "Connected"}${robotStarting ? " · Starting…" : robotRunning ? " · Running" : ""}`
              : "Offline"}
          </button>
          <span className="separator" />
          <button
            className="icon-button"
            disabled={!commands.find((c) => c.id === "run")?.enabled}
            aria-label="Run behavior"
            title={commands.find((c) => c.id === "run")?.reason}
            onClick={() => execute("run")}
          >
            {robotStarting ? <LoaderCircle size={16} className="spin" /> : <Play size={16} />}
          </button>
          <button
            className="icon-button"
            disabled={!commands.find((c) => c.id === "stop")?.enabled}
            aria-label="Stop behavior"
            title={commands.find((c) => c.id === "stop")?.reason}
            onClick={() => execute("stop")}
          >
            <Square size={14} />
          </button>
          <button
            className="toolbar-layout"
            onClick={() => palette("View")}
            title="Workspace tools"
          >
            <LayoutDashboard size={16} />
            <span>Workspace</span>
            <ChevronDown size={12} />
          </button>
          <button
            className="icon-button"
            aria-label="Settings"
            title="Settings"
            onClick={() => execute("settings")}
          >
            <Settings size={16} />
          </button>
        </header>
        {notice && (
          <div
            className={`notice ${noticeIsError ? "notice-error" : ""}`}
            role={noticeIsError ? "alert" : "status"}
          >
            {noticeIsError ? <X size={15} aria-hidden="true" /> : <Check size={15} aria-hidden="true" />}
            <span>{notice}</span>
            <button
              onClick={() => setNotice("")}
              aria-label="Dismiss notification"
            >
              ×
            </button>
          </div>
        )}
        <main
          className="workbench"
          onPointerDownCapture={(event) => {
            // WebKit can select page text during a divider drag despite user-select: none.
            if ((event.target as Element).closest(".dv-sash"))
              event.preventDefault();
          }}
        >
          <DockviewReact
            components={components}
            defaultTabComponent={PanelTab}
            watermarkComponent={Welcome}
            theme={studioTheme}
            dndStrategy="pointer"
            disableFloatingGroups
            rightHeaderActionsComponent={HeaderActions}
            onReady={onReady}
          />
        </main>
        <footer className="status-bar">
          <span className="status-mode">
            <Circle size={7} fill="currentColor" />
            {!isEditable
              ? "Read-only"
              : history?.dirty
                ? "Unsaved changes"
                : "Saved"}
          </span>
          <button onClick={() => showPanel("problems")}>
            <AlertTriangle size={12} />
            {diagnostics.length} problems
          </button>
          <span className="spacer" />
          <span className="status-selection">
            {inspectedBoxes.length === 1
              ? inspectedBoxes[0].name
              : `${selection.nodes.length} boxes selected`}
          </span>
          <span>
            {state.project ? state.project.format.toUpperCase() : "No project"}
          </span>
        </footer>
        {modal === "commands" && (
          <Modal
            title={commandGroup ? `${commandGroup} commands` : "Command search"}
            onClose={() => setModal(undefined)}
            wide
          >
            <div className="command-input">
              <Search size={18} />
              <input
                autoFocus
                aria-label="Search commands"
                value={commandQuery}
                placeholder="Type a command or tool name…"
                onChange={(e) => setCommandQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    (
                      e.currentTarget
                        .closest("dialog")
                        ?.querySelector(
                          ".command-result:not(:disabled)",
                        ) as HTMLElement
                    )?.focus();
                  }
                }}
              />
            </div>
            <div
              className="command-results"
              onKeyDown={(e) => {
                if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
                e.preventDefault();
                const buttons = [
                  ...e.currentTarget.querySelectorAll<HTMLButtonElement>(
                    "button:not(:disabled)",
                  ),
                ];
                const index = buttons.indexOf(
                  document.activeElement as HTMLButtonElement,
                );
                buttons[
                  (index + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) %
                    buttons.length
                ]?.focus();
              }}
            >
              {commands
                .filter(
                  (c) =>
                    (!commandGroup || c.group === commandGroup) &&
                    `${c.title} ${c.reason ?? ""}`
                      .toLowerCase()
                      .includes(commandQuery.toLowerCase()),
                )
                .map((c) => (
                  <button
                    className="command-result"
                    key={c.id}
                    disabled={!c.enabled}
                    onClick={() => {
                      setModal(undefined);
                      execute(c.id);
                    }}
                  >
                    <span>
                      <strong>{c.title}</strong>
                      {c.reason && <small>{c.reason}</small>}
                    </span>
                    <span className="spacer" />
                    {c.shortcut ? (
                      <kbd>{c.shortcut.replace("CmdOrCtrl+", mod)}</kbd>
                    ) : (
                      <small>{c.group}</small>
                    )}
                  </button>
                ))}
            </div>
          </Modal>
        )}
        {modal === "about" && <AboutDialog onClose={() => setModal(undefined)} />}
        {modal === "welcome" && (
          <WelcomeSetup
            onClose={() => {
              savePreference(WELCOME_KEY, "done");
              setModal(undefined);
            }}
          />
        )}
        {modal === "connection" && <Connection onClose={() => setModal(undefined)} onConnected={() => showPanel("robot")} />}
        {modal === "remove" && removal && <RemoveContent target={removal.target} warning={removal.warning}
          onClose={() => {setModal(undefined); setRemoval(undefined);}} onRemove={deleteFiles => removeContent(removal.target, deleteFiles)} />}
        {modal === "rename" && renaming && <RenameContent target={renaming} onClose={() => {setModal(undefined); setRenaming(undefined);}} />}
        {modal === "import" && <ImportContent initialDestination={importDestination} kind={importKind} onClose={() => setModal(undefined)} />}
        {modal === "new-behavior" && (
          <Modal
            title="New behavior"
            onClose={() => !creatingBehavior && setModal(undefined)}
          >
            <form
              className="settings-content"
              onSubmit={async (e) => {
                e.preventDefault();
                if (creatingBehavior) return;
                setCreatingBehavior(true);
                const result = await change(() =>
                  authoring.addBehavior(
                    session,
                    behaviorForm.name.trim(),
                    behaviorForm.root,
                  ),
                );
                setCreatingBehavior(false);
                if (!result) return;
                setModal(undefined);
                const added = result.document.behaviors.at(-1);
                if (added) openBehavior(added.id);
              }}
            >
              <label className="setting">
                Behavior name
                <input
                  autoFocus
                  value={behaviorForm.name}
                  placeholder="Automatic name"
                  onChange={(e) =>
                    setBehaviorForm({ ...behaviorForm, name: e.target.value })
                  }
                />
              </label>
              <label className="setting">
                Behavior type
                <select
                  value={behaviorForm.root}
                  onChange={(e) =>
                    setBehaviorForm({
                      ...behaviorForm,
                      root: e.target.value as "Diagram" | "Timeline",
                    })
                  }
                >
                  <option value="Diagram">Diagram</option>
                  <option value="Timeline">Timeline</option>
                </select>
              </label>
              <p className="muted">
                Diagrams connect boxes. Timelines add timed behavior layers and
                robot motion.
              </p>
              {noticeIsError && (
                <p className="error" role="alert">
                  {notice}
                </p>
              )}
              <button type="submit" disabled={creatingBehavior}>
                {creatingBehavior ? "Creating…" : "Create"}
              </button>
            </form>
          </Modal>
        )}
        {modal === "new-box" && (
          <Modal title="Create a new box" onClose={() => setModal(undefined)}>
            <form
              className="settings-content"
              onSubmit={(e) => {
                e.preventDefault();
                const diagram = currentDiagram?.node;
                if (diagram === undefined) return;
                setModal(undefined);
                const at = 40 + 20 * ((currentDiagram?.boxes.length ?? 0) % 10);
                void edit(`Add a new ${newBox.kind} box`, [
                  {
                    type: "newBox",
                    diagram,
                    kind: newBox.kind,
                    name: newBox.name,
                    dialogFile: newBox.kind === "Dialog" ? newBox.dialogFile : undefined,
                    x: at,
                    y: at,
                  },
                ]);
              }}
            >
              <label className="setting">
                Type
                <select
                  value={newBox.kind}
                  onChange={(e) =>
                    setNewBox({ ...newBox, kind: e.target.value as NewBoxKind })
                  }
                >
                  <option value="Python">Python</option>
                  <option value="Diagram">Diagram</option>
                  <option value="Timeline">Timeline</option>
                  <option value="Dialog">Dialog</option>
                </select>
              </label>
              <label className="setting">
                Name
                <input
                  value={newBox.name}
                  onChange={(e) =>
                    setNewBox({ ...newBox, name: e.target.value })
                  }
                  required
                />
              </label>
              {newBox.kind === "Dialog" && <label className="setting">Dialog file
                <select required value={newBox.dialogFile ?? ""} onChange={e => setNewBox({...newBox, dialogFile: e.target.value})}>
                  <option value="">Select a project dialog…</option>
                  {state.project?.files.filter(f => f.kind === "dialog").map(f => <option key={f.path} value={behaviorRelative(state.project!.behaviors.find(b => b.id === state.activeBehavior)?.source ?? "", f.path)}>{f.path}</option>)}
                </select>
              </label>}
              <button type="submit">Create</button>
            </form>
          </Modal>
        )}
        {modal === "convert" && (
          <Modal title="Group into box…" onClose={() => setModal(undefined)}>
            <form
              className="settings-content"
              onSubmit={(e) => {
                e.preventDefault();
                const diagram = groupTarget && resolveAddress(groupTarget.project, groupTarget.address)?.diagram?.node;
                if (diagram === undefined || !groupTarget || stateRef.current.project !== groupTarget.project) {
                  setModal(undefined); setNotice("The diagram changed. Select the boxes again.", "error"); return;
                }
                setModal(undefined);
                void edit("Group into box", [
                  {
                    type: "convertToBox",
                    diagram,
                    boxes: groupTarget.nodes,
                    name: convertName,
                  },
                ], groupTarget.address.behaviorId);
              }}
            >
              <label className="setting">
                Name of the new box
                <input
                  value={convertName}
                  onChange={(e) => setConvertName(e.target.value)}
                  required
                />
              </label>
              <button type="submit">Group</button>
            </form>
          </Modal>
        )}
        {modal === "new-dialog" && (
          <Modal title="New dialog topic" onClose={() => setModal(undefined)}>
            <form
              className="settings-content"
              onSubmit={(e) => {
                e.preventDefault();
                setModal(undefined);
                void change(() =>
                  authoring.addDialog(
                    session,
                    dialogForm.name,
                    dialogForm.languages,
                  ),
                ).then((r) => r && showPanel("dialog"));
              }}
            >
              <label className="setting">
                Topic name
                <input
                  value={dialogForm.name}
                  pattern="[A-Za-z0-9_]+"
                  title="Letters, numbers and underscores"
                  onChange={(e) =>
                    setDialogForm({ ...dialogForm, name: e.target.value })
                  }
                  required
                />
              </label>
              <fieldset className="setting">
                <legend>Languages</legend>
                {dialogForm.available.map((lang) => (
                  <label key={lang} className="checkbox">
                    <input
                      type="checkbox"
                      checked={dialogForm.languages.includes(lang)}
                      onChange={(e) =>
                        setDialogForm({
                          ...dialogForm,
                          languages: e.target.checked
                            ? [...dialogForm.languages, lang]
                            : dialogForm.languages.filter((l) => l !== lang),
                        })
                      }
                    />
                    {lang}
                  </label>
                ))}
              </fieldset>
              <button type="submit" disabled={!dialogForm.languages.length}>
                Create
              </button>
            </form>
          </Modal>
        )}
        {modal === "properties" && manifest && <ProjectProperties initialBehaviorPath={propertiesBehavior} initial={manifest}
          onClose={() => setModal(undefined)} onApply={m => change(() => authoring.setManifest(session, m))}
          onRaw={() => {
            const file = state.project?.files.find(f => f.kind === "manifest");
            if (file) { setInspection({ file }); showPanel("resource"); setModal(undefined); }
          }} />}
        {update && <UpdateDialog update={update} confirmReplace={confirmReplace} onClose={() => setUpdate(undefined)} />}
        {conflict && (
          <Modal
            title="Attached file conflict"
            onClose={() => setConflict(undefined)}
          >
            <div className="settings-content">
              <p>
                The project already has {conflict.paths.join(", ")}. Which file
                should the inserted box use?
              </p>
              {(
                [
                  ["UseExisting", "Use old attached file"],
                  ["Replace", "Use new attached file"],
                  ["Rename", "Rename new attached file"],
                ] as const
              ).map(([policy, label]) => (
                <button
                  key={policy}
                  onClick={() => {
                    setConflict(undefined);
                    conflict.retry(policy);
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
          </Modal>
        )}
        {unsaved && (
          <Modal
            title="Unsaved changes"
            onClose={() => {
              unsaved.resolve(false);
              setUnsaved(undefined);
            }}
          >
            <div className="settings-content">
              <p>
                {unsaved.draftsOnly
                  ? "Saving as a project folder gives the project new identities, so editor drafts cannot follow."
                  : `${state.project?.name ?? "This project"} has changes that are not saved.`}
                {unsaved.drafts > 0 &&
                  ` ${unsaved.drafts} editor draft${unsaved.drafts > 1 ? "s have" : " has"} text that was not applied. Cancel and choose Apply in its editor to keep it, or Discard to lose it.`}
              </p>
              <div className="add-row">
                {!unsaved.draftsOnly && (
                  <button
                    disabled={!historyRef.current?.dirty || unsaved.drafts > 0}
                    title={
                      unsaved.drafts > 0
                        ? "Apply or discard the editor drafts first: saving would lose them."
                        : undefined
                    }
                    onClick={() => {
                      const done = unsaved.resolve;
                      setUnsaved(undefined);
                      authoring
                        .save(stateRef.current.project?.session ?? 0)
                        .then(
                          (h) => {
                            setHistory(h);
                            done(true);
                          },
                          (e) => {
                            setNotice(String(e), "error");
                            done(false);
                          },
                        );
                    }}
                  >
                    Save
                  </button>
                )}
                <button
                  onClick={() => {
                    unsaved.resolve(true);
                    setUnsaved(undefined);
                  }}
                >
                  Discard
                </button>
                <button
                  onClick={() => {
                    unsaved.resolve(false);
                    setUnsaved(undefined);
                  }}
                >
                  Cancel
                </button>
              </div>
            </div>
          </Modal>
        )}
        {modal === "settings" && (
          <Modal title="Settings" onClose={() => setModal(undefined)}>
            <div className="settings-content">
              <label className="setting">
                <span>
                  Appearance
                  <small>
                    Apply a consistent theme to the whole workspace.
                  </small>
                </span>
                <select
                  aria-label="Appearance"
                  value={colorMode}
                  onChange={(e) => setColorMode(e.target.value as ColorMode)}
                >
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </label>
              <label className="setting">
                <span>
                  Graph UI
                  <small>Switch between colored cards and the original compact boxes.</small>
                </span>
                <select
                  aria-label="Graph UI"
                  value={graphStyle}
                  onChange={(event) => setGraphStyle(event.target.value as GraphStyle)}
                >
                  <option value="choren">Choren UI</option>
                  <option value="classic">Classic UI</option>
                </select>
              </label>
              <label className="setting">
                <span>
                  Text size
                  <small>Scale the text in menus, panels and dialogs.</small>
                </span>
                <select
                  aria-label="Text size"
                  value={textScale}
                  onChange={(event) => setTextScale(Number(event.target.value))}
                >
                  {[0.9, 1, 1.1, 1.25, 1.5].map((v) => (
                    <option key={v} value={v}>{v === 1 ? "Default" : `${Math.round(v * 100)}%`}</option>
                  ))}
                </select>
              </label>
              <VirtualMovementSpeed />
              <Layouts dock={dock} onRestore={() => {if (stateRef.current.activeBehavior) openBehavior(stateRef.current.activeBehavior);}} />
              <div className="setting-block">
                <h3>Choregraphe libraries</h3>
                <p className="muted">
                  Choose the installation's <strong>share/choregraphe</strong>{" "}
                  folder to load libraries and original icons.
                </p>
                <div className="folder-path">
                  {shareDir || "No folder configured"}
                </div>
                <div className="setting-row">
                <button
                  onClick={() => {
                    void chooseLibrary().then(
                      (dir) => {
                        if (dir) setShareDir(dir);
                      },
                      (e) => setNotice(String(e), "error"),
                    );
                  }}
                >
                  <FolderOpen size={15} />
                  Choose folder
                </button>
                {shareDir && (
                  <button onClick={() => setShareDir("")}>
                    Disconnect folder
                  </button>
                )}
                </div>
                <p className="muted">
                  Built-in symbols are used when original icons are unavailable.
                </p>
              </div>
              <button
                className="wide-button"
                onClick={() => {
                  resetLayout();
                  setModal(undefined);
                }}
              >
                <LayoutDashboard size={16} />
                Restore default layout
              </button>
              <p className="muted">
                Project edits are saved explicitly. Layout and appearance preferences are remembered on this computer.
              </p>
            </div>
          </Modal>
        )}
        {modal === "panel" && (
          <Modal
            title={`Panel: ${targetPanel?.title ?? "Workspace"}`}
            onClose={() => setModal(undefined)}
          >
            <div className="settings-content">
              {targetPanel ? (
                <>
                  <h3>Move panel</h3>
                  <label className="setting">
                    Destination
                    <select
                      aria-label="Destination panel group"
                      value={destination}
                      onChange={(e) => setDestination(e.target.value)}
                    >
                      <option value="">Choose a group</option>
                      {groups.map((g) => (
                        <option key={g.id} value={g.id}>
                          {g.panels.map((p) => p.title).join(" / ")}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="setting">
                    Placement
                    <select
                      aria-label="Panel placement"
                      value={placement}
                      onChange={(e) =>
                        setPlacement(e.target.value as typeof placement)
                      }
                    >
                      <option value="center">As a tab</option>
                      <option value="left">Left</option>
                      <option value="right">Right</option>
                      <option value="top">Above</option>
                      <option value="bottom">Below</option>
                    </select>
                  </label>
                  <button
                    className="primary"
                    disabled={!destination}
                    onClick={() => {
                      const group = dock?.groups.find(
                        (g) => g.id === destination,
                      );
                      if (group) {
                        targetPanel.api.moveTo({ group, position: placement });
                        targetPanel.api.setActive();
                        setModal(undefined);
                      }
                    }}
                  >
                    Move panel
                  </button>
                  <h3>Resize panel group</h3>
                  <div className="resize-buttons">
                    <button onClick={() => resizePanel("width", -32)}>
                      Narrower
                    </button>
                    <button onClick={() => resizePanel("width", 32)}>
                      Wider
                    </button>
                    <button onClick={() => resizePanel("height", -32)}>
                      Shorter
                    </button>
                    <button onClick={() => resizePanel("height", 32)}>
                      Taller
                    </button>
                  </div>
                  <p className="muted" aria-live="polite">
                    {sizeAnnouncement ||
                      "Adjust by 32 pixels. Neighboring panels share the available space."}
                  </p>
                  <div className="dialog-actions">
                    <button
                      onClick={() => {
                        targetPanel.api.isMaximized()
                          ? targetPanel.api.exitMaximized()
                          : targetPanel.api.maximize();
                        setModal(undefined);
                      }}
                    >
                      Maximize / restore
                    </button>
                    <button
                      onClick={() => {
                        targetPanel.api.close();
                        setModal(undefined);
                      }}
                    >
                      Close panel
                    </button>
                  </div>
                </>
              ) : (
                <p>Select a panel to arrange it.</p>
              )}
            </div>
          </Modal>
        )}
      </div>
    </WorkspaceContext.Provider>
  );
}
