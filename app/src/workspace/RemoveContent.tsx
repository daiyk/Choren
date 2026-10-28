import { useEffect, useState } from "react";
import { authoring } from "./authoring";
import { Modal } from "./Modal";
import type { ProjectDocument } from "../format/types";

/** A behavior, or every project entry under a folder of the Project tree. */
export interface RemovalTarget {
  project: ProjectDocument;
  behaviorId?: string;
  folder?: string;
}

/** Asks whether removing a behavior or folder also deletes its files from disk (on save). */
export function RemoveContent({target, warning, onClose, onRemove}: {
  target: RemovalTarget;
  warning?: string;
  onClose: () => void;
  onRemove: (deleteFiles: boolean) => Promise<unknown>;
}) {
  const {project, behaviorId, folder} = target;
  const [files, setFiles] = useState<string[]>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    if (project.session) void authoring.removalFiles(project.session, behaviorId, folder)
      .then(f => live && setFiles(f), e => live && setError(String(e)));
    return () => {live = false;};
  }, [project, behaviorId, folder]);
  const behavior = project.behaviors.find(b => b.id === behaviorId);
  const entries = folder ? project.files.filter(f => f.path.startsWith(`${folder}/`)) : [];
  const title = behavior ? "Delete behavior" : "Delete folder";
  const run = async (deleteFiles: boolean) => {
    setBusy(true);
    // Failures are reported in the notice bar, so the dialog closes either way.
    await onRemove(deleteFiles);
    onClose();
  };
  return <Modal title={title} onClose={() => !busy && onClose()}>
    <div className="settings-content import-form">
      {behavior ? <p>Remove “{behavior.name}” from the project?</p> : <>
        <p>Remove {entries.length === 1 ? "the project entry" : `the ${entries.length} project entries`} in “{folder}”?</p>
        {entries.length > 0 && <ul className="import-sources" aria-label="Project entries">{entries.map(f => <li key={f.path}>{f.path}</li>)}</ul>}
      </>}
      {warning && <p>{warning}</p>}
      <p>Its files can stay on disk. “Delete files too” deletes these when you save the project; Undo brings them back.</p>
      {error ? <p className="error" role="alert">{error}</p> : files === undefined ? <p className="muted">Checking files on disk…</p>
        : files.length ? <ul className="import-sources" aria-label="Files to delete">{files.map(path => <li key={path}>{path}</li>)}</ul>
          : <p className="muted">There are no files on disk to delete.</p>}
      <div className="dialog-footer">
        <button type="button" disabled={busy} onClick={onClose}>Cancel</button>
        <button type="button" disabled={busy} onClick={() => void run(false)}>Remove from project, keep files</button>
        <button type="button" className="danger" disabled={busy || !files?.length} onClick={() => void run(true)}>Delete files too</button>
      </div>
    </div>
  </Modal>;
}
