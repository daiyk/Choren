import { useState } from "react";
import { Info } from "lucide-react";
import { Modal } from "./Modal";
import { authoring } from "./authoring";
import { useWorkspace } from "./context";
import { resolveAddress, type GraphAddress } from "./model";
import { behaviorFolder, behaviorsUnder, moveEntry, nameOf, parentOf } from "./projectTree";

export type RenameTarget =
  | { kind: "project" }
  | { kind: "behavior"; behaviorId: string }
  | { kind: "folder"; path: string }
  | { kind: "box"; address: GraphAddress; boxId: number };

/** Renames the project, a behavior (its folder), a project folder or a box. */
export function RenameContent({ target, onClose }: { target: RenameTarget; onClose: () => void }) {
  const ws = useWorkspace();
  const project = ws.state.project;
  const box = target.kind === "box" ? resolveAddress(project, target.address)?.diagram?.boxes.find(b => b.id === target.boxId) : undefined;
  const named = target.kind === "box" || target.kind === "project";
  const path = target.kind === "folder" ? target.path
    : target.kind === "behavior" ? behaviorFolder(project?.behaviors.find(b => b.id === target.behaviorId)?.source ?? "") : "";
  const initial = target.kind === "box" ? box?.name ?? "" : target.kind === "project" ? project?.name ?? "" : nameOf(path);
  const [name, setName] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const clean = name.trim();
  const invalid = !clean ? "Enter a name." : !named && (/[\\/]/.test(clean) || clean === "." || clean === "..") ? "A name cannot contain / or \\." : "";
  const destination = parentOf(path) ? `${parentOf(path)}/${clean}` : clean;
  const affected = project && !named ? behaviorsUnder(project, path).length : 0;
  const pml = project?.source.split(/[\\/]/).pop();
  const title = target.kind === "project" ? "Rename project" : target.kind === "box" ? "Rename box" : target.kind === "behavior" ? "Rename behavior" : "Rename folder";
  const submit = async () => {
    if (busy || invalid || clean === initial) return;
    setBusy(true);
    setError("");
    const done = target.kind === "box"
      ? box?.node !== undefined && await ws.edit("Rename box", [{ type: "setAttributes", node: box.node, values: { name: clean } }], target.address.behaviorId)
      : target.kind === "project" ? !!project?.session && !!await ws.applyChange(() => authoring.renameProject(project.session!, clean))
      : await moveEntry(ws, path, destination);
    setBusy(false);
    if (done) onClose(); else setError("The name was not changed. See the notification for details.");
  };
  return <Modal title={title} onClose={() => !busy && onClose()}>
    <form className="settings-content import-form" onSubmit={e => { e.preventDefault(); void submit(); }}>
      <label className="import-field">
        <span className="import-label">Name</span>
        <input autoFocus aria-label="New name" value={name} disabled={busy} onChange={e => setName(e.target.value)} onFocus={e => e.currentTarget.select()} />
        <small className="import-hint">{invalid || (target.kind === "project" ? <>Saved in <code>{pml}</code>; the file keeps its name.</> : target.kind === "box" ? "Shown on the box in its diagram." : <>Renamed to <code>{destination}</code></>)}</small>
      </label>
      {affected > 0 && <p className="import-tip"><Info size={14} aria-hidden />
        {affected === 1 ? "The behavior keeps its contents and properties." : `The ${affected} behaviors inside keep their contents and properties.`} Robot apps and scripts that start a behavior by its path (&lt;package&gt;/{path}) are not updated.
      </p>}
      {error && <p className="error" role="alert">{error}</p>}
      <div className="dialog-footer">
        {target.kind === "project" ? <span className="dialog-note">Written on Save · Undoable</span>
          : target.kind !== "box" && <span className="dialog-note">Files move on Save · Undoable</span>}
        <button type="button" disabled={busy} onClick={onClose}>Cancel</button>
        <button type="submit" className="primary" disabled={busy || !!invalid || clean === initial}>{busy ? "Renaming…" : "Rename"}</button>
      </div>
    </form>
  </Modal>;
}
