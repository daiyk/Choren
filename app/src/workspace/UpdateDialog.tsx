import { useState } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Modal } from "./Modal";

/** Offers a release found by the updater; installing restarts the app once work is saved or discarded. */
export function UpdateDialog({ update, confirmReplace, onClose }: {
  update: Update;
  confirmReplace: () => Promise<boolean>;
  onClose: () => void;
}) {
  const [progress, setProgress] = useState<string>();
  const [error, setError] = useState("");
  const install = async () => {
    if (!(await confirmReplace())) return;
    setError("");
    setProgress("Downloading…");
    try {
      let total = 0;
      let received = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          if (total) setProgress(`Downloading… ${Math.round((received / total) * 100)}%`);
        }
        if (event.event === "Finished") setProgress("Installing…");
      });
      await relaunch();
    } catch (e) {
      setProgress(undefined);
      setError(`The update failed: ${e}`);
    }
  };
  return <Modal title="Update available" onClose={() => !progress && onClose()}>
    <div className="settings-content">
      <p>Choren {update.version} is available. You have {update.currentVersion}.</p>
      {update.body && <p className="muted" style={{ whiteSpace: "pre-wrap" }}>{update.body}</p>}
      <p className="muted">Choren restarts after the update. You are asked about unsaved changes first.</p>
      {error && <p className="error" role="alert">{error}</p>}
      <div className="dialog-actions">
        <button type="button" disabled={!!progress} onClick={onClose}>Later</button>
        <button type="button" disabled={!!progress} onClick={() => void install()}>{progress ?? "Update and restart"}</button>
      </div>
    </div>
  </Modal>;
}
