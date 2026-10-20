import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { native } from "../platform";
import { readPreference, savePreference } from "../workspace/storage";
import { getShareDir } from "../icons";

/** Robot connection as reported by the backend. */
export interface RobotStatus {
  url: string;
  model: string;
  virtual: boolean;
}
export interface RobotState {
  angles: number[];
  motionSpeed?: number;
  torsoPitch?: number;
  pose?: number[];
  stiffness?: number[];
  leds?: Record<string, number>;
}
export interface LogLine {
  level: string;
  category: string;
  message: string;
  time: number;
}
interface Store {
  status?: RobotStatus;
  busy: boolean;
  state?: RobotState;
  logs: LogLine[];
  speech: { text: string; time: number }[];
  running: string[];
  played?: { session: number; behavior: string; name: string; revision?: number };
  /** Play is uploading the package and starting the behavior. */
  starting: boolean;
  /** "behavior|1/4" → last stimulation time, for the diagram glow. */
  stimulated: Record<string, number>;
  frames: Record<string, number | null>;
  dialog: { speaker: "Human" | "Robot"; text: string }[];
  autoUpdateParameters: boolean;
  error?: string;
}

let store: Store = {
  busy: false,
  starting: false,
  logs: [],
  speech: [],
  running: [],
  stimulated: {},
  frames: {},
  dialog: [],
  autoUpdateParameters: readPreference("choren.auto-update-parameters") === true,
};
const listeners = new Set<() => void>();
/** The log with one more line; the app's own repeat of the line just before is dropped. */
function withLog(logs: LogLine[], level: string, category: string, message: string) {
  const last = logs.at(-1);
  if (category === "Choren" && last?.category === category && last.level === level && last.message === message && Date.now() - last.time < 2000) return logs;
  return [...logs.slice(-499), { level, category, message, time: Date.now() }];
}
const set = (patch: Partial<Store>) => {
  // An error the app shows is also kept in the robot log.
  if (patch.error && patch.error !== store.error)
    patch = { ...patch, logs: withLog(patch.logs ?? store.logs, "error", "Choren", patch.error.replace(/^Error: /, "")) };
  store = { ...store, ...patch };
  listeners.forEach((l) => l());
};
export const getRobot = () => store;
export function useRobot<T>(select: (s: Store) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => select(store),
  );
}

/** Event subscription that never rejects (the browser preview and tests have no event bridge). */
function on<T>(event: string, handler: (e: { payload: T }) => void) {
  listen<T>(event, handler).catch(() => {});
}

let pendingFrames: Store["frames"] = {};
let frameTimer: ReturnType<typeof setTimeout> | undefined;
function clearFrames() {
  clearTimeout(frameTimer); frameTimer = undefined; pendingFrames = {};
  set({ frames: {} });
}

/** Parameter values on the wire use their declared types, including false and zero. */
export function parameterValue(type: number, text: string): string | number | boolean {
  if (type === 0) return text === "1" || text === "true";
  if (type === 1 || type === 2) {
    const value = Number(text);
    if (!text.trim() || !Number.isFinite(value) || (type === 1 && !Number.isSafeInteger(value)))
      throw new Error(type === 1 ? "Enter a whole number." : "Enter a finite number.");
    return value;
  }
  return text;
}

let started = false;
/** Subscribes once to the backend's robot events. */
export function startRobotEvents() {
  if (started || !native) return;
  started = true;
  on<RobotState | number[]>("robot://state", (e) =>
    set({
      state: Array.isArray(e.payload) ? { angles: e.payload } : e.payload,
    }),
  );
  on<[string, string, string]>("robot://log", (e) => {
    const [level, category, message] = e.payload;
    set({ logs: withLog(store.logs, level, category, message) });
  });
  on<[string]>("robot://speech", (e) =>
    set({
      speech: [
        ...store.speech.slice(-19),
        { text: e.payload[0], time: Date.now() },
      ],
    }),
  );
  on<[string, string]>("robot://behavior", (e) => {
    const [name, what] = e.payload;
    const running =
      what === "started"
        ? [...new Set([...store.running, name])]
        : what === "stopped"
          ? store.running.filter((r) => r !== name)
          : store.running;
    set({
      running,
      error: what.startsWith("error") ? what.slice(7) : store.error,
    });
  });
  on<[string, number[], number | null]>("robot://frame", ({ payload: [name, path, frame] }) => {
    if (!store.status) return;
    pendingFrames[`${name}|${path.join("/")}`] = frame;
    frameTimer ??= setTimeout(() => {
      set({ frames: { ...store.frames, ...pendingFrames } });
      pendingFrames = {}; frameTimer = undefined;
    }, 100);
  });
  for (const [event, speaker] of [["robot://dialog-input", "Human"], ["robot://dialog-answer", "Robot"]] as const)
    on<[string]>(event, ({ payload: [text] }) => {
      if (store.status && typeof text === "string") set({ dialog: [...store.dialog.slice(-99), { speaker, text }] });
    });
  let pending: Record<string, number> = {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  on<[string, number[], string]>("robot://stimulated", (e) => {
    const [name, path] = e.payload;
    pending[`${name}|${path.join("/")}`] = Date.now();
    // Glow updates are batched to ~10 Hz.
    timer ??= setTimeout(() => {
      const now = Date.now();
      const kept = Object.fromEntries(
        Object.entries(store.stimulated).filter(([, t]) => now - t < 1000),
      );
      set({ stimulated: { ...kept, ...pending } });
      pending = {};
      timer = undefined;
      setTimeout(() => set({}), 700); // lets the glow fade when nothing else changes
    }, 100);
  });
  on("robot://disconnected", () => {
    clearFrames(); pending = {};
    set({ status: undefined, state: undefined, running: [], speech: [], dialog: [], stimulated: {}, played: undefined });
  });
  // A webview reload can leave the backend's robot connected.
  void invoke<RobotStatus | null>("robot_status")
    .then((status) => {
      if (status && !store.status && !store.busy) set({ status });
    })
    .catch(() => {});
}

async function connecting(work: () => Promise<RobotStatus>) {
  if (store.busy || store.status) return;
  clearFrames();
  set({
    dialog: [],
    busy: true,
    error: undefined,
    state: undefined,
    running: [],
    speech: [],
    stimulated: {},
    played: undefined,
  });
  try {
    set({ status: await work(), busy: false });
  } catch (e) {
    set({ busy: false, error: String(e) });
  }
}

export function inputActivationError(session: number, behavior: string, revision?: number) {
  if (!store.status) return "Connect to a robot before activating an input.";
  const played = store.played;
  if (!played || played.session !== session || played.behavior !== behavior || !store.running.includes(played.name))
    return "The behavior is not running. Play this behavior before activating an input.";
  if (played.revision !== revision)
    return "The diagram changed since Play. Play it again before activating an input.";
}

export const robot = {
  // With the install folder, the virtual robot uses its full posture graph.
  startVirtual: (model = "nao6") =>
    connecting(() =>
      invoke<RobotStatus>("robot_start_virtual", {
        model,
        shareDir: getShareDir() || null,
      }),
    ),
  connect: (url: string) =>
    connecting(() => invoke<RobotStatus>("robot_connect", { url })),
  disconnect: async () => {
    if (store.busy) return;
    set({ busy: true, error: undefined });
    try {
      await invoke("robot_disconnect");
      clearFrames();
      set({ status: undefined, state: undefined, running: [], speech: [], dialog: [], stimulated: {}, played: undefined });
    } finally {
      set({ busy: false });
    }
  },
  models: () => invoke<{ id: string; name: string }[]>("robot_models"),
  /** Robots announcing NAOqi on the local network (mDNS), for the connection dialog. */
  discover: () => invoke<{ name: string; url: string }[]>("robot_discover"),
  call: (service: string, method: string, ...args: unknown[]) =>
    invoke<unknown>("robot_call", { service, method, args }),
  setMotionSpeed: async (speed: number) => {
    const status = store.status;
    if (!status?.virtual) throw new Error("Movement speed is available on the virtual robot.");
    await robot.call("ChorenSim", "setMotionSpeed", speed);
    if (store.status === status)
      set({ state: { angles: [], ...store.state, motionSpeed: speed } });
  },
  play: async (session: number, behavior: string, revision?: number) => {
    if (store.starting) return;
    clearFrames();
    set({ error: undefined, played: undefined, starting: true });
    const status = store.status;
    try {
      const name = await invoke<string>("robot_play", { session, behavior });
      if (store.status === status) set({ played: { session, behavior, name, revision } });
      return name;
    } finally {
      set({ starting: false });
    }
  },
  activateInput: async (session: number, behavior: string, revision: number | undefined, path: number[][], input: number, value: unknown) => {
    const error = inputActivationError(session, behavior, revision);
    if (error) throw new Error(error);
    // ChorenSim.activateInput on Choren's simulator, ALFrameManager.callBoxInput on a real robot.
    return invoke<unknown>("robot_activate_input", { session, behavior, path, input, value });
  },
  /** "Auto-update parameters on robot": new values for a box of the playing behavior. */
  setParameters: async (session: number, behavior: string, path: number[][], values: [string, unknown][], revision?: number, nextRevision?: number) => {
    const error = inputActivationError(session, behavior, revision);
    if (error) throw new Error(error);
    const played = store.played;
    await invoke("robot_set_parameters", { session, behavior, path, values });
    // Only a successful, consecutive parameter edit keeps the running diagram in sync.
    if (store.played === played && revision !== undefined && nextRevision === revision + 1)
      set({ played: { ...played!, revision: nextRevision } });
  },
  setAutoUpdateParameters: (enabled: boolean) => {
    savePreference("choren.auto-update-parameters", enabled);
    set({ autoUpdateParameters: enabled });
  },
  clearDialog: () => set({ dialog: [] }),
  stop: async () => { await invoke("robot_stop"); clearFrames(); set({ played: undefined }); },
  refreshRunning: async () => {
    const status = store.status;
    if (!status) return;
    const running = await invoke<unknown>("robot_call", {
      service: "ALBehaviorManager",
      method: "getRunningBehaviors",
      args: [],
    });
    if (
      store.status === status &&
      Array.isArray(running) &&
      running.every((name) => typeof name === "string") &&
      JSON.stringify(running) !== JSON.stringify(store.running)
    )
      set({ running });
  },
  descriptor: (model: string) =>
    invoke<RobotDescriptor>("robot_descriptor", { model }),
  /** An install mesh (`descriptor.meshes`) in `ogre::to_bytes` layout. */
  mesh: (shareDir: string, model: string, name: string) =>
    invoke<ArrayBuffer>("robot_mesh", { shareDir, model, name }),
  clearLogs: () => set({ logs: [] }),
  /** Adds a line from Choren itself (category "Choren") to the robot log. */
  log: (level: string, message: string) => set({ logs: withLog(store.logs, level, "Choren", message.replace(/^Error: /, "")) }),
  reportError: (error: string) => set({ error }),
  clearError: () => set({ error: undefined }),
};

export function useRobotDescriptor(model?: string) {
  const [descriptor, setDescriptor] = useState<RobotDescriptor>();
  useEffect(() => {
    let live = true;
    setDescriptor(undefined);
    if (native && model)
      void robot.descriptor(model).then(
        (value) => {
          if (live) setDescriptor(value);
        },
        (error) => {
          if (live) robot.reportError(String(error));
        },
      );
    return () => {
      live = false;
    };
  }, [model]);
  return descriptor;
}

export interface RobotDescriptor {
  id: string;
  name: string;
  bodyNames: string[];
  joints: {
    name: string;
    parent: string | null;
    origin: [number, number, number];
    axis: [number, number, number];
    min: number;
    max: number;
    chain: string;
  }[];
  postures: { name: string; family: string }[];
  touch: { name: string; event: string; link: string | null }[];
  tips: Record<string, [string, [number, number, number]]>;
  /** Link (joint name or "Torso") → install mesh name. */
  meshes?: Record<string, string>;
}
