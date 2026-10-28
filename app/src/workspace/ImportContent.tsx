import { useId, useRef, useState } from "react";
import { File, Files, FolderOpen, Info, MoreHorizontal, Workflow } from "lucide-react";
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

const COPY: Record<ImportKind, { lead: string; choose: string; empty: string; field: string; tip: string }> = {
  behavior: {
    lead: "Add a Choregraphe behavior from a .xar file or a .crg archive.",
    choose: "Choose behavior file…",
    empty: "No behavior file selected",
    field: "Behavior folder name",
    tip: "A .crg also brings its packaged resources. For a .xar, add sounds and images with Import files.",
  },
  files: {
    lead: "Copy files into the project.",
    choose: "Choose files…",
    empty: "No files selected",
    field: "Destination folder",
    tip: "A behavior.xar becomes a behavior; .dlg and .top files become dialogs and topics.",
  },
  folder: {
    lead: "Copy a folder and everything inside it into the project.",
    choose: "Choose folder…",
    empty: "No folder selected",
    field: "Destination folder",
    tip: "Each behavior.xar becomes a behavior; .dlg and .top files become dialogs and topics. Empty folders are skipped.",
  },
};

const split = (path: string) => {
  const parts = path.split(/[\\/]/);
  const name = parts.pop() ?? path;
  return { name, dir: parts.join("/") };
};

export function ImportContent({ kind, initialDestination = "", onClose }: { kind: ImportKind; initialDestination?: string; onClose: () => void }) {
  const ws = useWorkspace();
  const [paths, setPaths] = useState<string[]>([]);
  const [destination, setDestination] = useState(initialDestination);
  const [conflicts, setConflicts] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const title = `Import ${kind}`;
  const copy = COPY[kind];
  const session = ws.state.project?.session;
  const target = destination.trim().replace(/\/+$/, "");
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
        const { name, dir } = split(paths[0]);
        setDestination(name.toLowerCase() === "behavior.xar" ? split(dir).name || "Imported behavior" : name.replace(/\.(xar|crg)$/i, ""));
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
          ? await authoring.importBehavior(session, paths[0], target, policy)
          : await authoring.importFiles(session, paths, target, kind === "folder", policy);
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
      ? result.document.behaviors.filter((b) => b.source === `${target}/behavior.xar`)
      : result.document.behaviors.filter((b) => !before.includes(b.id));
    if (added[0]) ws.openBehavior(added[0].id);
    const behaviors = kind !== "behavior" && added.length ? ` with ${added.length === 1 ? "1 behavior" : `${added.length} behaviors`}` : "";
    ws.notify(`${kind === "behavior" ? "Behavior" : kind === "folder" ? "Folder" : "Files"} imported${behaviors}. Save the project to write the imported content.`);
  };
  // Where the content lands, as the project tree will show it.
  const preview = !paths.length ? "" : kind === "behavior"
    ? target && `${target}/behavior.xar`
    : kind === "folder" ? `${target ? `${target}/` : ""}${split(paths[0]).name}/`
    : `${target ? `${target}/` : "project root"}`;
  const SourceIcon = kind === "behavior" ? Workflow : kind === "folder" ? FolderOpen : paths.length > 1 ? Files : File;
  return <>
  <Modal title={title} onClose={() => !busy && onClose()}>
    <form className="settings-content import-form" onSubmit={(e) => { e.preventDefault(); void run(); }}>
      <p className="import-lead">{copy.lead}</p>
      <div className="import-field">
        <span className="import-label">{kind === "folder" ? "Folder" : kind === "files" ? "Files" : "Source"}</span>
        <div className={`import-source ${paths.length ? "chosen" : ""}`}>
          <SourceIcon size={18} className="import-source-icon" aria-hidden />
          {paths.length ? <ul className="import-source-list" aria-label="Selected sources">
            {paths.map((path) => { const { name, dir } = split(path); return <li key={path} title={path}><strong>{name}</strong><small>{dir}</small></li>; })}
          </ul> : <span className="import-source-empty">{copy.empty}</span>}
          <button type="button" autoFocus disabled={busy} aria-label={copy.choose} onClick={() => void choose()}>{paths.length ? "Change…" : copy.choose}</button>
        </div>
      </div>
      <label className="import-field">
        <span className="import-label">{copy.field}</span>
        <input value={destination} disabled={busy} required={kind === "behavior"} placeholder={kind === "behavior" ? "e.g. Dance" : "Project root"} onChange={(e) => setDestination(e.target.value)} />
        <small className="import-hint">{preview ? <>Imported as <code>{preview}</code>{paths.length > 1 && ` · ${paths.length} files`}</> : kind === "behavior" ? "The folder that holds behavior.xar in the project." : "A project-relative folder such as assets/audio. Leave empty for the project root."}</small>
      </label>
      <p className="import-tip"><Info size={14} aria-hidden />{copy.tip}</p>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="dialog-footer">
        <span className="dialog-note">Written on Save · Undoable</span>
        <button type="button" disabled={busy} onClick={onClose}>Cancel</button>
        <button type="submit" className="primary" disabled={busy || !paths.length || (kind === "behavior" && !target)}>{busy ? "Importing…" : title}</button>
      </div>
    </form>
  </Modal>
  {conflicts.length > 0 && <Modal title="Replace existing content?" onClose={() => setConflicts([])}>
    <div className="settings-content import-form">
      <p className="import-lead">{kind === "behavior" && conflicts[0].endsWith("/behavior.xar") ? "A behavior with this name already exists." : `The project already has ${conflicts.length === 1 ? "a file" : `${conflicts.length} files`} with the same name.`}</p>
      <ul className="import-sources" aria-label="Existing content">{conflicts.map((path) => <li key={path}>{path}</li>)}</ul>
      <div className="dialog-footer">
        <button type="button" onClick={() => setConflicts([])}>Cancel</button>
        {kind !== "behavior" && <button type="button" className="primary" onClick={() => void run("UseExisting")}>{kind === "folder" ? "Merge, keep existing files" : "Skip these files"}</button>}
        <button type="button" onClick={() => void run("Replace")}>{kind === "folder" ? "Merge, replace files" : "Replace"}</button>
      </div>
    </div>
  </Modal>}
  </>;
}
