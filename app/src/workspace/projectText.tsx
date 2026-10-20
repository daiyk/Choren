import { useState } from "react";
import { useWorkspace } from "./context";
import { ProjectTextEditor } from "./ResourcePanel";

/** Dialog and translation files share the guarded project text editor. */
function FilePanel({ kind }: { kind: "topic" | "translation" }) {
  const ws = useWorkspace();
  const files = ws.state.project?.files.filter(f => f.kind === kind) ?? [];
  const [path, setPath] = useState<string>();
  const current = files.find(f => f.path === path)?.path ?? files[0]?.path;
  return <div className="panel-column"><div className="panel-toolbar" role="toolbar" aria-label={kind === "topic" ? "Dialog topics" : "Translations"}>
    <select aria-label={kind === "topic" ? "Topic file" : "Translation file"} value={current ?? ""} disabled={!files.length} onChange={e => setPath(e.target.value)}>
      {!files.length && <option value="">No files</option>}{files.map(f => <option key={f.path}>{f.path}</option>)}
    </select>
    <button disabled={!ws.editable} onClick={() => ws.execute(kind === "topic" ? "new-dialog" : "update-translations")}>{kind === "topic" ? "New dialog topic…" : "Update translations"}</button>
    {kind === "topic" && <button onClick={() => ws.showPanel("robot-dialog")}>Talk to robot…</button>}
  </div>{current && ws.editable ? <ProjectTextEditor key={`${ws.state.project?.session}:${current}`} path={current} /> : <p className="panel-message">{kind === "topic" ? "This project has no dialog topics yet." : "Update translations to gather localized string parameters for every project language."}</p>}</div>;
}
export const DialogPanel = () => <FilePanel kind="topic" />;
export const TranslationsPanel = () => <FilePanel kind="translation" />;
