import { useEffect, useRef, useState } from "react";
import { ArrowUp, CheckCircle2, ChevronRight, Download, File, Folder, FolderOpen, FolderPlus, RefreshCw, Server, Trash2, Unplug, Upload } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { native } from "../platform";
import { Modal } from "../workspace/Modal";
import { useRobot } from "./store";

interface Entry { name: string; dir: boolean; size: number }
interface Listing { path: string; entries: Entry[] }
const join = (dir: string, name: string) => `${dir.replace(/\/$/, "")}/${name}`;
const parent = (dir: string) => dir.replace(/\/[^/]*\/?$/, "") || "/";
const size = (n: number) => n < 1024 ? `${n} B` : n < 1 << 20 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`;
const host = (url?: string) => url?.replace(/^\w+:\/\//, "").replace(/:\d+$/, "") ?? "";

/** SFTP uses its own SSH login, independently of the robot's behavior connection. */
export function FilesPanel() {
  const status = useRobot((s) => s.status);
  const robotHost = status && !status.virtual ? host(status.url) : "";
  const [address, setAddress] = useState(robotHost);
  const [user, setUser] = useState("nao");
  const [password, setPassword] = useState("");
  const [folder, setFolder] = useState<string>();
  const [entries, setEntries] = useState<Entry[]>([]);
  const [selected, setSelected] = useState<Entry>();
  const [newFolder, setNewFolder] = useState<string>();
  const [confirmation, setConfirmation] = useState<{ title: string; detail: string; action: string; work: () => Promise<void> }>();
  const [transfer, setTransfer] = useState<string>();
  const [progress, setProgress] = useState<[number, number]>();
  const [canceling, setCanceling] = useState(false);
  const [busy, setBusy] = useState(false);
  const working = useRef(false);
  const mounted = useRef(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => { setAddress((current) => current || robotHost); }, [robotHost]);
  useEffect(() => {
    mounted.current = true;
    if (!native) return;
    const off = listen<[number, number]>("sftp://progress", (e) => setProgress(e.payload));
    void off.catch((e) => setError(String(e)));
    return () => {
      mounted.current = false;
      void off.then((f) => f()).catch(() => {});
      void invoke("sftp_disconnect").catch(() => {});
    };
  }, []);
  const run = async (work: () => Promise<unknown>) => {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try { await work(); }
    catch (e) {
      if (String(e) === "Transfer canceled.") setNotice("Transfer canceled.");
      else setError(String(e));
    } finally {
      working.current = false;
      setBusy(false);
      setTransfer(undefined);
      setProgress(undefined);
      setCanceling(false);
    }
  };
  const cd = async (path: string) => {
    const r = await invoke<Listing>("sftp_list", { path });
    setFolder(r.path);
    setEntries(r.entries);
    setSelected(undefined);
    return r;
  };
  const send = async (local: string, name: string) => {
    setTransfer(`Uploading ${name}`);
    try { await invoke("sftp_upload", { local, remote: join(folder!, name) }); }
    finally {
      // Failed or canceled replacements remove the destination, so refresh those too.
      await cd(folder!).catch((e) => setError(String(e)));
    }
    setNotice(`Uploaded ${name}`);
  };
  const upload = () => run(async () => {
    const local = await open({ title: "Upload file to robot", multiple: false, directory: false });
    if (!local) return;
    const name = local.split(/[\\/]/).pop()!;
    // Refresh before checking names so files added since the last listing are included.
    const latest = await cd(folder!);
    const existing = latest.entries.find((entry) => entry.name === name);
    if (existing?.dir) throw new Error(`A folder named “${name}” already exists. Choose a different file name.`);
    if (existing) {
      setConfirmation({ title: "Replace file?", detail: `“${name}” already exists in ${folder}. Replacing it overwrites the original; it cannot be restored if the transfer fails or is canceled.`, action: "Replace file", work: () => send(local, name) });
    } else await send(local, name);
  });
  const download = (entry: Entry) => run(async () => {
    const local = await save({ title: "Save file as", defaultPath: entry.name });
    if (!local) return;
    setTransfer(`Downloading ${entry.name}`);
    await invoke("sftp_download", { remote: join(folder!, entry.name), local });
    setNotice(`Downloaded ${entry.name}`);
  });
  if (!native) return <p className="panel-message">File transfer is available in the desktop app.</p>;
  if (folder === undefined) return (
    <div className="files-connect panel-scroll">
      <form className="files-connect-card" aria-label="SFTP connection" onSubmit={(e) => {
        e.preventDefault();
        void run(async () => {
          const home = await invoke<string>("sftp_connect", { host: address.trim(), user: user.trim(), password });
          setPassword("");
          if (!mounted.current) { await invoke("sftp_disconnect"); return; }
          try { await cd(home); }
          catch (e) { await invoke("sftp_disconnect").catch(() => {}); throw e; }
        });
      }}>
        <span className="files-emblem"><FolderOpen size={25} /></span>
        <div><h2>Robot files</h2><p className="muted">Upload assets, browse folders, and download files from your robot.</p></div>
        <label>Robot address<input required value={address} disabled={busy} onChange={(e) => setAddress(e.target.value)} placeholder="nao.local or 192.168.1.20" autoCapitalize="none" spellCheck={false} /></label>
        <label>Username<input required value={user} disabled={busy} onChange={(e) => setUser(e.target.value)} autoComplete="username" autoCapitalize="none" spellCheck={false} /></label>
        <label>Password<input type="password" value={password} disabled={busy} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" /></label>
        <button className="primary" type="submit" disabled={busy || !address.trim() || !user.trim()}><Server size={15} />{busy ? "Connecting…" : "Connect to files"}</button>
        <small className="muted">SFTP · Port 22 · Use your robot’s SSH login.</small>
        {error && <p className="error" role="alert">{error}</p>}
      </form>
    </div>
  );
  return (
    <section className="files-panel panel-column" aria-label="Robot file browser">
      <header className="files-heading">
        <span className="files-emblem"><Server size={20} /></span>
        <div className="files-identity"><h2>Robot files</h2><span title={`${user.trim()}@${address.trim()}`}>{user.trim()}@{address.trim()}</span></div>
        <span className="files-badge">SFTP</span>
        <button className="icon-button" aria-label="Disconnect files" title="Disconnect files" disabled={busy} onClick={() => void run(async () => {
          await invoke("sftp_disconnect");
          setFolder(undefined); setEntries([]); setSelected(undefined); setNewFolder(undefined);
        })}><Unplug size={16} /></button>
      </header>
      <div className="files-actions">
        <button className="primary" disabled={busy} onClick={() => void upload()}><Upload size={15} />Upload file</button>
        <button disabled={busy || !selected || selected.dir} onClick={() => void download(selected!)}><Download size={15} />Download</button>
        <button disabled={busy} onClick={() => { setError(""); setNewFolder(""); }}><FolderPlus size={15} />New folder</button>
        <span className="spacer" />
        <button className="icon-button" aria-label="Delete selected item" title="Delete selected item" disabled={busy || !selected} onClick={() => {
          if (selected) setConfirmation({ title: "Delete item?", detail: `Delete “${selected.name}” from the robot? This cannot be undone.${selected.dir ? " The folder must be empty." : ""}`, action: "Delete", work: async () => {
            await invoke("sftp_remove", { path: join(folder, selected.name), dir: selected.dir });
            await cd(folder);
            setNotice(`Deleted ${selected.name}`);
          } });
        }}><Trash2 size={15} /></button>
      </div>
      <div className="files-path">
        <button className="icon-button" aria-label="Parent folder" title="Parent folder" disabled={busy || folder === "/"} onClick={() => void run(() => cd(parent(folder)))}><ArrowUp size={15} /></button>
        <FolderOpen size={15} /><code title={folder}>{folder}</code>
        <button className="icon-button" aria-label="Refresh files" title="Refresh files" disabled={busy} onClick={() => void run(() => cd(folder))}><RefreshCw size={15} /></button>
      </div>
      {error && <p className="files-feedback error" role="alert">{error}</p>}
      {notice && <p className="files-feedback" role="status"><CheckCircle2 size={15} />{notice}</p>}
      <div className="panel-scroll" aria-busy={busy}>
        {entries.length ? <table className="files-table" aria-label="Robot files">
          <thead><tr><th scope="col">Name</th><th scope="col">Size</th></tr></thead>
          <tbody>{entries.map((entry) => <tr key={entry.name} aria-selected={selected === entry} className={selected === entry ? "selected" : undefined}
            onClick={() => { if (!busy) setSelected(entry); }}
            onDoubleClick={() => { if (!busy) void (entry.dir ? run(() => cd(join(folder, entry.name))) : download(entry)); }}>
            <td><div className="files-name">
              {entry.dir ? <Folder size={17} className="files-folder-icon" /> : <File size={17} />}
              <button disabled={busy} title={entry.name} onFocus={() => setSelected(entry)} onClick={(e) => {
                e.stopPropagation();
                if (entry.dir) void run(() => cd(join(folder, entry.name)));
                else setSelected(entry);
              }} onDoubleClick={(e) => { e.stopPropagation(); if (!entry.dir && !busy) void download(entry); }} onKeyDown={(e) => {
                if (e.key === "Enter" && !entry.dir) { e.preventDefault(); void download(entry); }
              }}>{entry.name}</button>
              {entry.dir && <ChevronRight size={14} />}
            </div></td>
            <td>{entry.dir ? "Folder" : size(entry.size)}</td>
          </tr>)}</tbody>
        </table> : <div className="files-empty"><FolderOpen size={32} /><h3>This folder is empty</h3><p>Upload a file to get started.</p><button disabled={busy} onClick={() => void upload()}><Upload size={15} />Choose file…</button></div>}
      </div>
      {transfer && <div className="files-transfer" aria-label="File transfer">
        <div><strong role="status">{transfer}</strong><button disabled={canceling} onClick={async () => {
          setCanceling(true);
          try { await invoke("sftp_cancel"); }
          catch (e) { setError(String(e)); setCanceling(false); }
        }}>{canceling ? "Canceling…" : "Cancel transfer"}</button></div>
        <progress aria-label="Transfer progress" max={progress?.[1] || 1} value={progress ? progress[0] : undefined} />
        <small className="muted">{progress ? `${size(progress[0])} / ${size(progress[1])}` : "Preparing transfer…"}</small>
      </div>}
      <footer className="files-footer"><span>{entries.length} {entries.length === 1 ? "item" : "items"}</span><span>{busy && !transfer ? "Working…" : selected ? `${selected.name} selected` : "Select a file to download"}</span></footer>
      {newFolder !== undefined && <Modal title="New folder" onClose={() => { if (!busy) setNewFolder(undefined); }}>
        <form className="files-dialog" onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invoke("sftp_mkdir", { path: join(folder, newFolder.trim()) });
            await cd(folder);
            setNewFolder(undefined);
          });
        }}>
          <label>Folder name<input autoFocus value={newFolder} disabled={busy} onChange={(e) => setNewFolder(e.target.value)} /></label>
          {error && <p className="error" role="alert">{error}</p>}
          <div className="files-dialog-actions"><button type="button" disabled={busy} onClick={() => setNewFolder(undefined)}>Cancel</button><button className="primary" type="submit" disabled={busy || !newFolder.trim() || [".", ".."].includes(newFolder.trim()) || /[\/\x00]/.test(newFolder)}>Create folder</button></div>
        </form>
      </Modal>}
      {confirmation && <Modal title={confirmation.title} onClose={() => setConfirmation(undefined)}><div className="files-dialog">
        <p>{confirmation.detail}</p><div className="files-dialog-actions"><button autoFocus onClick={() => setConfirmation(undefined)}>Cancel</button><button className="primary" onClick={() => { const work = confirmation.work; setConfirmation(undefined); void run(work); }}>{confirmation.action}</button></div>
      </div></Modal>}
    </section>
  );
}
