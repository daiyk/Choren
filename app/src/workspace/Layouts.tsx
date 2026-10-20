import { useState } from "react";
import type { DockviewApi } from "dockview-react";
import { readPreference, savePreference, validLayout } from "./storage";

const KEY = "choren.named-layouts";
export function Layouts({ dock, onRestore }: { dock?: DockviewApi; onRestore: () => void }) {
  const [name, setName] = useState("");
  const [selected, setSelected] = useState("");
  const [error, setError] = useState("");
  const [saved, setSaved] = useState<Record<string, unknown>>(() => {
    const value = readPreference(KEY);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
  });
  const update = (value: Record<string, unknown>) => {setSaved(value); savePreference(KEY, value);};
  return <div className="setting-block"><h3>Saved layouts</h3>
    <p className="muted">Save the current panel arrangement and switch back to it later.</p>
    <div className="setting-row">
    <input aria-label="Layout name" placeholder="Layout name" value={name} onChange={e => setName(e.target.value)} />
    <button disabled={!dock || !name.trim()} onClick={() => {
      const layout = dock!.toJSON();
      for (const panel of Object.values(layout.panels)) if (["welcome", "graph", "script"].includes(panel.params?.kind)) {panel.params = {kind: "welcome"}; panel.title = "Workspace";}
      update({...saved, [name.trim()]: {version: 1, layout}}); setSelected(name.trim());
    }}>Save layout</button>
    </div>
    <div className="setting-row">
    <select aria-label="Saved layout" value={selected} onChange={e => setSelected(e.target.value)}>
      <option value="">Choose a layout…</option>{Object.keys(saved).sort().map(n => <option key={n}>{n}</option>)}
    </select>
    <button disabled={!dock || !validLayout(saved[selected])} onClick={() => {
      const value = saved[selected]; if (!dock || !validLayout(value)) return;
      const previous = dock.toJSON();
      try {dock.fromJSON(structuredClone(value.layout)); onRestore(); setError("");}
      catch (e) {dock.fromJSON(previous); setError(String(e));}
    }}>Load layout</button>
    <button disabled={!selected || !(selected in saved)} onClick={() => {const next = {...saved}; delete next[selected]; update(next); setSelected("");}}>Delete layout</button>
    </div>
    {error && <p className="error" role="alert">{error}</p>}
  </div>;
}
