//! Robot connection for the editor. The editor is a libqi client: it talks to the in-app virtual
//! robot (choren-naoqi on 127.0.0.1) the same way it talks to a real NAO.

use crate::project::Sessions;
use choren_naoqi::runtime::{json_to_value, value_to_json, PythonConfig};
use choren_qi::{Client, Value};
use serde_json::{json, Value as Json};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct RobotState {
    inner: Mutex<Option<Connection>>,
}

struct Connection {
    client: Arc<Client>,
    url: String,
    model: String,
    /// Present when the robot is Choren's in-app virtual robot.
    virtual_robot: Option<choren_naoqi::VirtualRobot>,
    /// The behavior Play started last (`<uuid>/<folder>`), and on a real robot the task relaying
    /// its debugger.
    playing: Option<String>,
    debugger: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Its frame manager id (`<behavior id>:/`) on a real robot, for `callBoxInput`.
    frame_manager: Option<String>,
    /// What that run loaded on a real robot, for the editor's input stimulation.
    loaded: Loaded,
    /// Choren's simulator (in-app or not), which reports box activity through `ChorenSim`.
    simulated: bool,
    /// A Play is uploading or starting; a second Play is refused meanwhile.
    starting: bool,
    /// Counts Stops, so a Stop during a Play's upload keeps that Play from starting the robot.
    stops: u64,
    tasks: Vec<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        for t in self.tasks.iter().chain(&self.debugger) {
            t.abort();
        }
    }
}

#[tauri::command]
pub fn robot_models() -> Vec<Json> {
    choren_robot::MODELS.iter().filter_map(|id| choren_robot::Robot::load(id).ok()).map(|r| json!({"id": r.id, "name": r.name})).collect()
}

#[tauri::command]
pub fn robot_descriptor(model: String) -> Result<choren_robot::Robot, String> {
    choren_robot::Robot::load(&model)
}

fn python_config(app: &AppHandle) -> PythonConfig {
    let mut cfg = PythonConfig::default();
    if let Ok(dir) = app.path().resolve("python", tauri::path::BaseDirectory::Resource) {
        if dir.join("choren_host").is_dir() {
            cfg.dir = dir;
        }
    }
    // The CPython bundled by scripts/fetch-python-runtime.sh; `python3` from PATH in dev.
    let exe = if cfg!(windows) { "python-runtime/python.exe" } else { "python-runtime/bin/python3.12" };
    if let Ok(exe) = app.path().resolve(exe, tauri::path::BaseDirectory::Resource) {
        if exe.is_file() {
            cfg.exe = exe.to_string_lossy().into_owned();
        }
    }
    cfg
}

/// Forwards a robot signal to the UI as a Tauri event.
fn forward(app: &AppHandle, client: &Arc<Client>, service: &'static str, signal: &'static str, event: &'static str) -> tauri::async_runtime::JoinHandle<()> {
    let (app, client) = (app.clone(), client.clone());
    tauri::async_runtime::spawn(async move {
        let Ok(mut rx) = client.subscribe(service, signal).await else { return };
        while let Some(v) = rx.recv().await {
            let _ = app.emit(event, value_to_json(&v));
        }
    })
}

/// A real robot's log, as Choregraphe's log viewer reads it: `LogManager.getListener()`,
/// `clearFilters()`, `setCategory("*", debug)`, then `onLogMessage`, sent to the UI as
/// `robot://log` `[level, category, message]` like the virtual robot's.
fn robot_logs(app: &AppHandle, client: &Arc<Client>) -> tauri::async_runtime::JoinHandle<()> {
    let (app, client) = (app.clone(), client.clone());
    tauri::async_runtime::spawn(async move {
        let Ok(Value::Object(o)) = client.call("LogManager", "getListener", vec![]).await else { return };
        let Some(listener) = o.remote() else { return };
        let _ = listener.call("clearFilters", vec![]).await;
        let _ = listener.call("setCategory", vec!["*".into(), Value::Int(6)]).await;
        let Ok(mut rx) = listener.subscribe("onLogMessage").await else { return };
        while let Some(v) = rx.recv().await {
            let m = v.items().first().map(Value::items).unwrap_or_default();
            let level = ["fatal", "error", "warning", "info", "verbose", "debug"].get(m.get(1).and_then(Value::as_i64).unwrap_or(4).clamp(1, 6) as usize - 1).copied().unwrap_or("info");
            let text = |i: usize| m.get(i).and_then(Value::as_str).unwrap_or("").to_string();
            let _ = app.emit("robot://log", json!([level, text(2), text(4)]));
        }
    })
}

/// Calls `on` with each new value of an ALMemory key (`ALMemory.subscriber(key)` → `signal`).
fn watch_memory(app: &AppHandle, client: &Arc<Client>, key: &'static str, mut on: impl FnMut(&AppHandle, &Value) + Send + 'static) -> tauri::async_runtime::JoinHandle<()> {
    let (app, client) = (app.clone(), client.clone());
    tauri::async_runtime::spawn(async move {
        let Ok(Value::Object(o)) = client.call("ALMemory", "subscriber", vec![key.into()]).await else { return };
        let Some(subscriber) = o.remote() else { return };
        let Ok(mut rx) = subscriber.subscribe("signal").await else { return };
        while let Some(v) = rx.recv().await {
            on(&app, v.items().first().unwrap_or(&v));
        }
    })
}

/// `robot://behavior` events for a change of the running list (ALMemory `BehaviorsRun`).
fn running_changes(before: &[String], now: &[String]) -> Vec<Json> {
    let started = now.iter().filter(|n| !before.contains(n)).map(|n| json!([n, "started"]));
    started.chain(before.iter().filter(|b| !now.contains(b)).map(|b| json!([b, "stopped"]))).collect()
}

/// What the editor watches on a real robot: `BehaviorsRun` for the running behaviors and
/// `ALTextToSpeech/CurrentSentence` for speech, sent like the simulator's `behavior`/`spoken`.
fn robot_activity(app: &AppHandle, client: &Arc<Client>) -> [tauri::async_runtime::JoinHandle<()>; 2] {
    let mut running: Vec<String> = vec![];
    [
        watch_memory(app, client, "BehaviorsRun", move |app, v| {
            let now: Vec<String> = v.items().iter().filter_map(|n| n.as_str().map(str::to_string)).collect();
            for change in running_changes(&running, &now) {
                let _ = app.emit("robot://behavior", change);
            }
            running = now;
        }),
        watch_memory(app, client, "ALTextToSpeech/CurrentSentence", |app, v| {
            if let Some(text) = v.as_str().filter(|t| !t.is_empty()) {
                let _ = app.emit("robot://speech", json!([text]));
            }
        }),
    ]
}

async fn attach(app: &AppHandle, client: Client, url: String, virtual_robot: Option<choren_naoqi::VirtualRobot>) -> Result<Json, String> {
    let client = Arc::new(client);
    let simulated = client.services().await?.iter().any(|s| s == "ChorenSim");
    let model = if simulated { client.call("ChorenSim", "robot", vec![]).await?.as_str().unwrap_or("nao6").to_string() } else { "nao6".into() };
    let mut tasks = vec![];
    if !simulated {
        tasks.push(robot_logs(app, &client));
        tasks.extend(robot_activity(app, &client));
    }
    if simulated {
        tasks.push(forward(app, &client, "ChorenSim", "log", "robot://log"));
        tasks.push(forward(app, &client, "ChorenSim", "spoken", "robot://speech"));
        tasks.push(forward(app, &client, "ChorenSim", "behavior", "robot://behavior"));
        tasks.push(forward(app, &client, "ChorenSim", "stimulated", "robot://stimulated"));
    }
    tasks.push(forward(app, &client, "ALDialog", "answered", "robot://dialog-answer"));
    tasks.push(forward(app, &client, "ALDialog", "lastInput", "robot://dialog-input"));
    // Joint state for the 3-D view at ~30 Hz.
    let (a, c) = (app.clone(), client.clone());
    tasks.push(tauri::async_runtime::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_millis(33));
        loop {
            every.tick().await;
            let state = if simulated {
                c.call("ChorenSim", "getState", vec![]).await.map(|v| value_to_json(&v))
            } else {
                c.call("ALMotion", "getAngles", vec!["Body".into(), true.into()]).await.map(|v| json!({"angles": value_to_json(&v)}))
            };
            match state {
                Ok(s) => {
                    let _ = a.emit("robot://state", s);
                }
                Err(_) => {
                    let _ = a.emit("robot://disconnected", ());
                    return;
                }
            }
        }
    }));
    let info = json!({"url": url, "model": model, "virtual": virtual_robot.is_some(), "simulated": simulated});
    let state = app.state::<RobotState>();
    *state.inner.lock().await = Some(Connection { client, url, model, virtual_robot, playing: None, debugger: None, frame_manager: None, loaded: Loaded::default(), simulated, starting: false, stops: 0, tasks });
    Ok(info)
}

/// A file of the Choregraphe install next to its granted `share/choregraphe` folder
/// (e.g. `alrobotmodel/meshes/x.mesh`). Only the share folder itself must be granted.
fn install_file(app: &AppHandle, share_dir: &str, rel: &str) -> Result<std::path::PathBuf, String> {
    let share = std::path::Path::new(share_dir);
    crate::library::readable(app, share)?;
    Ok(share.parent().ok_or("Invalid share folder")?.join(rel))
}

/// Starts the in-app virtual robot (port 9559 when free) and connects to it. With the install's
/// share folder, ALRobotPosture uses the install's full posture graph.
#[tauri::command]
pub async fn robot_start_virtual(app: AppHandle, state: State<'_, RobotState>, model: String, share_dir: Option<String>) -> Result<Json, String> {
    state.inner.lock().await.take();
    let python = python_config(&app);
    let vr = match choren_naoqi::start(&model, "127.0.0.1:9559", Some(python.clone())).await {
        Ok(vr) => vr,
        Err(_) => choren_naoqi::start(&model, "127.0.0.1:0", Some(python)).await?,
    };
    let graph = vr.robotd.sim.robot.install_postures.clone();
    if let (Some(share), Some(graph)) = (share_dir.as_deref().filter(|s| !s.is_empty()), graph) {
        let loaded = install_file(&app, share, &format!("naoqi/robot_posture/{graph}")).and_then(|f| vr.robotd.load_posture_graph(&f));
        if let Err(e) = loaded {
            vr.robotd.log("warning", "ALRobotPosture", &format!("Using the built-in postures: {e}"));
        }
    }
    let url = format!("tcp://{}", vr.addr);
    let client = Client::connect(&url).await?;
    attach(&app, client, url, Some(vr)).await
}

/// Robots announcing NAOqi on the local network (mDNS `_naoqi._tcp`), for the connection dialog.
#[tauri::command]
pub async fn robot_discover() -> Result<Vec<Json>, String> {
    let robots = choren_qi::discovery::discover(std::time::Duration::from_millis(1500)).await?;
    Ok(robots.into_iter().map(|(name, url)| json!({"name": name, "url": url})).collect())
}

/// Connects to a robot at `url` (`tcp://host:port` or `host`, then on port 9561).
#[tauri::command]
pub async fn robot_connect(app: AppHandle, state: State<'_, RobotState>, url: String) -> Result<Json, String> {
    state.inner.lock().await.take();
    let url = if url.contains(':') { url } else { format!("{url}:9561") };
    let client = tokio::time::timeout(std::time::Duration::from_secs(5), Client::connect(&url)).await.map_err(|_| format!("No answer from {url}"))??;
    attach(&app, client, url, None).await
}

/// An install mesh of the robot (`descriptor.meshes`), as `choren_robot::ogre::to_bytes`.
#[tauri::command]
pub fn robot_mesh(app: AppHandle, share_dir: String, model: String, name: String) -> Result<tauri::ipc::Response, String> {
    let robot = choren_robot::Robot::load(&model)?;
    if !robot.meshes.values().any(|m| *m == name) {
        return Err(format!("{name} is not a mesh of {model}"));
    }
    let dir = install_file(&app, &share_dir, "alrobotmodel/meshes")?;
    let bytes = std::fs::read(dir.join(format!("{name}.mesh"))).map_err(|e| format!("Cannot read mesh {name}: {e}"))?;
    let mesh = choren_robot::ogre::parse(&bytes).map_err(|e| format!("{name}.mesh: {e}"))?;
    let mut colors = std::fs::read_to_string(dir.join("nao.material")).map(|m| choren_robot::ogre::material_colors(&m)).unwrap_or_default();
    for eye in ["NaoMat_Leye", "NaoMat_Reye"] {
        if !colors.iter().any(|c| c.0 == eye) {
            colors.push((eye.into(), 0xffffff)); // the eye materials have no colour of their own
        }
    }
    // The top byte marks the parts the view colours from the LEDs: 1 left eye, 2 right eye, 3 chest.
    for (material, rgb) in &mut colors {
        *rgb |= match material.as_str() {
            "NaoMat_Leye" => 1 << 24,
            "NaoMat_Reye" => 2 << 24,
            m if m.starts_with("NaoMat_Buton") => 3 << 24,
            _ => 0,
        };
    }
    Ok(tauri::ipc::Response::new(choren_robot::ogre::to_bytes(&mesh, &colors)))
}

#[tauri::command]
pub async fn robot_disconnect(state: State<'_, RobotState>) -> Result<(), String> {
    state.inner.lock().await.take();
    Ok(())
}

async fn client(state: &RobotState) -> Result<Arc<Client>, String> {
    state.inner.lock().await.as_ref().map(|c| c.client.clone()).ok_or_else(|| "Not connected to a robot".to_string())
}

/// Calls any robot service method (used by the robot view: postures, wake up, touch, speech input).
#[tauri::command]
pub async fn robot_call(state: State<'_, RobotState>, service: String, method: String, args: Vec<Json>) -> Result<Json, String> {
    let c = client(&state).await?;
    let v = c.call(&service, &method, args.iter().map(json_to_value).collect()).await?;
    Ok(value_to_json(&v))
}

/// Uploads the project as a package and starts the behavior, like Choregraphe's Play:
/// `PackageManager._install(qi::File, installer, md5)` on the virtual and on real robots. On a real
/// robot the behavior debugger (`ALFrameManager.getBehaviorDebuggerFor`) lights the boxes up
/// (`robot://stimulated`) and moves timeline playheads (`robot://frame`).
#[tauri::command]
pub async fn robot_play(app: AppHandle, state: State<'_, RobotState>, sessions: State<'_, Sessions>, session: u32, behavior: String) -> Result<String, String> {
    let (pkg, name, index, links) = sessions.package(session, &behavior)?;
    let (c, simulated, stops) = {
        let mut guard = state.inner.lock().await;
        let conn = guard.as_mut().ok_or("Not connected to a robot")?;
        if conn.starting {
            return Err("The behavior is still starting.".into());
        }
        conn.starting = true;
        (conn.client.clone(), conn.simulated, conn.stops)
    };
    let result = play(&app, &state, &c, pkg, &name, (index, links), simulated, stops).await;
    if let Some(conn) = state.inner.lock().await.as_mut().filter(|conn| Arc::ptr_eq(&conn.client, &c)) {
        conn.starting = false;
    }
    result
}

async fn play(app: &AppHandle, state: &RobotState, c: &Arc<Client>, pkg: Vec<u8>, name: &str, graph: Graph, simulated: bool, stops: u64) -> Result<String, String> {
    let name = name.to_string();
    if c.call("ALBehaviorManager", "isBehaviorRunning", vec![name.as_str().into()]).await?.as_bool() == Some(true) {
        c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await?;
    }
    // shortcut: no md5 (NAOqi skips the check when it is empty); the TCP transfer is checked already.
    let file = Value::Object(choren_qi::object::file(pkg));
    if c.call("PackageManager", "_install", vec![file, "choren".into(), "".into()]).await?.as_bool() != Some(true) {
        return Err("The robot refused the package.".into());
    }
    // NAOqi lists the behavior shortly after the install returns.
    for _ in 0..50 {
        if c.call("ALBehaviorManager", "isBehaviorInstalled", vec![name.as_str().into()]).await?.as_bool() == Some(true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let debugger = Some(debug(app, c, &name, graph, simulated).await?);
    let stopped = |guard: &Option<Connection>| guard.as_ref().is_some_and(|conn| Arc::ptr_eq(&conn.client, c) && conn.stops != stops);
    // Stop pressed during the upload: the robot must not start moving afterwards.
    if stopped(&*state.inner.lock().await) {
        debugger.inspect(|d| d.0.abort());
        return Err(CANCELLED.into());
    }
    let started = c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await;
    let mut guard = state.inner.lock().await;
    if started.is_ok() && stopped(&guard) {
        drop(guard);
        debugger.inspect(|d| d.0.abort());
        c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await?;
        return Err(CANCELLED.into());
    }
    // Record the play on the connection it went to; a reconnect during the upload is another robot.
    match (started, guard.as_mut().filter(|conn| Arc::ptr_eq(&conn.client, c))) {
        (Ok(_), Some(conn)) => {
            conn.playing = Some(name.clone());
            conn.frame_manager = debugger.as_ref().filter(|_| !simulated).map(|d| d.1.clone());
            conn.loaded = debugger.as_ref().map(|d| d.2.clone()).unwrap_or_default();
            if let Some(old) = std::mem::replace(&mut conn.debugger, debugger.map(|d| d.0)) {
                old.abort();
            }
            Ok(name)
        }
        (started, _) => {
            if let Some((task, ..)) = debugger {
                task.abort();
            }
            started.and(Err("The robot connection changed during Play.".into()))
        }
    }
}

const CANCELLED: &str = "Stopped before the behavior started.";

/// A behavior's boxes by debugger path (editor ids) and its links by output key (`output_links`).
type Graph = (std::collections::HashMap<String, Vec<i64>>, std::collections::HashMap<String, Vec<(String, String)>>);

/// What a run loaded on a real robot, by `diagram_key`: diagrams NAOqi reported (`#onLoad`), and
/// boxes the editor loaded alone because their diagram was not.
#[derive(Default)]
struct Run {
    diagrams: std::collections::HashSet<String>,
    boxes: std::collections::HashSet<String>,
}

type Loaded = Arc<std::sync::Mutex<Run>>;

/// Calls `onInput_<input>__` of the box at `uri` like Choregraphe. NAOqi creates every box at Play
/// but loads and connects a box only with its diagram, so a box whose diagram the run never entered
/// gets its `onLoad` once first (`ALFrameManager.getBox(uri).__onLoad__()`).
async fn stimulate(c: &Client, loaded: &Loaded, frame_manager: &str, uri: &str, input: &str, value: Value) -> Result<Value, String> {
    let target = format!("{frame_manager}{uri}");
    let unloaded = {
        let mut run = loaded.lock().unwrap();
        !run.diagrams.contains(&choren_naoqi::runtime::diagram_key(parent(uri))) && run.boxes.insert(choren_naoqi::runtime::diagram_key(uri))
    };
    if unloaded {
        let Value::Object(b) = c.call("ALFrameManager", "getBox", vec![target.as_str().into()]).await? else { return Err(format!("The robot has no box {uri}")) };
        b.remote().ok_or("The robot returned no box object.")?.call("__onLoad__", vec![]).await?;
    }
    c.call("ALFrameManager", "callBoxInput", vec![target.into(), format!("onInput_{}__", choren_naoqi::runtime::corrected(input)).into(), value]).await
}

/// The diagram path of a box path.
fn parent(uri: &str) -> &str {
    uri.rsplit_once('/').map_or("", |p| p.0)
}

/// The behavior debugger on a robot: preload, `_getBehaviorFrameManagerId`, then the debugger
/// object's `signal(key, value)`; keys are `<id>:<box path>[#<port>]` (gaps doc §6). On a real robot
/// it also relays the outputs of boxes the editor loaded alone along their links, since NAOqi did
/// not connect them; aborting the task (Stop, another Play) ends the relays.
async fn debug(app: &AppHandle, c: &Arc<Client>, name: &str, (index, links): Graph, frames_only: bool) -> Result<(tauri::async_runtime::JoinHandle<()>, String, Loaded), String> {
    c.call("ALBehaviorManager", "preloadBehavior", vec![name.into()]).await?;
    let id = c.call("ALBehaviorManager", "_getBehaviorFrameManagerId", vec![name.into()]).await?;
    let Value::Object(o) = c.call("ALFrameManager", "getBehaviorDebuggerFor", vec![id.clone()]).await? else { return Err("The robot has no behavior debugger.".into()) };
    let mut events = o.remote().ok_or("The robot has no behavior debugger.")?.subscribe("signal").await?;
    let prefix = id.as_str().unwrap_or("").trim_end_matches('/').to_string();
    let (app, name, frame_manager, loaded) = (app.clone(), name.to_string(), prefix.clone(), Loaded::default());
    let (seen, c) = (loaded.clone(), c.clone());
    let task = tauri::async_runtime::spawn(async move {
        let _keep = o;
        let mut relays = tokio::task::JoinSet::new();
        while let Some(v) = events.recv().await {
            let f = v.items();
            let Some(key) = f.first().and_then(Value::as_str).and_then(|k| k.strip_prefix(prefix.as_str())) else { continue };
            if let Some(diagram) = choren_naoqi::runtime::loaded_diagram(key) {
                seen.lock().unwrap().diagrams.insert(diagram);
            }
            let (path, port) = choren_naoqi::runtime::split_debugger_key(key);
            let alone = {
                let run = seen.lock().unwrap();
                run.boxes.contains(&choren_naoqi::runtime::diagram_key(path)) && !run.diagrams.contains(&choren_naoqi::runtime::diagram_key(parent(path)))
            };
            if let (true, Some(port), false) = (alone, port, frames_only) {
                for (to, input) in links.get(&format!("{path}#{}", choren_naoqi::runtime::corrected(port))).into_iter().flatten() {
                    let (c, seen, prefix, to, input, value) = (c.clone(), seen.clone(), prefix.clone(), to.clone(), input.clone(), f.get(1).cloned().unwrap_or(Value::Void));
                    relays.spawn(async move { stimulate(&c, &seen, &prefix, &to, &input, value).await });
                }
            }
            while relays.try_join_next().is_some() {}
            let Some(ids) = index.get(path) else { continue };
            if let Some((event, payload)) = debugger_event(&name, ids, port, f.get(1).and_then(Value::as_i64), frames_only) {
                let _ = app.emit(event, payload);
            }
        }
    });
    Ok((task, frame_manager, loaded))
}

// ChorenSim already sends port activity; its debugger supplies only timeline frames.
fn debugger_event(name: &str, ids: &[i64], port: Option<&str>, frame: Option<i64>, frames_only: bool) -> Option<(&'static str, Json)> {
    match port {
        Some(_) if frames_only => None,
        Some(port) => Some(("robot://stimulated", json!([name, ids, port]))),
        None => Some(("robot://frame", json!([name, ids, frame]))),
    }
}

#[cfg(test)]
mod wiring_tests {
    use super::*;

    #[test]
    fn running_list_changes_become_started_and_stopped_events() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(running_changes(&names(&["a/b", "c/d"]), &names(&["c/d", "e/f"])), [json!(["e/f", "started"]), json!(["a/b", "stopped"])]);
        assert!(running_changes(&names(&["a/b"]), &names(&["a/b"])).is_empty());
    }

    #[test]
        fn debugger_forwards_frames_on_both_robots_without_duplicate_virtual_glow() {
        for frames_only in [true, false] {
            for frame in [Some(20), Some(-1), None] {
                assert_eq!(debugger_event("demo", &[3, 2], None, frame, frames_only), Some(("robot://frame", json!(["demo", [3, 2], frame]))));
            }
        }
        assert_eq!(debugger_event("demo", &[3], Some("onStart"), None, true), None);
        assert_eq!(debugger_event("demo", &[3], Some("onStart"), None, false), Some(("robot://stimulated", json!(["demo", [3], "onStart"]))));
    }
}

/// Stimulates an input of the playing behavior from the editor: `ChorenSim.activateInput` on
/// Choren's simulator, and on a real robot what Choregraphe sends,
/// `ALFrameManager.callBoxInput("<id>:<box path>", "onInput_<input>__", value)`
/// (`AR::Constants::k_legacyPathSeparator`). NAOqi generates that `onInput_<input>__(self, p)`
/// wrapper, which glows the input and calls the script's `onInput_<input>` with or without the value.
/// A box whose diagram the run never entered is loaded alone first and its outputs relayed (`debug`),
/// as on Choren's simulator.
#[tauri::command]
pub async fn robot_activate_input(state: State<'_, RobotState>, sessions: State<'_, Sessions>, session: u32, behavior: String, path: Vec<[i64; 3]>, input: i64, value: Json) -> Result<Json, String> {
    let (c, name, frame_manager, simulated, loaded) = {
        let guard = state.inner.lock().await;
        let conn = guard.as_ref().ok_or("Not connected to a robot")?;
        (conn.client.clone(), conn.playing.clone().ok_or("Play the behavior first.")?, conn.frame_manager.clone(), conn.simulated, conn.loaded.clone())
    };
    let v = if simulated {
        c.call("ChorenSim", "activateInput", vec![name.as_str().into(), json_to_value(&json!(path)), Value::Int(input), json_to_value(&value)]).await?
    } else {
        let (uri, port) = sessions.box_port(session, &behavior, &path, Some(input))?;
        let frame_manager = frame_manager.ok_or("The robot's behavior debugger is not available.")?;
        stimulate(&c, &loaded, &frame_manager, &uri, &port, json_to_value(&value)).await?
    };
    Ok(value_to_json(&v))
}

/// "Auto-update parameters on robot": `ALFrameManager.callBoxInput("<id>:<box path>",
/// "setParameter", name, value)` for each parameter, as Choregraphe sends it (virtual and real).
#[tauri::command]
pub async fn robot_set_parameters(state: State<'_, RobotState>, sessions: State<'_, Sessions>, session: u32, behavior: String, path: Vec<[i64; 3]>, values: Vec<(String, Json)>) -> Result<(), String> {
    let (c, name) = {
        let guard = state.inner.lock().await;
        let conn = guard.as_ref().ok_or("Not connected to a robot")?;
        (conn.client.clone(), conn.playing.clone().ok_or("Play the behavior first.")?)
    };
    let (uri, _) = sessions.box_port(session, &behavior, &path, None)?;
    let id = c.call("ALBehaviorManager", "_getBehaviorFrameManagerId", vec![name.as_str().into()]).await?;
    let target = format!("{}{uri}", id.as_str().unwrap_or("").trim_end_matches('/'));
    for (param, value) in values {
        c.call("ALFrameManager", "callBoxInput", vec![target.as_str().into(), "setParameter".into(), param.into(), json_to_value(&value)]).await?;
    }
    Ok(())
}

/// Stops the behavior Play started (every behavior when none is known).
#[tauri::command]
pub async fn robot_stop(state: State<'_, RobotState>) -> Result<(), String> {
    let playing = state.inner.lock().await.as_mut().and_then(|c| {
        c.stops += 1;
        c.debugger.take().inspect(|d| d.abort());
        c.playing.take()
    });
    let c = client(&state).await?;
    match playing {
        Some(name) => c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.map(|_| ()),
        None => c.call("ALBehaviorManager", "stopAllBehaviors", vec![]).await.map(|_| ()),
    }
}

#[tauri::command]
pub async fn robot_status(state: State<'_, RobotState>) -> Result<Option<Json>, String> {
    Ok(state.inner.lock().await.as_ref().map(|c| json!({"url": c.url, "model": c.model, "virtual": c.virtual_robot.is_some()})))
}

/// Service metadata comes from the connected robot, including third-party modules.
#[tauri::command]
pub async fn robot_services(state: State<'_, RobotState>, service: Option<String>) -> Result<Json, String> {
    let c = client(&state).await?;
    if let Some(name) = service {
        let (_, meta) = c.service(&name).await?;
        Ok(json!({"description": meta.description, "methods": meta.methods.values().filter(|m| m.uid > 8).map(|m| json!({"id": m.uid, "name": m.name, "parameters": m.params, "returns": m.ret})).collect::<Vec<_>>(), "signals": meta.signals.values().map(|s| json!({"name": s.name, "signature": s.sig})).collect::<Vec<_>>()}))
    } else { Ok(json!(c.services().await?)) }
}

/// Camera bytes must bypass the generic service JSON conversion, which treats raw buffers as text.
#[tauri::command]
pub async fn robot_camera_frame(state: State<'_, RobotState>, handle: String) -> Result<Json, String> {
    let c = client(&state).await?;
    let image = c.call("ALVideoDevice", "getImageRemote", vec![handle.as_str().into()]).await?;
    let _ = c.call("ALVideoDevice", "releaseImage", vec![handle.as_str().into()]).await;
    camera_image(&image)
}

fn camera_image(image: &choren_qi::value::Value) -> Result<Json, String> {
    let fields = image.items();
    let width = fields.first().and_then(|v| v.as_i64()).ok_or("Camera returned no image")?;
    let height = fields.get(1).and_then(|v| v.as_i64()).ok_or("Missing camera height")?;
    if width <= 0 || height <= 0 || width > 4096 || height > 4096 { return Err("Invalid camera dimensions".into()); }
    let bytes = match fields.get(6).map(|v| v.plain()) {
        Some(choren_qi::value::Value::Raw(bytes)) => bytes,
        _ => return Err("Camera did not return a binary image buffer".into()),
    };
    if bytes.len() != (width * height * 3) as usize { return Err("Camera did not return RGB pixels".into()); }
    Ok(json!({"width": width, "height": height, "bytes": bytes}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use choren_qi::value::Value as V;
    #[test]
    fn camera_frames_preserve_raw_rgb_and_reject_invalid_buffers() {
        let image = |w, bytes| V::List(vec![V::Int(w), V::Int(1), V::Int(3), V::Int(11), V::Int(0), V::Int(0), V::Dynamic(Box::new(bytes))]);
        assert_eq!(camera_image(&image(1, V::Raw(vec![0, 255, 128]))).unwrap()["bytes"], json!([0,255,128]));
        assert!(camera_image(&image(0, V::Raw(vec![]))).is_err());
        assert!(camera_image(&image(4097, V::Raw(vec![]))).is_err());
        assert!(camera_image(&image(1, V::Raw(vec![0,255]))).is_err());
        assert!(camera_image(&image(1, V::Str("abc".into()))).is_err());
    }
}
