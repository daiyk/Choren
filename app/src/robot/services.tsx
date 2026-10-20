import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getRobot, robot, useRobot } from "./store";
import { useContextMenu } from "../workspace/ContextMenu";

interface ServiceMeta {
  description: string;
  methods: { id: number; name: string; parameters: string; returns: string }[];
  signals: { name: string; signature: string }[];
}

export function ServicesPanel() {
  const status = useRobot(s => s.status);
  const [services, setServices] = useState<string[]>([]);
  const [service, setService] = useState("");
  const [meta, setMeta] = useState<ServiceMeta>();
  const [method, setMethod] = useState("");
  const [args, setArgs] = useState("[]");
  const [result, setResult] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const menu = useContextMenu([status, service, method, result, meta, busy], e => setError(String(e)));
  useEffect(() => {
    let live = true;
    setServices([]); setMeta(undefined); setService(""); setResult(""); setError("");
    if (status) void invoke<string[]>("robot_services", {service: null}).then(v => {if (live) {setServices(v.sort()); setService(v[0] ?? "");}}).catch(e => live && setError(String(e)));
    return () => {live = false;};
  }, [status]);
  useEffect(() => {
    let live = true;
    setMeta(undefined); setMethod(""); setArgs("[]"); setError("");
    if (status && service) void invoke<ServiceMeta>("robot_services", {service}).then(v => {if (live) {setMeta(v); setMethod(String(v.methods[0]?.id ?? ""));}}).catch(e => live && setError(String(e)));
    return () => {live = false;};
  }, [status, service]);
  const selected = meta?.methods.find(m => String(m.id) === method);
  return <div className="panel-column">{menu.element}<form className="settings-content" aria-label="Service call" onSubmit={async e => {
    e.preventDefault(); if (!selected || busy) return;
    setError(""); setBusy(true);
    try {
      const values = JSON.parse(args);
      if (!Array.isArray(values)) throw new Error("Arguments must be a JSON array.");
      const value = await robot.call(service, selected.name, ...values);
      if (getRobot().status === status) setResult(JSON.stringify(value, null, 2));
    } catch (e) {if (getRobot().status === status) setError(String(e));} finally {setBusy(false);}
  }}>
    {!status && <p>Connect to a robot to inspect its services.</p>}
    <label>Service<select aria-label="Robot service" disabled={busy || !status} value={service} onChange={e => setService(e.target.value)}>{services.map(s => <option key={s}>{s}</option>)}</select></label>
    {meta?.description && <p>{meta.description}</p>}
    <label>Method<select aria-label="Service method" disabled={busy || !meta} value={method} onChange={e => {setMethod(e.target.value); setResult("");}}>
      {meta?.methods.map(m => <option key={m.id} value={m.id}>{m.name}{m.parameters} → {m.returns}</option>)}
    </select></label>
    <label>Arguments (JSON array)<textarea aria-label="Method arguments" disabled={busy} value={args} onChange={e => setArgs(e.target.value)} spellCheck={false} /></label>
    <button type="submit" disabled={!status || !selected || busy}>{busy ? "Calling…" : "Call method"}</button>
    {error && <p className="error" role="alert">{error}</p>}
  </form><pre className="panel-scroll service-result" aria-label="Service result" tabIndex={0} {...menu.trigger(() => [{label: "Copy result", run: () => navigator.clipboard.writeText(result), disabled: !result, reason: "Call a method to get a result."}])}>{result}</pre>
    {!!meta?.signals.length && <details className="settings-content"><summary>Signals</summary>{meta.signals.map(s => <div key={s.name} tabIndex={0} {...menu.trigger(() => [{label: "Copy signal name", run: () => navigator.clipboard.writeText(s.name)}, {label: "Copy signature", run: () => navigator.clipboard.writeText(s.signature)}])}>{s.name} {s.signature}</div>)}</details>}
  </div>;
}

export function ApplicationsPanel() {
  const status = useRobot(s => s.status);
  const running = useRobot(s => s.running);
  const [installed, setInstalled] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const menu = useContextMenu([status, installed, running, busy], e => setError(String(e)));
  const refresh = async () => {
    if (!status || getRobot().status !== status) return;
    const names = await robot.call("ALBehaviorManager", "getInstalledBehaviors");
    if (!Array.isArray(names) || names.some(n => typeof n !== "string")) throw new Error("Unexpected installed behavior list.");
    if (getRobot().status !== status) return;
    setInstalled(names); await robot.refreshRunning();
  };
  useEffect(() => {setInstalled([]); setError(""); if (status) void refresh().catch(e => setError(String(e)));}, [status]);
  const run = async (work: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true); setError("");
    try {await work(); await refresh();} catch (e) {setError(String(e));} finally {setBusy(false);}
  };
  const refreshAction = () => ({label: "Refresh applications", run: () => run(refresh), disabled: !status || busy, reason: !status ? "Connect to a robot first." : "Wait for the current operation."});
  const toggle = (name: string) => run(() => robot.call("ALBehaviorManager", running.includes(name) ? "stopBehavior" : "startBehavior", name));
  return <div className="panel-column" tabIndex={0} {...menu.trigger(() => [refreshAction()])}>{menu.element}<div className="panel-toolbar"><button disabled={!status || busy} onClick={() => void run(refresh)}>Refresh applications</button></div>
    {error && <p className="error" role="alert">{error}</p>}
    <div className="panel-scroll">{!status ? <p className="panel-message">Connect to a robot to see its installed behaviors.</p> : !installed.length ? <p className="panel-message">No installed behaviors.</p> : installed.map(name => <div className="panel-toolbar" key={name} tabIndex={0} {...menu.trigger(() => [
      {label: running.includes(name) ? "Stop behavior" : "Run behavior", run: () => toggle(name), disabled: !status || busy, reason: !status ? "Connect to a robot first." : "Wait for the current operation."}, refreshAction(),
    ])}>
      <span>{name}</span><span className="spacer" /><span>{running.includes(name) ? "Running" : "Stopped"}</span>
      <button disabled={busy} onClick={() => void toggle(name)}>{running.includes(name) ? "Stop" : "Run"}</button>
    </div>)}</div>
  </div>;
}

export function CameraPanel() {
  const status = useRobot(s => s.status);
  const [camera, setCamera] = useState(0);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState("");
  const menu = useContextMenu([status, streaming, camera], e => setError(String(e)));
  const toggle = () => {setError(""); setStreaming(!streaming);};
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {setStreaming(false); setError("");}, [status]);
  useEffect(() => {
    if (!status || !streaming) return;
    let live = true;
    let handle: string | undefined;
    let timer: ReturnType<typeof setTimeout>;
    const stop = async () => {if (handle) {const h = handle; handle = undefined; await robot.call("ALVideoDevice", "unsubscribe", h).catch(() => {});}};
    const tick = async () => {
      try {
        const image = await invoke<{width: number; height: number; bytes: number[]}>("robot_camera_frame", {handle});
        if (!live) return;
        const el = canvas.current;
        if (el) {
          el.width = image.width; el.height = image.height;
          const context = el.getContext("2d")!;
          const pixels = context.createImageData(image.width, image.height);
          for (let i = 0, p = 0; i < image.bytes.length; i += 3, p += 4) {pixels.data[p] = image.bytes[i]; pixels.data[p + 1] = image.bytes[i + 1]; pixels.data[p + 2] = image.bytes[i + 2]; pixels.data[p + 3] = 255;}
          context.putImageData(pixels, 0, 0);
        }
        timer = setTimeout(() => void tick(), 100);
      } catch (e) {if (live) {setError(String(e)); setStreaming(false);} await stop();}
    };
    void robot.call("ALVideoDevice", "subscribeCamera", `Choren-${crypto.randomUUID()}`, camera, 1, 11, 10).then(async h => {
      if (typeof h !== "string" || !h) throw new Error("Camera subscription failed.");
      handle = h;
      if (live) await tick(); else await stop();
    }).catch(e => {if (live) {setError(String(e)); setStreaming(false);}});
    return () => {live = false; clearTimeout(timer); void stop();};
  }, [status, streaming, camera]);
  return <div className="panel-column" tabIndex={0} {...menu.trigger(() => [
    {label: streaming ? "Stop camera" : "Start camera", run: toggle, disabled: !status || status.virtual, reason: !status ? "Connect to a robot first." : "The virtual robot has no camera sensor."},
    ...["Top camera", "Bottom camera"].map((label, index) => ({label, checked: camera === index, run: () => setCamera(index), disabled: streaming, reason: "Stop the camera before switching."})),
  ])}>{menu.element}<div className="panel-toolbar">
    <select aria-label="Robot camera" disabled={streaming} value={camera} onChange={e => setCamera(Number(e.target.value))}><option value={0}>Top camera</option><option value={1}>Bottom camera</option></select>
    <button disabled={!status || status.virtual} onClick={toggle}>{streaming ? "Stop camera" : "Start camera"}</button>
  </div>{status?.virtual && <p className="panel-message">The virtual robot has no camera sensor.</p>}
    {error && <p className="error" role="alert">{error}</p>}<canvas ref={canvas} className="camera-image" aria-label="Robot camera image" />
  </div>;
}
