import { useEffect, useState } from "react";
import { readPreference, savePreference } from "../workspace/storage";
import { Modal } from "../workspace/Modal";
import { getRobot, robot, useRobot } from "./store";

export function robotAddress(host: string, port: string): string {
  host = host.trim();
  if (!host || /[\s/\\@?#]/.test(host)) throw new Error("Enter a hostname or IP address without a URL or port.");
  if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) throw new Error("Port must be between 1 and 65535.");
  if (host.includes(":")) {
    const address = host.replace(/^\[|\]$/g, "");
    try { new URL(`http://[${address}]`); } catch { throw new Error("Use the Port field for the port, or enter a valid IPv6 address."); }
    host = `[${address}]`;
  } else if (!/^[\p{L}\p{N}_.-]+$/u.test(host)) throw new Error("Enter a valid hostname or IP address.");
  return `tcp://${host}:${Number(port)}`;
}

/** NAOqi port of a real robot when none is given. */
export const ROBOT_PORT = "9561";
const LAST_ROBOT = "choren.last-robot";

/** Remembers a real robot the app connected to, for the next connection dialog. */
function rememberRobot(url: string) {
  const m = /^tcp:\/\/(\[[^\]]+\]|[^:]+):(\d+)$/.exec(url);
  if (m) savePreference(LAST_ROBOT, { host: m[1].replace(/^\[|\]$/g, ""), port: m[2] });
}

export function Connection({ onClose, onConnected }: { onClose: () => void; onConnected: () => void }) {
  const status = useRobot((s) => s.status);
  const busy = useRobot((s) => s.busy);
  const error = useRobot((s) => s.error);
  const [scan, setScan] = useState(0);
  const [discovering, setDiscovering] = useState(true);
  const [found, setFound] = useState<{ name: string; url: string }[]>([]);
  const [discoveryError, setDiscoveryError] = useState("");
  const [kind, setKind] = useState("virtual");
  const [models, setModels] = useState<{ id: string; name: string }[]>([]);
  const [model, setModel] = useState("nao6");
  const [last] = useState(() => {
    const saved = readPreference(LAST_ROBOT) as { host?: unknown; port?: unknown } | undefined;
    return typeof saved?.host === "string" && typeof saved.port === "string" ? { host: saved.host, port: saved.port } : undefined;
  });
  const [host, setHost] = useState(last?.host ?? "");
  const [port, setPort] = useState(last?.port ?? ROBOT_PORT);
  const [favorites, setFavorites] = useState<{host: string; port: string}[]>(() => {
    const saved = readPreference("choren.robot-favorites");
    return Array.isArray(saved) ? saved.filter(v => v && typeof v.host === "string" && typeof v.port === "string") : [];
  });
  const favorite = () => {
    try {
      robotAddress(host, port);
      const next = [...favorites.filter(f => f.host !== host.trim() || f.port !== port), {host: host.trim(), port}];
      setFavorites(next); savePreference("choren.robot-favorites", next);
    } catch (e) { robot.reportError(String(e)); }
  };
  useEffect(() => {
    let active = true;
    void robot.models().then((models) => {
      if (!active) return;
      setModels(models);
      setModel((current) => models.some((m) => m.id === current) ? current : models[0]?.id ?? "");
    }, (e) => { if (active) robot.reportError(String(e)); });
    return () => { active = false; };
  }, []);
  useEffect(() => {
    let active = true;
    setDiscovering(true); setDiscoveryError("");
    void robot.discover().then(robots => {
      if (active) setFound(robots);
    }, e => {
      if (active) { setFound([]); setDiscoveryError(String(e)); }
    }).finally(() => { if (active) setDiscovering(false); });
    return () => { active = false; };
  }, [scan]);
  return <Modal title="Robot connection" onClose={() => !busy && onClose()}>
    <form className="settings-content connection-form" onSubmit={async (e) => {
      e.preventDefault();
      if (busy || status) return;
      robot.clearError();
      try {
        if (kind === "virtual") await robot.startVirtual(model);
        else {
          const url = robotAddress(host, port);
          await robot.connect(url);
          if (getRobot().status) rememberRobot(url);
        }
        if (getRobot().status) { onClose(); onConnected(); }
      } catch (e) { robot.reportError(String(e)); }
    }}>
      <div className="connection-summary" role="status">
        <strong>{status ? `Connected to ${status.virtual ? "virtual" : "real"} robot` : busy ? "Connecting…" : "Disconnected"}</strong>
        {status && <span>{models.find((m) => m.id === status.model)?.name ?? status.model} · {status.url}</span>}
      </div>
      {status ? <>
        <p className="muted">Disconnect to choose another robot.</p>
        <button type="button" disabled={busy} onClick={() => void robot.disconnect().catch((e) => robot.reportError(String(e)))}>{busy ? "Disconnecting…" : "Disconnect"}</button>
        <button type="button" onClick={() => { onClose(); onConnected(); }}>Show robot view</button>
      </> : <>
        <fieldset className="connection-choices" disabled={busy}>
          <legend>Connect to</legend>
          <label><input type="radio" name="robot-kind" value="virtual" checked={kind === "virtual"} onChange={() => setKind("virtual")} /> Virtual robot</label>
          <label><input type="radio" name="robot-kind" value="real" checked={kind === "real"} onChange={() => setKind("real")} /> Real robot</label>
        </fieldset>
        {kind === "virtual" ? <>
          <label className="setting">Robot model<select value={model} disabled={busy || !models.length} onChange={(e) => setModel(e.target.value)}>{models.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
          <p className="muted">Start a local virtual robot to run behaviors and simulate its joints and sensors.</p>
        </> : <>
          <label className="setting">Saved robots<select aria-label="Saved robots" value="" disabled={busy} onChange={e => {
            const f = favorites[Number(e.target.value)]; if (f) {setHost(f.host); setPort(f.port);}
          }}><option value="">Choose a favorite…</option>{favorites.map((f,i) => <option key={i} value={i}>{f.host}:{f.port}</option>)}</select></label>
          <label className="setting">Hostname or IP address<input placeholder="nao.local or 192.168.1.10" value={host} disabled={busy} required onChange={(e) => setHost(e.target.value)} /></label>
          <label className="setting">Port<input type="number" min="1" max="65535" step="1" required value={port} disabled={busy} onChange={(e) => setPort(e.target.value)} /></label>
          <div className="add-row"><button type="button" disabled={busy || !host.trim()} onClick={favorite}>Save favorite</button>
            <button type="button" disabled={busy || !favorites.some(f => f.host === host && f.port === port)} onClick={() => {
              const next = favorites.filter(f => f.host !== host || f.port !== port); setFavorites(next); savePreference("choren.robot-favorites", next);
            }}>Remove favorite</button></div>
          <p className="muted">Enter the robot’s network address and NAOqi port (usually {ROBOT_PORT}).</p>
        </>}
        <button type="submit" disabled={busy || (kind === "virtual" ? !models.length : !host.trim())}>{busy ? "Connecting…" : `Connect to ${kind} robot`}</button>
        {kind === "real" && <>
          <section aria-label="Robots on this network">
            <div className="add-row"><strong>Robots on this network</strong>
              <button type="button" disabled={busy || discovering} onClick={() => setScan(n => n + 1)}>Refresh robots</button></div>
            {discovering ? <p role="status" aria-busy="true">Searching for robots…</p> : discoveryError ? <p className="error" role="alert">{discoveryError}</p> : !found.length ? <p>No robots found</p> : found.map(r =>
              <button type="button" className="tree-row" key={r.url} disabled={busy} onClick={() => {
                void robot.connect(r.url).then(() => { if (getRobot().status) { rememberRobot(r.url); onClose(); onConnected(); } });
              }}>{r.name} · {r.url}</button>)}
          </section>
        </>}
      </>}
      {error && <p className="error" role="alert">{error}</p>}
    </form>
  </Modal>;
}
