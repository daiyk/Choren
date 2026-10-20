import { useEffect, useState } from "react";
import { getRobot, robot, useRobot } from "./store";

type Kind = "Behavior" | "Dialog";
interface Entry {id: string; name: string; kind: Kind; start: number; end?: number}
const source = (kind: Kind) => kind === "Behavior" ? ["ALBehaviorManager", "getRunningBehaviors", "stopBehavior"] : ["ALDialog", "getActivatedTopics", "deactivateTopic"];

export function ActiveContentPanel() {
  const status = useRobot(s => s.status);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [filter, setFilter] = useState("");
  const [kind, setKind] = useState("");
  const [history, setHistory] = useState(true);
  const [timestamp, setTimestamp] = useState("clock");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    setEntries([]); setError("");
    const tick = async () => {
      const kinds: Kind[] = ["Behavior", "Dialog"];
      const results = await Promise.allSettled(kinds.map(async kind => {
        const [service, method] = source(kind);
        const names = await robot.call(service, method);
        if (!Array.isArray(names) || names.some(n => typeof n !== "string")) throw new Error(`${kind} list is unavailable.`);
        return {kind, names: names as string[]};
      }));
      if (!live || getRobot().status !== status) return;
      const time = Date.now(); setNow(time);
      setError(results.flatMap(r => r.status === "rejected" ? [String(r.reason)] : []).join("\n"));
      setEntries(current => {
        let next = [...current];
        for (const result of results) if (result.status === "fulfilled") {
          const {kind, names} = result.value;
          next = next.map(e => e.kind === kind && e.end === undefined && !names.includes(e.name) ? {...e, end: time} : e);
          for (const name of new Set(names)) if (!next.some(e => e.kind === kind && e.name === name && e.end === undefined)) next.push({id: crypto.randomUUID(), kind, name, start: time});
        }
        const active = next.filter(e => e.end === undefined);
        return [...next.filter(e => e.end !== undefined).slice(-500), ...active];
      });
      timer = setTimeout(() => void tick(), 1000);
    };
    if (status) void tick();
    return () => {live = false; clearTimeout(timer);};
  }, [status]);
  const stop = async (items: Entry[]) => {
    if (!status || busy) return;
    setBusy(true); setError("");
    const results = await Promise.allSettled(items.map(e => {const [service,,method] = source(e.kind); return robot.call(service, method, e.name);}));
    if (getRobot().status === status) setError(results.flatMap(r => r.status === "rejected" ? [String(r.reason)] : []).join("\n"));
    setBusy(false);
  };
  const formatTime = (value: number) => timestamp === "milliseconds" ? String(value) : timestamp === "seconds" ? (value / 1000).toFixed(3) : new Date(value).toLocaleString();
  const active = entries.filter(e => e.end === undefined);
  const visible = entries.filter(e => (history || e.end === undefined) && (!kind || e.kind === kind) && e.name.toLowerCase().includes(filter.toLowerCase()));
  return <div className="panel-column">
    <div className="panel-toolbar"><strong>Active content ({active.length})</strong><span className="spacer" />
      <button disabled={!status || busy || !active.length} onClick={() => void stop(active)}>Stop all active content</button>
      <button onClick={() => setEntries(current => current.filter(e => e.end === undefined))}>Clear history</button>
    </div>
    <div className="panel-toolbar"><input aria-label="Filter active content" placeholder="Filter by name" value={filter} onChange={e => setFilter(e.target.value)} />
      <select aria-label="Content type" value={kind} onChange={e => setKind(e.target.value)}><option value="">All types</option><option>Behavior</option><option>Dialog</option></select>
      <label><input type="checkbox" checked={history} onChange={e => setHistory(e.target.checked)} />Show history</label>
      <select aria-label="Timestamp format" value={timestamp} onChange={e => setTimestamp(e.target.value)}><option value="clock">Date and time</option><option value="seconds">POSIX seconds</option><option value="milliseconds">POSIX milliseconds</option></select>
    </div>
    {error && <p className="error" role="alert">{error}</p>}
    <div className="panel-scroll"><p className="panel-message">{status ? "Observed while this panel is connected. Times mark when activity was first or last seen; up to 500 finished entries are kept." : "Connect to a robot to inspect active content."}</p>
      <table className="active-content-table"><thead><tr>{["Name", "Type", "Status", "First seen", "Last seen", "Duration", "Action"].map(v => <th key={v}>{v}</th>)}</tr></thead><tbody>
        {visible.map(e => <tr key={e.id}><td>{e.name}</td><td>{e.kind}</td><td>{e.end === undefined ? "Running" : "Stopped"}</td><td>{formatTime(e.start)}</td><td>{e.end === undefined ? "—" : formatTime(e.end)}</td><td>{Math.max(0, ((e.end ?? now) - e.start) / 1000).toFixed(1)} s</td><td><button aria-label={`Stop ${e.kind.toLowerCase()} ${e.name}`} disabled={!status || busy || e.end !== undefined} onClick={() => void stop([e])}>Stop</button></td></tr>)}
      </tbody></table>
    </div>
  </div>;
}
