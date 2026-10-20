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
import type { Box } from "../format/types";
import { useShareDir } from "../icons";
import { CategoryBadge } from "../diagram/appearance";
import {
  LIBRARY_BOX_MIME,
  indexLibrary,
  native,
  readLibraryBox,
  type LibraryEntry,
} from "../platform";
import { useWorkspace } from "./context";
import { Modal } from "./Modal";
import { readPreference, savePreference } from "./storage";

interface LibraryFolder {
  folders: Map<string, LibraryFolder>;
  entries: LibraryEntry[];
}

function LibraryTree({
  folder,
  searching,
  selected,
  onSelect,
  draggable,
}: {
  draggable: boolean;
  folder: LibraryFolder;
  searching: boolean;
  selected?: string;
  onSelect: (entry: LibraryEntry) => void;
}) {
  return (
    <>
      {[...folder.folders].map(([name, child]) => (
        <details
          className="library-category"
          key={name}
          open={searching || name === "Choren"}
        >
          <summary>{name}</summary>
          <LibraryTree
            draggable={draggable}
            folder={child}
            searching={searching}
            selected={selected}
            onSelect={onSelect}
          />
        </details>
      ))}
      {folder.entries.map((entry) => (
        <button
          className="tree-row"
          key={entry.path}
          title={entry.path}
          aria-pressed={selected === entry.path}
          draggable={draggable}
          onDragStart={(event) => {
            event.dataTransfer.setData(LIBRARY_BOX_MIME, entry.path);
            event.dataTransfer.effectAllowed = "copy";
          }}
          onClick={() => onSelect(entry)}
        >
          <Workflow size={14} />
          <span>{entry.name}</span>
        </button>
      ))}
    </>
  );
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
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [refresh, setRefresh] = useState(0);
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
  }>();
  useEffect(() => savePreference("choren.user-libraries", roots), [roots]);

  useEffect(() => {
    if (!native) return;
    let live = true;
    setLoading(true);
    setError("");
    void indexLibrary(directory, roots)
      .then(
        (result) => {
          if (live) setEntries(result);
        },
        async (e) => {
          // An expired install grant must not hide the bundled boxes.
          const bundled = await indexLibrary().catch(() => []);
          if (live) {
            setEntries(bundled);
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
  const filtered = entries.filter((entry) =>
    `${entry.category}/${entry.name}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  const tree: LibraryFolder = { folders: new Map(), entries: [] };
  for (const entry of filtered) {
    const path = entry.category
      .split("/")
      .map((part) => part.trim())
      .filter(Boolean);
    let folder = tree;
    for (const name of path.length ? path : ["My boxes"]) {
      if (!folder.folders.has(name))
        folder.folders.set(name, { folders: new Map(), entries: [] });
      folder = folder.folders.get(name)!;
    }
    folder.entries.push(entry);
  }
  const previewRoot =
    preview?.entry.source === "user"
      ? roots.find((root) => preview.entry.path.startsWith(`${root}/`))
      : undefined;

  if (!native)
    return (
      <p className="panel-message">
        Box libraries are available in the desktop app.
      </p>
    );
  return (
    <div className="panel-column">
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
        <button disabled={busy} onClick={() => void run(async () => {
          const path = await open({ filters: [{ name: "Box library", extensions: ["cbl"] }] });
          if (path) setForm({ kind: "import", archive: path, name: path.replace(/\\/g, "/").split("/").at(-1)!.replace(/\.cbl$/i, ""), folder: "" });
        })}>Import .cbl…</button>
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
            <button disabled={busy || !library} onClick={() => void run(async () => {
              const path = await save({ defaultPath: `${library.split("/").at(-1)}.cbl`, filters: [{ name: "Box library", extensions: ["cbl"] }] });
              if (path) await invoke("library_export_cbl", { library, path });
            })}>Save as .cbl…</button>
            <button
              disabled={busy || !saveBox || saveBox.node === undefined}
              title="Select one box in a diagram to save its applied definition"
              onClick={() => {
                setError("");
                setForm({
                  kind: "save",
                  name: saveBox!.name,
                  folder: "",
                  node: saveBox!.node,
                  behavior: ws.state.activeBehavior,
                  session: ws.state.project?.session,
                });
              }}
            >
              Save selected box…
            </button>
            <button
              disabled={busy}
              title="Remove this library from the workspace; files stay on disk"
              onClick={() => {
                const remaining = roots.filter((r) => r !== library);
                setRoots(remaining);
                setLibrary(remaining[0] ?? "");
                setRequested(undefined);
              }}
            >
              Close library
            </button>
          </div>
        )}
        {loading && <p className="panel-message">Loading libraries…</p>}
        <LibraryTree
          key={query}
          draggable={ws.editable}
          folder={tree}
          searching={!!query}
          selected={requested?.path}
          onSelect={(entry) => {
            setError("");
            setRequested(entry);
          }}
        />
        {!filtered.length && !loading && (
          <p className="panel-message">
            {query ? "No matching boxes." : "This library has no boxes yet."}
          </p>
        )}
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
            {ws.editable ? (
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
                onClick={() => {
                  if (
                    !confirm(
                      `Delete “${preview.box.name}” from the user library? This cannot be undone.`,
                    )
                  )
                    return;
                  void run(async () => {
                    const path = preview.entry.path
                      .slice(previewRoot.length + 1)
                      .replace(/\/(box|behavior)\.xar$/, "");
                    await invoke("library_remove", {
                      library: previewRoot,
                      path,
                    });
                    setRequested(undefined);
                    setRefresh((n) => n + 1);
                  });
                }}
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
                  await invoke("library_add_box", {
                    session: form.session,
                    behavior: form.behavior,
                    node: form.node,
                    library,
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
