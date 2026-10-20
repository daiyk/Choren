import { Inspector } from "../Inspector";
const ScriptViewer = lazy(() => import("./ScriptViewer").then(m => ({default: m.ScriptViewer})));
import type { EditCommand } from "./authoring";
import { lazy, Suspense, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Code2,
  FolderOpen,
  Plus,
  Search,
  Trash2,
  Workflow,
} from "lucide-react";
import type { Box, ProjectDocument } from "../format/types";
import { useShareDir } from "../icons";
import { CategoryBadge } from "../diagram/appearance";
import {
  LIBRARY_BOX_MIME,
  libraryTree,
  type LibraryRoot,
  type LibraryTreeNode,
  native,
  readLibraryBox,
  type LibraryEntry,
} from "../platform";
import { useWorkspace } from "./context";
import { useContextMenu, type MenuAction } from "./ContextMenu";
import { resolveAddress } from "./model";
import { Modal } from "./Modal";
import { readPreference, savePreference } from "./storage";

function LibraryTree({root, nodes, query, selected, onSelect, draggable, menu, actions}: {
  root: LibraryRoot; nodes: LibraryTreeNode[]; query: string; selected?: string; draggable: boolean;
  onSelect: (entry: LibraryEntry) => void;
  menu: ReturnType<typeof useContextMenu>;
  actions: (root: LibraryRoot, node: LibraryTreeNode | undefined, element: Element) => MenuAction[];
}) {
  const matches = (node: LibraryTreeNode): boolean => `${node.relativePath} ${node.name}`.toLowerCase().includes(query.toLowerCase()) || node.children.some(matches);
  return <>{nodes.filter(matches).map(node => node.kind === "folder" ?
    <details className="library-category" key={node.relativePath} open={!!query || undefined}>
      <summary {...menu.trigger(event => actions(root, node, event.currentTarget.parentElement!))}>{node.name}</summary>
      <LibraryTree {...{root, query, selected, onSelect, draggable, menu, actions}} nodes={node.children}/>
    </details> : <button className="tree-row" key={node.relativePath} title={node.boxFile!} aria-pressed={selected === node.boxFile}
      draggable={draggable} onDragStart={event => {event.dataTransfer.setData(LIBRARY_BOX_MIME, node.boxFile!); event.dataTransfer.effectAllowed = "copy";}}
      onClick={() => onSelect(libraryEntry(root, node))} {...menu.trigger(event => actions(root, node, event.currentTarget))}>
      <Workflow size={14}/><span>{node.name}</span>
    </button>)}</>;
}
function libraryEntry(root: LibraryRoot, node: LibraryTreeNode): LibraryEntry {
  return {name: node.name, path: node.boxFile!, category: node.relativePath.split("/").slice(0, -1).join(" / "), source: root.source, root: root.path, relativePath: node.relativePath};
}

export function LibrariesPanel() {
  const ws = useWorkspace();
  const directory = useShareDir();
  const [clipboard, setClipboard] = useState("");
  const [roots, setRoots] = useState<string[]>(() => {
    const saved = readPreference("choren.user-libraries");
    return Array.isArray(saved)
      ? saved.filter((v): v is string => typeof v === "string" && !!v)
      : [];
  });
  const [library, setLibrary] = useState(roots[0] ?? "");
  const [query, setQuery] = useState("");
  const [trees, setTrees] = useState<LibraryRoot[]>([]);
  const [refresh, setRefresh] = useState(0);
  const [folderAction, setFolderAction] = useState<{root: string; path: string; action: "create" | "rename" | "paste"; name: string}>();
  const [manage, setManage] = useState(false);
  const [folderPath, setFolderPath] = useState("");
  const [folderName, setFolderName] = useState("");
  const [editing, setEditing] = useState(false);
  const [script, setScript] = useState(false);
  const [scriptDraft, setScriptDraft] = useState<string>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [requested, setRequested] = useState<LibraryEntry>();
  const [preview, setPreview] = useState<{ box: Box; entry: LibraryEntry }>();
  const [form, setForm] = useState<{
    kind: "create" | "save" | "import";
    archive?: string;
    name: string;
    folder: string;
    node?: number;
    behavior?: string;
    session?: number;
    library?: string;
    document?: ProjectDocument;
  }>();
  useEffect(() => savePreference("choren.user-libraries", roots), [roots]);

  useEffect(() => {
    if (!native) return;
    let live = true;
    setLoading(true);
    setError("");
    void libraryTree(directory, roots)
      .then(
        (result) => {
          if (live) {
            setTrees(result);
            const canonical = result.filter(root => root.source === "user").map(root => root.path);
            if (JSON.stringify(canonical) !== JSON.stringify(roots)) setRoots(canonical);
          }
        },
        async (e) => {
          // An expired install grant must not hide the bundled boxes.
          const bundled = await libraryTree().catch(() => []);
          if (live) {
            setTrees(bundled);
            setError(`${String(e)} Reopen the folder to grant access.`);
          }
        },
      )
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [directory, roots, refresh]);

  useEffect(() => {
    let live = true;
    setPreview(undefined);
    if (requested)
      void readLibraryBox(requested.path).then(
        (box) => {
          if (!live) return;
          const named = { ...box, name: requested.name };
          setPreview({ box: named, entry: requested });
          ws.inspectLibrary(named, requested.path);
        },
        (e) => {
          if (live) setError(String(e));
        },
      );
    return () => {
      live = false;
    };
    // Only a new library selection should replace the Inspector.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requested]);

  const run = async (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const remember = (path: string) => {
    setRoots((old) => [...new Set([...old, path])]);
    setLibrary(path);
    setRefresh((n) => n + 1);
  };
  const choose = () =>
    void run(async () => {
      const path = await open({
        directory: true,
        recursive: true,
        title: "Open a user box library",
      });
      if (path) remember(path.replace(/\\/g, "/").replace(/\/$/, ""));
    });
  const saveBox =
    ws.inspectorEditable && ws.inspectedBoxes.length === 1
      ? ws.inspectedBoxes[0]
      : undefined;
  const previewRoot = trees.find(root => root.path === preview?.entry.root && root.writable)?.path;
  const menu = useContextMenu([trees, roots, busy, refresh, query, ws.state.project, ws.state.activeBehavior], setError);
  const activeAddress = ws.state.addresses[ws.state.activeBehavior ?? ""];
  const canInsert = ws.editable && !!(activeAddress && ws.state.project && resolveAddress(ws.state.project, activeAddress)?.diagram);
  const importArchive = () => void run(async () => {
    const path = await open({filters: [{name: "Box library", extensions: ["cbl"]}]});
    if (path) setForm({kind: "import", archive: path, name: path.replace(/\\/g, "/").split("/").at(-1)!.replace(/\.cbl$/i, ""), folder: ""});
  });
  const exportLibrary = (root: string) => run(async () => {
    const path = await save({defaultPath: `${root.split("/").at(-1)}.cbl`, filters: [{name: "Box library", extensions: ["cbl"]}]});
    if (path) await invoke("library_export_cbl", {library: root, path});
  });
  const closeLibrary = (root: string) => {
    const remaining = roots.filter(r => r !== root);
    setRoots(remaining); if (library === root) setLibrary(remaining[0] ?? "");
    if (requested?.root === root) setRequested(undefined);
  };
  const saveSelected = (root: string, folder = "") => {
    if (!saveBox) return;
    setForm({kind: "save", document: ws.state.project, library: root, name: saveBox.name, folder, node: saveBox.node, behavior: ws.state.activeBehavior, session: ws.state.project?.session});
  };
  const removeEntry = (root: string, path: string, name: string) => {
    if (!confirm(`Delete “${name}” and its contents from the user library? This writes directly to disk and cannot be undone.`)) return;
    return run(async () => {
      await invoke("library_remove", {library: root, path}); setRequested(undefined); setRefresh(n => n + 1);
    });
  };
  const loadEntry = (entry: LibraryEntry, action: "script" | "edit") => run(async () => {
    const box = await readLibraryBox(entry.path);
    if (action === "script") ws.openScript(box, undefined, entry.path);
    else {setRequested(entry); setPreview({box: {...box, name: entry.name}, entry}); setEditing(true); setScript(false); setScriptDraft(undefined);}
  });
  const backgroundActions = (element: Element): MenuAction[] => [
    {label: "Open library…", run: choose, disabled: busy},
    {label: "New library…", run: () => setForm({kind: "create", name: "My boxes", folder: ""}), disabled: busy},
    {label: "Import library archive…", run: importArchive, disabled: busy},
    {label: "Refresh", run: () => setRefresh(n => n + 1), disabled: busy},
    {label: "Expand all", run: () => element.querySelectorAll("details").forEach(d => d.open = true)},
    {label: "Collapse all", run: () => element.querySelectorAll("details").forEach(d => d.open = false)},
  ];
  const actions = (root: LibraryRoot, node: LibraryTreeNode | undefined, element: Element): MenuAction[] => {
    const path = node?.relativePath ?? "";
    const expansion: MenuAction[] = [
      {label: "Expand subtree", run: () => {element.querySelectorAll("details").forEach(d => d.open = true); if (element instanceof HTMLDetailsElement) element.open = true;}},
      {label: "Collapse subtree", run: () => {element.querySelectorAll("details").forEach(d => d.open = false); if (element instanceof HTMLDetailsElement) element.open = false;}},
    ];
    const editFolder = (action: "create" | "rename" | "paste") => setFolderAction({root: root.path, path, action, name: action === "rename" ? path.split("/").at(-1)! : action === "paste" ? `${clipboard.split("/").at(-1) ?? "Box"} copy` : "New folder"});
    if (node?.kind === "box") {
      const entry = libraryEntry(root, node);
      return [
        {label: "Preview", run: () => setRequested(entry)},
        ...(node.hasScript ? [{label: "View script", run: () => loadEntry(entry, "script")}] : []),
        {label: "Insert into diagram", run: () => ws.insertLibraryBox(entry.path), disabled: !canInsert, reason: "Open an editable diagram first."},
        {label: "Copy", run: () => setClipboard(`${root.path}/${path}`)},
        ...(root.writable ? [
          {label: "Edit…", run: () => loadEntry(entry, "edit"), disabled: busy},
          {label: "Rename…", run: () => editFolder("rename"), disabled: busy},
          {label: "Delete…", run: () => removeEntry(root.path, path, node.name), disabled: busy},
        ] : []),
      ];
    }
    return [
      ...(root.writable ? [
        {label: "New folder…", run: () => editFolder("create"), disabled: busy},
        ...(node ? [{label: "Rename…", run: () => editFolder("rename"), disabled: busy}, {label: "Copy", run: () => setClipboard(`${root.path}/${path}`)}] : []),
        {label: "Paste", run: () => editFolder("paste"), disabled: busy || !clipboard, reason: busy ? "Wait for the current operation." : "Copy a library box or folder first."},
        ...(node ? [{label: "Sort by name", run: () => run(async () => {await invoke("library_manage", {library: root.path, folder: path, action: "sort", name: ""}); setRefresh(n => n + 1);}), disabled: busy}] : [
          {label: "Save selected diagram box…", run: () => saveSelected(root.path), disabled: busy || !saveBox || saveBox.node === undefined, reason: "Select one editable diagram box."},
          {label: "Export library…", run: () => exportLibrary(root.path), disabled: busy},
        ]),
      ] : []),
      ...expansion,
      ...(!root.writable ? [{label: "Refresh", run: () => setRefresh(n => n + 1), disabled: busy}] : []),
      ...(root.source === "user" && !node ? [{label: "Close library", run: () => closeLibrary(root.path), disabled: busy}] : []),
      ...(root.writable && node ? [{label: "Delete folder…", run: () => removeEntry(root.path, path, node.name), disabled: busy}] : []),
    ];
  };

  if (!native)
    return (
      <p className="panel-message">
        Box libraries are available in the desktop app.
      </p>
    );
  return (
    <div className="panel-column" tabIndex={0} {...menu.trigger(event => backgroundActions(event.currentTarget))}>{menu.element}
      <div
        className="panel-toolbar library-actions"
        role="toolbar"
        aria-label="Box libraries"
      >
        <button disabled={busy} onClick={choose}>
          <FolderOpen size={13} />
          Open library…
        </button>
        <button
          disabled={busy}
          onClick={() => {
            setError("");
            setForm({ kind: "create", name: "My boxes", folder: "" });
          }}
        >
          <Plus size={13} />
          New library…
        </button>
        <button disabled={busy} onClick={importArchive}>Import .cbl…</button>
      </div>
      <label className="search-field">
        <Search size={14} />
        <input
          aria-label="Search libraries"
          placeholder="Search libraries"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>
      <div className="panel-scroll">
        {roots.length > 0 && (
          <div className="library-target">
            <label>
              User library
              <select
                aria-label="User library"
                value={library}
                onChange={(e) => setLibrary(e.target.value)}
              >
                {roots.map((root) => (
                  <option key={root} value={root}>
                    {root.split("/").at(-1)}
                  </option>
                ))}
              </select>
            </label>
            <button disabled={busy} onClick={() => {setError(""); setManage(true);}}>Manage folders…</button>
            <button disabled={busy || !library} onClick={() => void exportLibrary(library)}>Save as .cbl…</button>
            <button
              disabled={busy || !saveBox || saveBox.node === undefined}
              title="Select one box in a diagram to save its applied definition"
              onClick={() => saveSelected(library)}
            >
              Save selected box…
            </button>
            <button
              disabled={busy}
              title="Remove this library from the workspace; files stay on disk"
              onClick={() => closeLibrary(library)}
            >
              Close library
            </button>
          </div>
        )}
        {loading && <p className="panel-message">Loading libraries…</p>}
        {trees.map(root => <details className="library-root" key={`${root.path}:${query}`} open>
          <summary title={root.path} {...menu.trigger(event => actions(root, undefined, event.currentTarget.parentElement!))}>{root.name} <small>{root.source === "user" ? root.path : "Read-only"}</small></summary>
          <LibraryTree {...{root, query, menu, actions}} nodes={root.children} draggable={canInsert} selected={requested?.path} onSelect={entry => {setError(""); setRequested(entry);}}/>
          {!root.children.length && <p className="panel-message">Empty library</p>}
        </details>)}
        {!trees.length && !loading && <p className="panel-message">No libraries loaded.</p>}
        {error && !form && (
          <p className="panel-message error" role="alert">
            {error}
          </p>
        )}
        {preview && (
          <div className="library-preview">
            <strong>{preview.box.name}</strong>
            <CategoryBadge box={preview.box} />
            <p>{preview.box.tooltip || "No description."}</p>
            {canInsert ? (
              <button onClick={() => ws.insertLibraryBox(preview.entry.path)}>
                <Plus size={14} />
                Insert into diagram
              </button>
            ) : (
              <span className="badge">Preview only</span>
            )}
            {preview.box.script && (
              <button
                onClick={() =>
                  ws.openScript(preview.box, undefined, preview.entry.path)
                }
              >
                <Code2 size={14} />
                View script
              </button>
            )}
            {previewRoot && <button disabled={busy} onClick={() => {setEditing(true); setScript(false); setScriptDraft(undefined);}}>Edit library box…</button>}
            <button disabled={busy || !library} onClick={() => {setClipboard(preview.entry.path.replace(/\/(box|behavior)\.xar$/, "")); setFolderPath(""); setFolderName(`${preview.entry.name} copy`); setManage(true);}}>Copy to user library…</button>
            {previewRoot && (
              <button
                disabled={busy}
                onClick={() => void removeEntry(previewRoot, preview.entry.relativePath!, preview.box.name)}
              >
                <Trash2 size={13} />
                Delete from library
              </button>
            )}
          </div>
        )}
        <div className="panel-toolbar">
          <button onClick={() => ws.execute("settings")} title="Choregraphe installation folder, in Settings">
            {directory ? "Installation settings…" : "Add installation…"}
          </button>
        </div>
      </div>
      {folderAction && <Modal title={folderAction.action === "create" ? "New library folder" : folderAction.action === "rename" ? "Rename library item" : "Paste into library"} onClose={() => !busy && setFolderAction(undefined)}>
        <form className="settings-content" onSubmit={event => {event.preventDefault(); void run(async () => {
          if (folderAction.action === "paste") await invoke("library_copy", {source: clipboard, library: folderAction.root, folder: folderAction.path, name: folderAction.name.trim()});
          else await invoke("library_manage", {library: folderAction.root, folder: folderAction.path, action: folderAction.action, name: folderAction.name.trim()});
          setFolderAction(undefined); setRequested(undefined); setRefresh(n => n + 1);
        });}}>
          <p>{folderAction.root}/{folderAction.path}</p>
          <label>Name<input autoFocus required aria-label="Library item name" value={folderAction.name} onChange={event => setFolderAction({...folderAction, name: event.target.value})}/></label>
          <p>Changes are written directly to disk and do not use project undo.</p>
          {error && <p role="alert" className="error">{error}</p>}
          <button disabled={busy || !folderAction.name.trim()} type="submit">{folderAction.action === "paste" ? "Paste" : folderAction.action === "rename" ? "Rename" : "Create folder"}</button>
        </form>
      </Modal>}
      {manage && <Modal title="Manage library folders" onClose={() => !busy && setManage(false)}>
        <div className="settings-content"><p>{library}</p>
          <label>Folder path (empty = library root)<input aria-label="Library folder path" value={folderPath} onChange={e => setFolderPath(e.target.value)} /></label>
          <label>New name<input aria-label="Library folder name" value={folderName} onChange={e => setFolderName(e.target.value)} /></label>
          <button disabled={busy || !folderPath.trim()} onClick={() => setClipboard(`${library}/${folderPath.trim()}`)}>Copy folder</button>
          <button disabled={busy || !clipboard || !folderName.trim()} onClick={() => void run(async () => {
            await invoke("library_copy", {source: clipboard, library, folder: folderPath.trim(), name: folderName.trim()}); setRefresh(n => n + 1); setManage(false);
          })}>Paste into folder</button>
          {clipboard && <p>Copied: {clipboard}. Paste uses the folder path and new name above.</p>}
          {[["create", "Create folder"], ["rename", "Rename folder"], ["sort", "Sort alphabetically"]].map(([action, label]) => <button key={action} disabled={busy || (action !== "sort" && !folderName.trim()) || (action === "rename" && !folderPath)} onClick={() => void run(async () => {
            await invoke("library_manage", {library, folder: folderPath.trim(), action, name: folderName.trim()});
            setRefresh(n => n + 1); setManage(false);
          })}>{label}</button>)}
          <button disabled={busy || !folderPath} onClick={() => {
            if (confirm(`Delete folder ${folderPath} and all its boxes? This cannot be undone.`)) void run(async () => {
              await invoke("library_remove", {library, path: folderPath}); setRefresh(n => n + 1); setManage(false);
            });
          }}>Delete folder…</button>
          {error && <p className="error" role="alert">{error}</p>}
        </div>
      </Modal>}
      {editing && preview && previewRoot && <Modal title={`Edit library box: ${preview.box.name}`} wide onClose={() => {
        if (!busy && (scriptDraft === undefined || confirm("Discard unapplied library script changes?"))) setEditing(false);
      }}>
        {(() => {
          const apply = async (commands: EditCommand[]) => run(async () => {
            const box = await invoke<Box>("library_edit", {library: previewRoot, path: preview.entry.path, commands});
            setPreview({...preview, box}); setRefresh(n => n + 1);
          });
          return <div className="library-editor"><div className="panel-toolbar"><button onClick={() => setScript(false)}>Properties</button><button onClick={() => setScript(true)}>Script</button></div>
            {script ? <Suspense fallback={<p>Loading editor…</p>}><ScriptViewer box={preview.box} initialDraft={scriptDraft} onDraft={setScriptDraft} onSave={text => void apply([{type: "setScript", box: preview.box.node!, text}])} /></Suspense>
              : <Inspector boxes={[{...preview.box, id: 1}]} onScript={() => setScript(true)} onEdit={(_, commands) => {if (!busy) void apply(commands);}} />}
            {error && <p className="error" role="alert">{error}</p>}
          </div>;
        })()}
      </Modal>}
      {form && (
        <Modal
          title={
            form.kind === "import" ? "Import box library" : form.kind === "create" ? "New box library" : "Save box to library"
          }
          onClose={() => !busy && setForm(undefined)}
        >
          <form
            className="settings-content"
            onSubmit={(e) => {
              e.preventDefault();
              void run(async () => {
                if (form.kind !== "save") {
                  const parent = await open({
                    directory: true,
                    recursive: true,
                    title: "Choose the parent folder for the new library",
                  });
                  if (!parent) return;
                  const dir = `${parent.replace(/\\/g, "/").replace(/\/$/, "")}/${form.name.trim()}`;
                  if (form.kind === "import") await invoke("library_import_cbl", { path: form.archive, dir });
                  else await invoke("library_create", { dir, title: form.name.trim() });
                  remember(dir);
                } else {
                  if (form.document !== ws.state.project) throw new Error("The project changed. Select the box again before saving it.");
                  await invoke("library_add_box", {
                    session: form.session,
                    behavior: form.behavior,
                    node: form.node,
                    library: form.library ?? library,
                    folder: form.folder.trim(),
                    name: form.name.trim(),
                  });
                  setRefresh((n) => n + 1);
                }
                setForm(undefined);
              });
            }}
          >
            <label className="setting">
              {form.kind !== "save" ? "Library name" : "Box name"}
              <input
                autoFocus
                required
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
            {form.kind === "import" && <p>A new or empty folder with this name will be used inside the parent folder you choose.</p>}
            {form.kind === "save" && (
              <>
                <label className="setting">
                  Category
                  <input
                    placeholder="Optional folder"
                    value={form.folder}
                    onChange={(e) =>
                      setForm({ ...form, folder: e.target.value })
                    }
                  />
                </label>
                <p className="muted">
                  Saves the applied box definition. Attached files and unapplied
                  script drafts are not copied.
                </p>
              </>
            )}
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <button
              type="submit"
              disabled={
                busy ||
                !form.name.trim() ||
                /[\\/<>:"|?*]/.test(form.name) ||
                [".", ".."].includes(form.name.trim())
              }
            >
              {busy
                ? "Saving…"
                : form.kind === "import" ? "Import library…" : form.kind === "create"
                  ? "Choose location…"
                  : "Save to library"}
            </button>
          </form>
        </Modal>
      )}
    </div>
  );
}
