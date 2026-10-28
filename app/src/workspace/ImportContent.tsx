import { useId, useRef, useState } from "react";
import { MoreHorizontal } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "./Modal";
import { useWorkspace } from "./context";
import { authoring, type FilePolicy } from "./authoring";

export type ImportKind = "behavior" | "files" | "folder";

export function ImportMenu() {
  const ws = useWorkspace();
  const id = useId();
  const menu = useRef<HTMLDivElement>(null);
  return <>
    <button
      className="icon-button"
      aria-label="Import content"
      title="Import content"
      popoverTarget={id}
      onClick={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        menu.current!.style.left = `${Math.max(8, Math.min(bounds.right - 180, window.innerWidth - 188))}px`;
        menu.current!.style.top = `${bounds.bottom + 4}px`;
      }}
    ><MoreHorizontal size={16} /></button>
    <div id={id} ref={menu} popover="auto" className="project-import-menu" role="group" aria-label="Import content" onToggle={(event) => {
      if (event.newState === "open") menu.current?.querySelector("button")?.focus();
    }}>
      {(["behavior", "files", "folder"] as const).map((kind) => {
        const command = ws.commands.find((c) => c.id === `import-${kind}`);
        return <button key={kind} disabled={!command?.enabled} title={!command?.enabled ? command?.reason : undefined} onClick={() => {
          menu.current?.hidePopover();
          ws.execute(`import-${kind}`);
        }}>{command?.title}</button>;
      })}
    </div>
  </>;
}

export function ImportContent({ kind, initialDestination = "", onClose }: { kind: ImportKind; initialDestination?: string; onClose: () => void }) {
  const ws = useWorkspace();
  const [paths, setPaths] = useState<string[]>([]);
  const [destination, setDestination] = useState(initialDestination);
  const [conflicts, setConflicts] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const title = `Import ${kind}`;
  const session = ws.state.project?.session;
  const choose = async () => {
    setBusy(true);
    setError("");
    try {
      const selected = await open({
        title,
        directory: kind === "folder",
        multiple: kind === "files",
        ...(kind === "behavior" ? { filters: [{ name: "Choregraphe behavior", extensions: ["xar", "crg"] }] } : {}),
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      setPaths(paths);
      if (kind === "behavior") {
        const parts = paths[0].split(/[\\/]/);
        const filename = parts.pop()!;
        setDestination(filename.toLowerCase() === "behavior.xar" ? parts.pop() || "Imported behavior" : filename.replace(/\.(xar|crg)$/i, ""));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  // Same-named content changes nothing at first; the conflict dialog retries with a policy.
  const run = async (policy?: FilePolicy) => {
    if (busy || !session || !paths.length) return;
    setBusy(true);
    setError("");
    setConflicts([]);
    const before = ws.state.project?.behaviors.map(b => b.id) ?? [];
    const result = await ws.applyChange(async () => {
      try {
        const r = kind === "behavior"
          ? await authoring.importBehavior(session, paths[0], destination.trim(), policy)
          : await authoring.importFiles(session, paths, destination.trim(), kind === "folder", policy);
        if ("conflicts" in r) { setConflicts(r.conflicts); return undefined; }
        return r;
      } catch (e) {
        setError(String(e));
        return undefined;
      }
    });
    setBusy(false);
    if (!result) return;
    onClose();
    ws.showPanel("project");
    // Folder and file imports list any behavior.xar they bring as a behavior.
    const added = kind === "behavior"
      ? result.document.behaviors.filter((b) => b.source === `${destination.trim()}/behavior.xar`)
      : result.document.behaviors.filter((b) => !before.includes(b.id));
    if (added[0]) ws.openBehavior(added[0].id);
    const behaviors = kind !== "behavior" && added.length ? ` with ${added.length === 1 ? "1 behavior" : `${added.length} behaviors`}` : "";
    ws.notify(`${kind === "behavior" ? "Behavior" : kind === "folder" ? "Folder" : "Files"} imported${behaviors}. Save the project to write the imported content.`);
  };
  return <>
  <Modal title={title} onClose={() => !busy && onClose()}>
    <form className="settings-content import-form" onSubmit={(e) => { e.preventDefault(); void run(); }}>
      <p className="muted">Add content to <strong>{ws.state.project?.name}</strong>.</p>
      <button type="button" disabled={busy} onClick={() => void choose()}>
        Choose {kind === "behavior" ? "behavior file…" : `${kind}…`}
      </button>
      {paths.length > 0 && <ul className="import-sources" aria-label="Selected sources">{paths.map((path) => <li key={path}>{path}</li>)}</ul>}
      <label className="setting">
        {kind === "behavior" ? "Behavior folder name" : "Destination folder"}
        <input value={destination} disabled={busy} required={kind === "behavior"} placeholder={kind === "behavior" ? "Imported behavior" : "Project root (or e.g. assets/audio)"} onChange={(e) => setDestination(e.target.value)} />
      </label>
      <p className="muted">
        {kind === "behavior"
          ? "XAR imports the behavior definition; add any external resources with Import files or Import folder. CRG also includes its packaged resources. Behavior imports can be undone through project history."
          : kind === "folder" ? "Keeps the selected folder’s name and all files inside it, merging into a folder that already exists. Empty folders are omitted. You can undo this import."
          : "Copies the selected files into this project. You can undo this import."}
        {kind !== "behavior" && " Each behavior.xar becomes a behavior, and .dlg and .top files become dialogs and topics."}
        {" "}If something with the same name exists, you are asked what to do. Changes are written on Save.
      </p>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="dialog-actions">
        <button type="button" disabled={busy} onClick={onClose}>Cancel</button>
        <button type="submit" disabled={busy || !paths.length || (kind === "behavior" && !destination.trim())}>{busy ? "Working…" : title}</button>
      </div>
    </form>
  </Modal>
  {conflicts.length > 0 && <Modal title="Replace existing content?" onClose={() => setConflicts([])}>
    <div className="settings-content">
      <p>{kind === "behavior" && conflicts[0].endsWith("/behavior.xar") ? "A behavior with this name already exists." : "The project already has files with the same names."}</p>
      <ul className="import-sources" aria-label="Existing content">{conflicts.map((path) => <li key={path}>{path}</li>)}</ul>
      <div className="dialog-actions">
        <button type="button" onClick={() => setConflicts([])}>Cancel</button>
        {kind !== "behavior" && <button type="button" onClick={() => void run("UseExisting")}>{kind === "folder" ? "Merge, keep existing files" : "Skip these files"}</button>}
        <button type="button" onClick={() => void run("Replace")}>{kind === "folder" ? "Merge, replace files" : "Replace"}</button>
      </div>
    </div>
  </Modal>}
  </>;
}
