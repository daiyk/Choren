import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { save } from "@tauri-apps/plugin-dialog";
import { Circle, FolderOpen, Pencil, Plus, Square, Trash2 } from "lucide-react";
import { useShareDir } from "../icons";
import { native } from "../platform";
import { useWorkspace } from "../workspace/context";
import { Modal } from "../workspace/Modal";
import { useContextMenu, type MenuAction } from "../workspace/ContextMenu";
import { graphId, resolveAddress } from "../workspace/model";
import {
  getRobot,
  robot,
  useRobot,
  useRobotDescriptor,
  type RobotDescriptor,
} from "./store";

const DEG = 180 / Math.PI;
const isHand = (j: RobotDescriptor["joints"][number]) =>
  j.axis.every((a) => a === 0);
/** Robot angles (radians, hands 0–1) → file units (degrees, hands 0–1). */
const toFile = (j: RobotDescriptor["joints"][number], v: number) =>
  isHand(j) ? v : v * DEG;

/** Robot-side storage that survives reloads (a per-user convenience). */
function useStored<T>(key: string, initial: T) {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw ? (JSON.parse(raw) as T) : initial;
    } catch {
      return initial;
    }
  });
  const update = (v: T) => {
    setValue(v);
    try {
      localStorage.setItem(key, JSON.stringify(v));
    } catch {
      /* private mode: kept for this session only */
    }
  };
  return [value, update] as const;
}

const message = (text: string) => (
  <div className="panel-scroll">
    <p className="panel-message">{text}</p>
  </div>
);

interface Pose {
  folder: string;
  name: string;
  description: string;
  bitmap: string;
  motors: [string, number][];
}

/** Pose library: the install's standard.xap (read-only) plus the user's own .xap file. */
export function PosesPanel() {
  const shareDir = useShareDir();
  const status = useRobot((s) => s.status);
  const descriptor = useRobotDescriptor(status?.model);
  const [userFile, setUserFile] = useStored<string>("choren-user-poses", "");
  const [libs, setLibs] = useState<
    { file: string; user: boolean; poses: Pose[] }[]
  >([]);
  const [error, setError] = useState<string>();
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("My poses");
  const [editingPose, setEditingPose] = useState<{original: Pose; value: Pose}>();
  const [poseBusy, setPoseBusy] = useState(false);
  const poseName = useRef<HTMLInputElement>(null);
  const menu = useContextMenu([status, libs, userFile, poseBusy], e => setError(String(e)));
  const files = useMemo(
    () =>
      [
        shareDir && {
          file: `${shareDir}/libraries/positions/standard.xap`,
          user: false,
        },
        userFile && { file: userFile, user: true },
      ].filter(Boolean) as { file: string; user: boolean }[],
    [shareDir, userFile],
  );
  useEffect(() => {
    if (!native) return;
    void Promise.all(
      files.map(async (f) => ({
        ...f,
        poses: await invoke<Pose[]>("poses_read", { path: f.file }).catch(
          () => [] as Pose[], // a new user file does not exist until the first Store
        ),
      })),
    ).then(setLibs, (e) => setError(String(e)));
  }, [files]);
  if (!native) return message("The pose library needs the desktop app.");
  const run = (p: Promise<unknown>) => {
    setError(undefined);
    void p.catch((e) => setError(String(e)));
  };
  const apply = (pose: Pose) => {
    const joints = descriptor?.joints ?? [];
    const motors = pose.motors.filter(([n]) =>
      joints.some((j) => j.name === n),
    );
    const values = motors.map(([n, v]) =>
      isHand(joints.find((j) => j.name === n)!) ? v : v / DEG,
    );
    run(
      robot.call(
        "ALMotion",
        "angleInterpolationWithSpeed",
        motors.map(([n]) => n),
        values,
        0.3,
      ),
    );
  };
  const store = () => {
    const angles = getRobot().state?.angles;
    if (!descriptor || !angles || !userFile || !name.trim()) return;
    const pose: Pose = {
      folder: folder.trim(),
      name: name.trim(),
      description: "",
      bitmap: "",
      motors: descriptor.joints.map((j, i) => [
        j.name,
        toFile(j, angles[i] ?? 0),
      ]),
    };
    run(
      invoke<Pose[]>("poses_write", { path: userFile, pose }).then((poses) => {
        setLibs((l) => l.map((x) => (x.user ? { ...x, poses } : x)));
        setName("");
      }),
    );
  };
  const remove = (pose: Pose) => {
    if (poseBusy || !window.confirm(`Delete pose “${pose.name}” from your library? This cannot be undone.`)) return;
    run(
      invoke<Pose[]>("poses_write", {
        path: userFile,
        remove: [pose.folder, pose.name],
      }).then((poses) =>
        setLibs((l) => l.map((x) => (x.user ? { ...x, poses } : x))),
      ),
    );
  };
  const chooseLibrary = () => run(save({
    title: "User pose library", defaultPath: "poses.xap", filters: [{ name: "Pose library", extensions: ["xap"] }],
  }).then(f => { if (f) setUserFile(f); }));
  const properties = (pose: Pose) => {setError(undefined); setEditingPose({original: pose, value: {...pose}});};
  const backgroundActions = (): MenuAction[] => [
    {label: "Choose user library…", run: chooseLibrary, disabled: poseBusy, reason: "Wait for the current operation."},
    {label: "Store current pose…", run: () => poseName.current?.focus(), disabled: !status || !descriptor || !userFile || poseBusy, reason: poseBusy ? "Wait for the current operation." : !userFile ? "Choose a user library first." : "Connect to a robot first."},
  ];
  return (
    <div className="panel-column" tabIndex={0} {...menu.trigger(backgroundActions)}>
      {menu.element}
      <div className="pose-toolbar">
        <div className="pose-library">
          <span className={userFile ? undefined : "muted"} title={userFile || undefined}>
            {userFile ? userFile.split(/[\\/]/).pop() : "No user library"}
          </span>
          <button
            title="Choose or create the .xap file that stores your poses"
            disabled={poseBusy}
            onClick={chooseLibrary}
          >
            <FolderOpen size={13} />
            {userFile ? "Change…" : "Choose…"}
          </button>
        </div>
        <div className="pose-store">
          <input aria-label="New pose folder" placeholder="Folder" value={folder} onChange={e => setFolder(e.target.value)} />
          <input
            ref={poseName}
            aria-label="New pose name"
            placeholder="Pose name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <button
            disabled={!status || !userFile || !name.trim() || !folder.trim()}
            title={
              userFile
                ? "Store the robot's current angles"
                : "Choose a user library first"
            }
            onClick={store}
          >
            <Plus size={13} />
            Store
          </button>
        </div>
      </div>
      <div className="panel-scroll" role="list" aria-label="Poses">
        {!files.length && (
          <p className="panel-message">
            Configure the Choregraphe folder in Settings, or choose a user
            library.
          </p>
        )}
        {libs.flatMap((lib) =>
          [...new Set(lib.poses.map((p) => p.folder))].map((group) => (
            <div key={`${lib.file}|${group}`} role="presentation">
              <div className="tree-section pose-folder">
                {group}
                {lib.user && <span className="badge">User</span>}
              </div>
              {lib.poses.filter((p) => p.folder === group).map((p) => (
                <div
                  key={p.name}
                  className="pose-row"
                  role="listitem"
                  tabIndex={0}
                  {...menu.trigger(() => [
                    {label: "Apply pose", run: () => apply(p), disabled: !status || !descriptor, reason: "Connect to a robot first."},
                    ...(lib.user ? [
                      {label: "Properties…", run: () => properties(p), disabled: poseBusy, reason: "Wait for the current operation."},
                      {label: "Delete pose…", run: () => remove(p), disabled: poseBusy, reason: "Wait for the current operation."},
                    ] : []),
                  ])}
                >
                  <div className="pose-text">
                    <strong title={p.name}>{p.name}</strong>
                    {p.description && <small title={p.description}>{p.description}</small>}
                  </div>
                  <button
                    disabled={!status || !descriptor}
                    aria-label={`Apply ${p.name}`}
                    onClick={() => apply(p)}
                  >
                    Apply
                  </button>
                  {lib.user && <button className="icon-button" aria-label={`Properties of ${p.name}`} title="Properties" disabled={poseBusy} onClick={() => properties(p)}><Pencil size={13} /></button>}
                  {lib.user && (
                    <button
                      className="icon-button"
                      disabled={poseBusy}
                      aria-label={`Delete ${p.name}`}
                      title="Delete"
                      onClick={() => remove(p)}
                    >
                      <Trash2 size={13} />
                    </button>
                  )}
                </div>
              ))}
            </div>
          )),
        )}
        {error && <p className="panel-message error">{error}</p>}
      </div>
      {editingPose && <Modal title="Pose properties" onClose={() => !poseBusy && setEditingPose(undefined)}>
        <form className="settings-content" onSubmit={async e => {
          e.preventDefault(); if (poseBusy) return;
          setPoseBusy(true); setError(undefined);
          try {
            const poses = await invoke<Pose[]>("poses_write", {path: userFile, pose: editingPose.value, previous: [editingPose.original.folder, editingPose.original.name]});
            setLibs(l => l.map(x => x.user ? {...x, poses} : x)); setEditingPose(undefined);
          } catch (e) {setError(String(e));} finally {setPoseBusy(false);}
        }}>
          {([['folder', 'Folder'], ['name', 'Name'], ['description', 'Description'], ['bitmap', 'Image path']] as const).map(([key, label]) => <label key={key}>{label}<input aria-label={`Pose ${label.toLowerCase()}`} required={key === 'folder' || key === 'name'} disabled={poseBusy} value={editingPose.value[key]} onChange={e => setEditingPose({...editingPose, value: {...editingPose.value, [key]: e.target.value}})} /></label>)}
          <p>{editingPose.value.motors.length} stored joints. Changing properties preserves the recorded position.</p>
          {error && <p className="error" role="alert">{error}</p>}<button type="submit" disabled={poseBusy}>Save pose properties</button>
        </form>
      </Modal>}
    </div>
  );
}

/** Memory watcher: chosen ALMemory keys polled with getData. */
export function MemoryPanel() {
  const status = useRobot((s) => s.status);
  const [keys, setKeys] = useStored<string[]>("choren-memory-keys", []);
  const [values, setValues] = useState<Record<string, string>>({});
  const [key, setKey] = useState("");
  const [eventName, setEventName] = useState("");
  const [eventValue, setEventValue] = useState("1");
  const [eventError, setEventError] = useState("");
  const [sending, setSending] = useState(false);
  const [period, setPeriod] = useState(500);
  const [available, setAvailable] = useState<string[]>([]);
  const [recording, setRecording] = useState(false);
  const [samples, setSamples] = useState<{time: string; values: Record<string, string>}[]>([]);
  const keyInput = useRef<HTMLInputElement>(null);
  const menu = useContextMenu([status, keys, recording, samples.length > 0], e => setEventError(String(e)));
  const toggleRecording = () => {if (!recording) setSamples([]); setRecording(!recording);};
  const discover = () => robot.call("ALMemory", "getDataListName").then(v => {
    if (!Array.isArray(v) || v.some(x => typeof x !== "string")) throw new Error("Unexpected memory key list"); setAvailable(v.sort());
  }).catch(e => setEventError(String(e)));
  const exportSamples = async () => {try {
    const path = await save({defaultPath: "memory.json", filters: [{name: "Memory recording", extensions: ["json"]}]});
    if (path) await writeTextFile(path, JSON.stringify(samples, null, 2));
  } catch (e) {setEventError(String(e));}};
  useEffect(() => {
    setValues({});
    if (!status || !keys.length) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      const r = await Promise.all(
        keys.map((k) =>
          robot.call("ALMemory", "getData", k).then(
            (v) => [k, JSON.stringify(v)] as const,
            (e) => [k, `⚠ ${String(e)}`] as const,
          ),
        ),
      );
      if (!live) return;
      const next = Object.fromEntries(r);
      setValues(next);
      if (recording) setSamples(previous => previous.length >= 10000 ? previous : [...previous, {time: new Date().toISOString(), values: next}]);
      timer = setTimeout(() => void poll(), period);
    };
    void poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [status, keys, period, recording]);
  useEffect(() => {if (!status) setRecording(false);}, [status]);
  useEffect(() => {if (samples.length >= 10000) setRecording(false);}, [samples.length]);
  if (!native) return message("The memory watcher needs the desktop app.");
  return (
    <div className="panel-column" tabIndex={0} {...menu.trigger(() => [
      {label: "Watch key…", run: () => keyInput.current?.focus()},
      {label: "Discover memory keys", run: discover, disabled: !status, reason: "Connect to a robot first."},
      {label: recording ? "Stop recording memory" : "Record memory", run: toggleRecording, disabled: !status || !keys.length, reason: !status ? "Connect to a robot first." : "Watch a memory key first."},
      {label: "Export samples…", run: exportSamples, disabled: !samples.length, reason: "Record memory samples first."},
    ])}>
      {menu.element}
      <form
        className="panel-toolbar"
        onSubmit={(e) => {
          e.preventDefault();
          if (key.trim() && !keys.includes(key.trim()))
            setKeys([...keys, key.trim()]);
          setKey("");
        }}
      >
        <input
          ref={keyInput}
          list="memory-keys"
          aria-label="Memory key"
          placeholder="ALMemory key, e.g. Device/SubDeviceList/HeadYaw/Position/Sensor/Value"
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <button type="submit" disabled={!key.trim()}>
          <Plus size={13} />
          Watch
        </button>
      </form>
      <datalist id="memory-keys">{available.map(k => <option key={k} value={k} />)}</datalist>
      <div className="panel-toolbar">
        <button disabled={!status} onClick={() => void discover()}>Discover memory keys</button>
        <label>Period (ms)<input aria-label="Memory sampling period" type="number" min={100} max={60000} step={100} value={period} disabled={recording}
          onChange={e => setPeriod(Math.max(100, Math.min(60000, Number(e.target.value) || 100)))} /></label>
        <button disabled={!status || !keys.length} onClick={toggleRecording}>{recording ? "Stop recording memory" : "Record memory"}</button>
        <button disabled={!samples.length} onClick={() => void exportSamples()}>Export {samples.length} samples</button>
      </div>
      {eventError && !status?.virtual && <p className="error" role="alert">{eventError}</p>}
      {status?.virtual && (
        <form
          className="panel-toolbar memory-event"
          aria-label="Simulate memory event"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!eventName.trim() || sending) return;
            setEventError("");
            let value: unknown;
            try {
              value = JSON.parse(eventValue);
            } catch {
              setEventError('Enter a JSON value, such as 1, true, or "hello".');
              return;
            }
            setSending(true);
            try {
              await robot.call(
                "ALMemory",
                "raiseEvent",
                eventName.trim(),
                value,
              );
            } catch (e) {
              setEventError(String(e));
            } finally {
              setSending(false);
            }
          }}
        >
          <input
            aria-label="Event name"
            placeholder="Event name"
            value={eventName}
            onChange={(e) => setEventName(e.target.value)}
          />
          <input
            aria-label="Event value (JSON)"
            placeholder="JSON value"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            value={eventValue}
            onChange={(e) => setEventValue(e.target.value)}
          />
          <button type="submit" disabled={!eventName.trim() || sending}>
            {sending ? "Sending…" : "Raise event"}
          </button>
          {eventError && (
            <span className="error" role="alert">
              {eventError}
            </span>
          )}
        </form>
      )}
      <div className="panel-scroll">
        {!status && (
          <p className="panel-message">
            Connect to a robot to read its memory.
          </p>
        )}
        <table className="memory-table" aria-label="Watched memory keys">
          <tbody>
            {keys.map((k) => (
              <tr key={k} tabIndex={0} {...menu.trigger(() => [
                {label: "Stop watching", run: () => setKeys(keys.filter(x => x !== k))},
                {label: "Copy key", run: () => navigator.clipboard.writeText(k)},
                {label: "Copy value", run: () => navigator.clipboard.writeText(values[k] ?? ""), disabled: values[k] === undefined, reason: "Waiting for a value."},
              ])}>
                <td>{k}</td>
                <td>{values[k] ?? "…"}</td>
                <td>
                  <button
                    className="icon-button"
                    aria-label={`Stop watching ${k}`}
                    onClick={() => setKeys(keys.filter((x) => x !== k))}
                  >
                    <Trash2 size={13} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const CHAINS = ["Head", "LArm", "RArm", "LLeg", "RLeg"];

/**
 * Motion recording: relax chains, place joints (sliders, or drag them in the Robot view), and
 * store keys into the open timeline: on click, or every half second while recording.
 */
export function RecordingPanel() {
  const ws = useWorkspace();
  const status = useRobot((s) => s.status);
  const angles = useRobot((s) => s.state?.angles);
  const stiffness = useRobot((s) => s.state?.stiffness);
  const descriptor = useRobotDescriptor(status?.model);
  const [chains, setChains] = useState<string[]>(["Head"]);
  const [recordingScope, setRecordingScope] = useState<string>();
  const [interval, setIntervalMs] = useState(500);
  const [trigger, setTrigger] = useState("");
  const [extend, setExtend] = useState(true);
  const [storing, setStoring] = useState(false);
  const inFlight = useRef(false);
  const [error, setError] = useState<string>();
  const address = ws.state.activeBehavior
    ? ws.state.addresses[ws.state.activeBehavior]
    : undefined;
  const resolved = resolveAddress(ws.state.project, address);
  const t = resolved?.owner.timeline;
  // Same key as the Timeline panel, so the stored frame is the playhead's.
  const key =
    ws.state.project && address
      ? graphId(ws.state.project.id, {
          ...address,
          levels: [
            ...address.levels.slice(0, -1),
            { ...address.levels.at(-1)!, layer: 0, keyframe: 0 },
          ],
        })
      : "";
  const view = ws.state.timelines[key] ?? { frame: 1, scale: 8, scroll: 0 };
  const joints = (descriptor?.joints ?? []).filter((j) =>
    chains.includes(j.chain),
  );
  const target =
    ws.editable && t?.enabled && t.node !== undefined ? t : undefined;
  const scope = `${ws.state.project?.session}:${key}:${target?.node}`;
  const recording = !!status && recordingScope === scope;
  const ready =
    !!target &&
    !!joints.length &&
    !!angles &&
    joints.every((j) => Number.isFinite(angles[descriptor!.joints.indexOf(j)]));
  const menu = useContextMenu([status, scope, ws.history?.revision, recording, storing, ready, chains, view.frame], e => setError(String(e)));
  const toggleRecording = () => setRecordingScope(recording ? undefined : scope);
  const run = (p: Promise<unknown>) => {
    setError(undefined);
    return p.catch((e) => {
      setError(String(e));
      setRecordingScope(undefined);
    });
  };
  const storeAt = async (frame: number) => {
    const now = getRobot().state?.angles;
    if (!target || !now || !descriptor || !joints.length || inFlight.current)
      return false;
    if (
      !joints.every((j) => Number.isFinite(now[descriptor.joints.indexOf(j)]))
    ) {
      setError("Waiting for joint positions from the robot.");
      return false;
    }
    if (!extend && frame > target.size) {setError("Reached the timeline end. Enable Extend timeline to continue."); return false;}
    const values = joints.map((j) => {
      const i = descriptor.joints.indexOf(j);
      return [j.name, toFile(j, now[i] ?? 0), isHand(j)] as [
        string,
        number,
        boolean,
      ];
    });
    inFlight.current = true;
    setStoring(true);
    setError(undefined);
    try {
      const result = await ws.edit(
        "Store joints",
        [{ type: "recordKeys", timeline: target.node!, frame, values }],
        address!.behaviorId,
      );
      if (!result) setError("Could not store motion. Recording stopped.");
      return !!result;
    } finally {
      inFlight.current = false;
      setStoring(false);
    }
  };
  useEffect(() => {
    if (!recording || !target) {
      setRecordingScope(undefined);
      return;
    }
    let live = true;
    let frame = view.frame;
    const step = Math.max(1, Math.round(target.fps * interval / 1000));
    let pressed = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      if (trigger) {
        try {
          const value = await robot.call("ALMemory", "getData", trigger);
          if (!live) return;
          const down = Number(value) > 0.5;
          const rising = down && !pressed; pressed = down;
          if (!rising) {timer = setTimeout(() => void tick(), 50); return;}
        } catch (e) {if (live) {setError(String(e)); setRecordingScope(undefined);} return;}
      }
      const ok = await storeAt(frame);
      if (!live) return;
      if (!ok) {
        setRecordingScope(undefined);
        return;
      }
      frame += step;
      ws.saveTimeline(key, { ...view, frame });
      timer = setTimeout(() => void tick(), trigger ? 50 : interval);
    };
    timer = setTimeout(() => void tick(), trigger ? 50 : interval);
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // Freeze the selected joints and frame for this take; stop when its destination changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, scope, status, target?.fps, interval, trigger, extend]);
  if (!native) return message("Motion recording needs the desktop app.");
  if (!status) return message("Connect to a robot to record motion.");
  const place = (name: string, v: number) =>
    void run(
      status.virtual
        ? robot.call("ChorenSim", "setAngles", [name], [v])
        : robot.call("ALMotion", "setAngles", name, v, 0.2),
    );
  return (
    <div className="panel-column" tabIndex={0} {...menu.trigger(() => [
      {label: `Store at frame ${view.frame}`, run: () => storeAt(view.frame), disabled: !ready || recording || storing, reason: !target ? "Open a timeline in an editable project." : recording || storing ? "Wait for recording to finish." : "Select joints and wait for their positions."},
      {label: recording ? "Stop recording" : "Record motion", run: toggleRecording, disabled: !recording && (!ready || storing), reason: !target ? "Open a timeline in an editable project." : "Select joints and wait for their positions."},
      ...[0, 1].map(value => ({label: `Stiffness ${value ? "on" : "off"} for selected chains`, run: () => run(robot.call("ALMotion", "setStiffnesses", chains, value)), disabled: !chains.length, reason: "Select a chain first."})),
    ])}>
      {menu.element}
      <div
        className="panel-toolbar recording-toolbar"
        role="toolbar"
        aria-label="Recording"
      >
        <label>Step (ms)<input aria-label="Recording step" type="number" min={50} max={60000} value={interval} disabled={recording}
          onChange={e => setIntervalMs(Math.max(50, Math.min(60000, Number(e.target.value) || 50)))} /></label>
        <select aria-label="Recording trigger" value={trigger} disabled={recording} onChange={e => setTrigger(e.target.value)}>
          <option value="">Periodic</option><option value="FrontTactilTouched">Front head touch</option><option value="MiddleTactilTouched">Middle head touch</option><option value="RearTactilTouched">Rear head touch</option><option value="LeftBumperPressed">Left bumper</option><option value="RightBumperPressed">Right bumper</option>
        </select>
        <label className="checkbox"><input type="checkbox" checked={extend} disabled={recording} onChange={e => setExtend(e.target.checked)} />Extend timeline</label>
        {CHAINS.map((c) => (
          <label key={c}>
            <input
              type="checkbox"
              disabled={recording}
              checked={chains.includes(c)}
              onChange={(e) =>
                setChains(
                  e.target.checked
                    ? [...chains, c]
                    : chains.filter((x) => x !== c),
                )
              }
            />
            {c}
          </label>
        ))}
        <button
          disabled={!chains.length}
          onClick={() =>
            void run(robot.call("ALMotion", "setStiffnesses", chains, 0.0))
          }
        >
          Stiffness off
        </button>
        <button
          disabled={!chains.length}
          onClick={() =>
            void run(robot.call("ALMotion", "setStiffnesses", chains, 1.0))
          }
        >
          Stiffness on
        </button>
        <span className="spacer" />
        <button
          disabled={!ready || recording || storing}
          title={
            target
              ? `Store the chosen joints at frame ${view.frame}`
              : "Open a timeline box to store keys"
          }
          onClick={() => void storeAt(view.frame)}
        >
          Store at {view.frame}
        </button>
        <button
          disabled={!recording && (!ready || storing)}
          aria-pressed={recording}
          onClick={toggleRecording}
        >
          {recording ? <Square size={13} /> : <Circle size={13} />}
          {recording ? "Stop" : "Record"}
        </button>
      </div>
      <div className="panel-scroll recording-joints">
        {!target && (
          <p className="panel-message">
            Open a timeline box in an editable project to store keys.
          </p>
        )}
        {target && !ready && (
          <p className="panel-message">
            Select joints and wait for their positions before storing motion.
          </p>
        )}
        {recording && (
          <p className="panel-message" role="status">
            Recording {joints.length} joints · frame {view.frame}. Switching
            timelines stops this take.
          </p>
        )}
        {joints.map((j) => {
          const i = descriptor!.joints.indexOf(j);
          const relaxed = (stiffness?.[i] ?? 1) <= 0.01;
          return (
            <label key={j.name} className="joint-slider">
              <span>
                {j.name}
                {relaxed && <span className="muted"> · relaxed</span>}
              </span>
              <input
                type="range"
                aria-label={j.name}
                min={j.min}
                max={j.max}
                step={0.01}
                value={angles?.[i] ?? 0}
                onChange={(e) => place(j.name, Number(e.target.value))}
              />
              <output>
                {toFile(j, angles?.[i] ?? 0).toFixed(isHand(j) ? 2 : 1)}
                {isHand(j) ? "" : "°"}
              </output>
            </label>
          );
        })}
        {error && <p className="panel-message error">{error}</p>}
      </div>
    </div>
  );
}
