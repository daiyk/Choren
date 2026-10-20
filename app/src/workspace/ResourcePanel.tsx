import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useSyncExternalStore } from "react";
import { authoring } from "./authoring";
import { useWorkspace } from "./context";

/** File drafts use the same close/save guard as script drafts. */
export function ProjectTextEditor({ path }: { path: string }) {
  const ws = useWorkspace();
  const session = ws.state.project?.session;
  const draftId = `file:${session}:${path}`;
  const [original, setOriginal] = useState<string>();
  const draft = useSyncExternalStore(ws.subscribeDraft, () => ws.draftOf(draftId));
  const text = draft ?? original ?? "";
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const kind = ws.state.project?.files.find(f => f.path === path)?.kind;
  const writable = ws.editable && kind !== "project" && kind !== "behavior";
  useEffect(() => {
    let live = true;
    if (session) void authoring.readText(session, path).then(value => {
      if (!live) return;
      setOriginal(value); setError("");
      const draft = ws.draftOf(draftId);
      if (draft === value) ws.setDraft(draftId, undefined);
    }).catch(e => live && setError(String(e)));
    return () => { live = false; };
    // File identity is the component key. Changes to other files must not discard this draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session, path, ws.history?.revision]);
  const apply = async () => {
    if (!session || busy) return;
    setBusy(true);
    const result = await ws.applyChange(() => authoring.writeText(session, path, text));
    if (result) { if (ws.draftOf(draftId) === text) ws.setDraft(draftId, undefined); setOriginal(text); }
    setBusy(false);
  };
  return <div className="panel-column project-text-editor">
    <div className="panel-toolbar">
      <span title={path}>{path}</span>{kind === "translation" && <span className="muted">{text.match(/<message[ >]/g)?.length ?? 0} strings · {text.match(/type="unfinished"/g)?.length ?? 0} unfinished</span>}<span className="spacer" />
      <button disabled={!writable || busy || original === undefined || text === original} onClick={() => void apply()}>Apply to project</button>
      <button disabled={busy || original === undefined || text === original} onClick={() => ws.setDraft(draftId, undefined)}>Revert draft</button>
    </div>
    {error ? <p className="panel-message" role="alert">{error}</p> : original === undefined ? <p className="panel-message">Loading…</p> :
      <textarea className="topic-editor" aria-label={kind === "topic" ? "Topic text" : kind === "translation" ? "Translation file text" : "File text"} readOnly={!writable || busy} spellCheck={false} value={text}
        onChange={e => ws.setDraft(draftId, e.target.value === original ? undefined : e.target.value)}
        onKeyDown={e => {if (writable && (e.metaKey || e.ctrlKey) && e.key === "Enter") {e.preventDefault(); void apply();}}} />}
  </div>;
}

export function ResourcePanel() {
  const ws = useWorkspace();
  const files = ws.state.project?.files ?? [];
  const file = files.find(f => f.path === ws.inspectedFile?.path) ?? files.find(f => f.kind === "resource") ?? files[0];
  const [renaming, setRenaming] = useState(false);
  const [target, setTarget] = useState("");
  return <div className="panel-column">
    <div className="panel-toolbar">
      <select aria-label="Project resource" value={file?.path ?? ""} disabled={!files.length}
        onChange={e => {setRenaming(false); ws.inspectFile(files.find(f => f.path === e.target.value)!);}}>
        {!files.length && <option value="">No project files</option>}
        {files.map(f => <option key={f.path}>{f.path}</option>)}
      </select>
      <button disabled={!ws.editable || file?.kind !== "resource"} onClick={() => {setTarget(file!.path); setRenaming(true);}}>Rename…</button>
      <button disabled={!ws.editable || file?.kind !== "resource"} onClick={() => {
        if (window.confirm(`Remove ${file!.path} from the project? Files already saved on disk are kept. Box references are not removed. Any unapplied draft for this file will be discarded.`))
          void ws.applyChange(() => authoring.resource(ws.state.project!.session!, file!.path)).then(result => {
            if (result) ws.setDraft(`file:${ws.state.project!.session}:${file!.path}`, undefined);
          });
      }}>Remove reference</button>
    </div>
    {renaming && file && <form className="settings-content" onSubmit={async e => {
      e.preventDefault();
      const destination = target.trim().replace(/^(\.\/)+/, "");
      const result = await ws.applyChange(() => authoring.resource(ws.state.project!.session!, file.path, destination));
      if (result) {
        setRenaming(false);
        const draftId = `file:${ws.state.project!.session}:${file.path}`;
        const draft = ws.draftOf(draftId);
        const renamed = result.document.files.find(f => f.path === destination);
        if (renamed) {ws.setDraft(`file:${ws.state.project!.session}:${renamed.path}`, draft); ws.setDraft(draftId, undefined); ws.inspectFile(renamed);}
      }
    }}>
      <label>Project-relative path<input aria-label="Resource path" required value={target} onChange={e => setTarget(e.target.value)} /></label>
      <p>Resource parameters and bitmap paths follow the rename. Update any paths written inside scripts yourself.</p>
      <button type="submit">Rename</button><button type="button" onClick={() => setRenaming(false)}>Cancel</button>
    </form>}
    {file && ws.editable ? mediaType(file.path) ? <ResourcePreview key={`${ws.state.project?.session}:${file.path}`} path={file.path} /> : <ProjectTextEditor key={`${ws.state.project?.session}:${file.path}`} path={file.path} /> : <p className="panel-message">Open a project in the desktop app to inspect its files.</p>}
  </div>;
}

export function HistoryPanel() {
  const ws = useWorkspace();
  return <div className="panel-column">
    <div className="panel-toolbar"><button disabled={!ws.history?.undo} onClick={() => ws.execute("undo")}>Undo</button><button disabled={!ws.history?.redo} onClick={() => ws.execute("redo")}>Redo</button></div>
    <div className="panel-scroll"><ol aria-label="Applied edits">{ws.history?.undoSteps?.map((label, i) => <li key={i}>{label}</li>)}</ol>
      {!!ws.history?.redoSteps?.length && <><p>Available to redo</p><ol>{ws.history.redoSteps.map((label, i) => <li key={i}>{label}</li>)}</ol></>}
      {!ws.history?.undo && !ws.history?.redo && <p className="panel-message">No edits in this session.</p>}
    </div>
  </div>;
}

const MEDIA: Record<string, string> = {png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", svg: "image/svg+xml", wav: "audio/wav", ogg: "audio/ogg", mp3: "audio/mpeg", mp4: "video/mp4", webm: "video/webm"};
const mediaType = (path: string) => MEDIA[path.split(".").at(-1)?.toLowerCase() ?? ""];
function ResourcePreview({path}: {path: string}) {
  const ws = useWorkspace();
  const session = ws.state.project?.session;
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true, objectUrl = "";
    setUrl(""); setError("");
    void invoke<number[]>("project_read_binary", {session, path}).then(bytes => {
      if (!live) return;
      objectUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], {type: mediaType(path)})); setUrl(objectUrl);
    }).catch(e => live && setError(String(e)));
    return () => {live = false; if (objectUrl) URL.revokeObjectURL(objectUrl);};
  }, [session, path, ws.history?.revision]);
  return <div className="resource-preview panel-scroll">{error ? <p role="alert">{error}</p> : !url ? <p>Loading preview…</p> : mediaType(path).startsWith("image/") ? <img src={url} alt={path} /> : mediaType(path).startsWith("audio/") ? <audio controls src={url} aria-label={path} /> : <video controls src={url} aria-label={path} />}</div>;
}
