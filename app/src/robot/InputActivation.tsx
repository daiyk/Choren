import { useState } from "react";
import type { Port } from "../format/types";
import { Modal } from "../workspace/Modal";
import { robot } from "./store";

export function InputActivation({
  session, behavior, revision, boxName, port, path, onClose,
}: {
  session: number;
  behavior: string;
  revision?: number;
  boxName: string;
  port: Port;
  path: number[][];
  onClose: () => void;
}) {
  const [text, setText] = useState(port.type === 2 ? "0" : port.type === 0 ? "null" : "");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={`Activate ${boxName}.${port.name}`} onClose={() => !busy && onClose()}>
      <form className="settings-content" onSubmit={(event) => {
        event.preventDefault();
        if (busy) return;
        void (async () => {
          setError("");
          try {
            let value: unknown = text;
            if (port.type === 2) {
              value = Number(text);
              if (!text.trim() || !Number.isFinite(value)) throw new Error("Enter a finite number.");
            } else if (port.type === 0) {
              try {
                value = JSON.parse(text, (_key, item) => {
                  if (typeof item === "number" && !Number.isFinite(item))
                    throw new Error("Non-finite number");
                  return item;
                });
              } catch {
                throw new Error("Enter a valid JSON value with finite numbers.");
              }
            }
            setBusy(true);
            await robot.activateInput(session, behavior, revision, path, port.id, value);
            onClose();
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        })();
      }}>
        <label className="setting">
          {port.type === 0 ? "Value (JSON)" : port.type === 2 ? "Number" : "Text"}
          <input autoFocus value={text} onChange={(e) => setText(e.target.value)} disabled={busy} />
        </label>
        {error && <p className="error" role="alert">{error}</p>}
        <button type="submit" disabled={busy}>{busy ? "Activating…" : "Activate input"}</button>
      </form>
    </Modal>
  );
}
