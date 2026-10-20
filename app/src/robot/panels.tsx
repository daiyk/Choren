import { useState } from "react";
import {
  Bot,
  FolderUp,
  Hand,
  MessageSquare,
  Play,
  Plug,
  Power,
  Square,
  Unplug,
} from "lucide-react";
import { native } from "../platform";
import { RobotView } from "./RobotView";
import { robot, useRobot, useRobotDescriptor } from "./store";
import { useWorkspace } from "../workspace/context";

export function VirtualMovementSpeed() {
  const status = useRobot((s) => s.status);
  const busy = useRobot((s) => s.busy);
  const motionSpeed = useRobot((s) => s.state?.motionSpeed ?? 1);
  const [changingSpeed, setChangingSpeed] = useState(false);
  const [error, setError] = useState("");
  if (!status?.virtual) return null;
  return <div>
    <label className="setting">
      <span>
        Virtual movement speed
        <small id="movement-speed-help">Applies immediately to movement and animation timelines. Wait boxes and speech keep their timing.</small>
      </span>
      <select
        aria-label="Virtual movement speed"
        aria-describedby="movement-speed-help"
        value={motionSpeed}
        disabled={busy || changingSpeed}
        onChange={async (e) => {
          setChangingSpeed(true);
          setError("");
          robot.clearError();
          try {
            await robot.setMotionSpeed(Number(e.target.value));
          } catch (e) {
            setError(String(e));
            robot.reportError(String(e));
          } finally {
            setChangingSpeed(false);
          }
        }}
      >
        {[0.25, 0.5, 0.75, 1, 1.25, 1.5, 2].map((speed) =>
          <option key={speed} value={speed}>{speed}×{speed === 1 ? " (Normal)" : ""}</option>
        )}
      </select>
    </label>
    {error && <p className="error" role="alert">{error}</p>}
  </div>;
}

/** Robot view: connection, 3-D model, posture and stiffness controls, simulated sensors. */
export function RobotPanel() {
  const [display, setDisplay] = useState({world: true, position: true, motors: true});
  const ws = useWorkspace();
  const status = useRobot((s) => s.status);
  const busy = useRobot((s) => s.busy);
  const error = useRobot((s) => s.error);
  const speech = useRobot((s) => s.speech);
  const running = useRobot((s) => s.running);
  const descriptor = useRobotDescriptor(status?.model);
  const [heard, setHeard] = useState("");
  const [posture, setPosture] = useState("StandInit");
  const [sending, setSending] = useState(false);
  const run = (p: Promise<unknown>) => {
    robot.clearError();
    void p.catch((e) => robot.reportError(String(e)));
  };
  const play = ws.commands.find((c) => c.id === "run");
  const stop = ws.commands.find((c) => c.id === "stop");
  const selectedPosture = descriptor?.postures.some((p) => p.name === posture)
    ? posture
    : (descriptor?.postures[0]?.name ?? "");
  if (!native)
    return (
      <div className="robot-panel panel-scroll">
        <p className="muted">The robot view needs the desktop app.</p>
      </div>
    );
  if (!status)
    return (
      <div className="robot-panel panel-scroll">
        <div className="inspector-heading">
          <Bot size={18} />
          <h2>Robot</h2>
        </div>
        <p className="muted">
          Run behaviors, pose joints, and simulate touch and speech with a
          virtual NAO.
        </p>
        <button
          className="wide-button"
          disabled={busy}
          onClick={() => void robot.startVirtual("nao6")}
        >
          <Power size={15} />
          {busy ? "Starting…" : "Start virtual NAO 6"}
        </button>
        <button className="wide-button" onClick={() => ws.execute("connect-robot")}>
            <Plug size={14} />
            Robot connection…
        </button>
        <button className="wide-button" onClick={() => ws.showPanel("files")}><FolderUp size={14} />Robot files…</button>
        {error && <p className="panel-message error" role="alert">{error}</p>}
      </div>
    );
  return (
    <div className="robot-panel">
      <div className="panel-toolbar">
        <span className="robot-connection-label" title={status.url}>
          {status.virtual ? "Virtual " : ""}
          {descriptor?.name ?? status.model} · {status.url}
        </span>
        <span className="spacer" />
        <button className="icon-button" aria-label="Robot files" title="Robot files" onClick={() => ws.showPanel("files")}><FolderUp size={15} /></button>
        <button className="icon-button" aria-label="Robot connection" title="Robot connection" onClick={() => ws.execute("connect-robot")}><Plug size={15} /></button>
        <button
          className="icon-button"
          aria-label="Disconnect"
          title="Disconnect"
          disabled={busy}
          onClick={() => run(robot.disconnect())}
        >
          <Unplug size={15} />
        </button>
      </div>
      <details className="robot-display"><summary>Display</summary><div className="panel-toolbar">
        {([['world', 'World grid'], ['position', 'Robot position'], ['motors', 'Motor updates']] as const).map(([key, name]) => <label className="checkbox" key={key}><input type="checkbox" checked={display[key]} onChange={e => setDisplay({...display, [key]: e.target.checked})} />{name}</label>)}
      </div></details>
      {descriptor && (
        <RobotView
          descriptor={descriptor}
          options={display}
          onDrag={
            status.virtual
              ? (joint, angle) =>
                  run(robot.call("ChorenSim", "setAngles", [joint], [angle]))
              : undefined
          }
        />
      )}
      <div className="robot-controls panel-scroll">
        <section className="robot-section" aria-label="Behavior execution">
          <div className="robot-row">
            <strong>
              {ws.state.project?.behaviors.find(
                (b) => b.id === ws.state.activeBehavior,
              )?.name ?? "Behavior execution"}
            </strong>
            <span className="spacer" />
            <button
              disabled={!play?.enabled}
              title={
                !play?.enabled
                  ? play?.reason
                  : "Save and run the active behavior"
              }
              onClick={() => ws.execute("run")}
            >
              <Play size={13} />
              Play
            </button>
            <button
              disabled={!stop?.enabled}
              onClick={() => ws.execute("stop")}
            >
              <Square size={13} />
              Stop
            </button>
          </div>
          <p className="muted robot-runtime" role="status">
            {running.length ? `Running: ${running.join(", ")}` : "Robot ready"}
          </p>
        </section>
        <section className="robot-section" aria-label="Robot posture">
          <div className="robot-row">
            <button onClick={() => run(robot.call("ALMotion", "wakeUp"))}>
              Wake up
            </button>
            <button onClick={() => run(robot.call("ALMotion", "rest"))}>
              Rest
            </button>
            <select
              aria-label="Posture"
              value={selectedPosture}
              disabled={!descriptor?.postures.length}
              onChange={(e) => setPosture(e.target.value)}
            >
              {descriptor?.postures.map((p) => (
                <option key={p.name}>{p.name}</option>
              ))}
            </select>
            <button
              disabled={!selectedPosture}
              onClick={() =>
                run(
                  robot.call(
                    "ALRobotPosture",
                    "goToPosture",
                    selectedPosture,
                    0.8,
                  ),
                )
              }
            >
              Go
            </button>
          </div>
          <div className="robot-row robot-tools">
            <button onClick={() => ws.showPanel("recording")}>
              Joints & recording
            </button>
            <button onClick={() => ws.showPanel("poses")}>Poses</button>
            <button onClick={() => ws.showPanel("memory")}>
              Memory & events
            </button>
            <button onClick={() => ws.showPanel("logs")}>Logs</button>
          </div>
        </section>
        {status.virtual && descriptor && (
          <details className="robot-section">
            <summary>
              <Hand size={14} />
              Simulated touch sensors
            </summary>
            <div className="robot-row" aria-label="Simulated touch sensors">
              {descriptor.touch.map((t) => (
                <button
                  key={t.name}
                  title={t.event}
                  onClick={() => run(robot.call("ChorenSim", "press", t.name))}
                >
                  {t.name.replace(/([a-z])([A-Z])/g, "$1 $2")}
                </button>
              ))}
            </div>
          </details>
        )}
        {status.virtual && (
          <form
            className="robot-row"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!heard.trim() || sending) return;
              setSending(true);
              robot.clearError();
              try {
                await robot.call("ChorenSim", "hear", heard.trim());
                setHeard("");
              } catch (e) {
                robot.reportError(String(e));
              } finally {
                setSending(false);
              }
            }}
          >
            <MessageSquare size={14} />
            <input
              aria-label="Say to the robot"
              placeholder="Say to the robot…"
              value={heard}
              disabled={sending}
              onChange={(e) => setHeard(e.target.value)}
            />
            <button type="submit" disabled={!heard.trim() || sending}>
              {sending ? "Sending…" : "Send speech"}
            </button>
          </form>
        )}
        {speech.length > 0 && (
          <div className="robot-speech" aria-live="polite">
            {speech.slice(-4).map((s, i) => (
              <p key={`${s.time}:${i}`}>“{s.text}”</p>
            ))}
          </div>
        )}
        {error && (
          <p className="panel-message error" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

/** Severity order and the short tag each line starts with. */
const LOG_LEVELS: [string, string][] = [["fatal", "FATAL"], ["error", "ERROR"], ["warning", "WARN"], ["info", "INFO"], ["verbose", "VERB"], ["debug", "DEBUG"]];
const logTag = (level: string) => LOG_LEVELS.find(([l]) => l === level)?.[1] ?? level.toUpperCase();

export function LogsPanel() {
  const logs = useRobot((s) => s.logs);
  const status = useRobot((s) => s.status);
  const [filter, setFilter] = useState("");
  const [level, setLevel] = useState("all");
  const shown = logs.filter(l => (level === "all" || l.level === level) && `${l.category} ${l.message}`.toLowerCase().includes(filter.toLowerCase()));
  if (!logs.length)
    return (
      <div className="panel-scroll">
        <p className="panel-message">
          {status
            ? "No robot messages yet."
            : "Connect to a robot to see its log."}
        </p>
      </div>
    );
  return (
    <div className="panel-column">
      <div className="panel-toolbar">
        <input aria-label="Filter robot logs" placeholder="Filter category or message" value={filter} onChange={e => setFilter(e.target.value)} />
        <select aria-label="Log severity" value={level} onChange={e => setLevel(e.target.value)}>
          <option value="all">All levels</option>
          {[...LOG_LEVELS.map(([l]) => l), ...new Set(logs.map(l => l.level).filter(l => !LOG_LEVELS.some(([k]) => k === l)))]
            .map(l => [l, logs.filter(x => x.level === l).length] as const)
            .filter(([l, n]) => n || l === level)
            .map(([l, n]) => <option key={l} value={l}>{logTag(l)} ({n})</option>)}
        </select>
        <span>{shown.length} / {logs.length} messages</span>
        <span className="spacer" />
        <button onClick={robot.clearLogs}>Clear</button>
      </div>
      <div className="robot-logs panel-scroll" role="log">
        {shown.map((l, i) => (
          <div key={i} className="log-line" data-level={l.level}>
            <time>{new Date(l.time).toLocaleTimeString()}</time>
            <span className="log-level">{logTag(l.level)}</span>
            <span className="log-category" title={l.category}>{l.category}</span>
            <span className="log-message">{l.message}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
