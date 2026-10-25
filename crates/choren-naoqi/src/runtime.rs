//! Behavior runtime: what ALFrameManager and ALBehaviorManager do for a behavior on the robot.
//!
//! Flow semantics run here, in Rust: loading a diagram loads its boxes and fires the owner's
//! inner onLoad; onStart of a diagram box loads its diagram and follows the rail links; an
//! onStopped output unloads the box's diagram before propagating; onStop unloads. Script boxes
//! run in a Python 3 host process (python/choren_host) that speaks JSON lines on stdin/stdout.

use crate::resources::{Mode, Owner, Want, DEMAND_GRACE};
use crate::sim::{Curve, Key, Segment};
use crate::Robotd;
use choren_core::project::Project;
use choren_core::xml::{Doc, NodeId};
use choren_core::{rules, view};
use choren_qi::{ServiceBuilder, Value};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch, Notify};

#[derive(Default)]
pub struct Runtime {
    d: Mutex<Weak<Robotd>>,
    /// Installed packages: uuid → their `.pml`, or the `manifest.xml` of an extracted `.pkg`.
    packages: Mutex<BTreeMap<String, PathBuf>>,
    /// Where this robot extracts `.pkg`s (removed with the runtime).
    pkg_dir: Mutex<Option<PathBuf>>,
    running: Mutex<BTreeMap<String, Arc<Instance>>>,
    /// Legacy `ALMemory.subscribeToEvent(event, module, method)` registrations.
    modules: Mutex<Vec<(String, String, String)>>,
    pub python: Mutex<PythonConfig>,
    pub resources: crate::resources::Resources,
}

#[derive(Clone, Debug)]
pub struct PythonConfig {
    pub exe: String,
    /// Folder that contains the `choren_host` package.
    pub dir: PathBuf,
}

impl Default for PythonConfig {
    fn default() -> Self {
        PythonConfig { exe: "python3".into(), dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python") }
    }
}

enum Ev {
    Input(NodeId, i64, Value, oneshot::Sender<Result<(), String>>),
    Memory(String, Value),
    Output(NodeId, String, Value),
    /// Ticks of a timeline's play generation (an older generation's ticks are ignored).
    TimelineFrame(NodeId, i64, u64),
    TimelineDone(NodeId, u64),
    /// `callBoxInput(…, "setParameter", …)`: a parameter value; the box is not loaded for it.
    Param(NodeId, i64, Value, oneshot::Sender<Result<(), String>>),
    /// A script's `getTimeline()`/`getParentTimeline()` call on timeline box `NodeId`.
    Timeline(NodeId, Control, oneshot::Sender<Value>),
    Unload(NodeId),
    /// The box's resources were granted: deliver the input that asked for them.
    Granted(NodeId, i64, Value),
    ResourceFailed(NodeId),
    /// Another box demanded a resource this "Stop on demand" box holds.
    StopBox(NodeId),
    Stop,
}

pub struct Instance {
    pub name: String,
    dir: PathBuf,
    doc: Arc<Doc>,
    tx: mpsc::UnboundedSender<Ev>,
    host: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    stopped: AtomicBool,
    finished: Notify,
    tasks: Mutex<HashMap<u64, Task>>,
    next_task: AtomicU64,
    writer: tokio::sync::Mutex<()>,
}

/// A posted call (`post`/`pCall`). `done` turns true when it returns and closes when it is aborted,
/// so one thread can `wait` while another `stop`s it.
struct Task {
    abort: tokio::task::AbortHandle,
    done: watch::Receiver<bool>,
}

/// Timeline operations scripts reach through the ALBehavior timeline object.
#[derive(Debug)]
enum Control {
    Play,
    Pause,
    Stop,
    GoTo(Json),
    SetFps(f64),
    Size,
    Fps,
}

/// Per-instance execution state, owned by the event loop.
#[derive(Default)]
struct Exec {
    loaded_boxes: HashSet<NodeId>,
    loaded_diagrams: HashSet<NodeId>,
    /// ALMemory key → (box, input id) for nature-4 inputs of loaded boxes.
    memory_inputs: Vec<(String, NodeId, i64)>,
    /// Parameter values set through links (box, name) → value.
    overrides: HashMap<(NodeId, String), Value>,
    timelines: HashMap<NodeId, tokio::task::JoinHandle<()>>,
    /// Simulator task playing each timeline box's motion, cancelled when the timeline stops.
    motions: HashMap<NodeId, u32>,
    /// Active keyframe diagram per (timeline box, layer index).
    keyframes: HashMap<(NodeId, usize), NodeId>,
    /// Last frame each timeline played, and frame rates set by scripts (`setFPS`).
    frames: HashMap<NodeId, i64>,
    fps: HashMap<NodeId, f64>,
    /// Play generation per timeline, bumped whenever its tick stops.
    generations: HashMap<NodeId, u64>,
    /// Dialog boxes' topics, and the ALMemory keys `<topic><output>` that fire their outputs.
    dialogs: HashMap<NodeId, String>,
    memory_outputs: Vec<(String, NodeId, i64)>,
}

pub fn value_to_json(v: &Value) -> Json {
    match v.plain() {
        Value::Void => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::UInt(u) => json!(u),
        Value::Float(f) => json!(*f as f64),
        Value::Double(f) => json!(f),
        Value::Str(s) => json!(s),
        Value::Raw(b) => json!(String::from_utf8_lossy(b)),
        Value::List(l) | Value::Tuple(l) => Json::Array(l.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            if m.iter().all(|(k, _)| k.as_str().is_some()) {
                Json::Object(m.iter().map(|(k, v)| (k.as_str().unwrap().to_string(), value_to_json(v))).collect())
            } else {
                Json::Array(m.iter().map(|(k, v)| json!([value_to_json(k), value_to_json(v)])).collect())
            }
        }
        Value::Dynamic(_) | Value::Object(..) => Json::Null,
    }
}

pub fn json_to_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Void,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) if n.is_f64() => Value::Float(n.as_f64().unwrap() as f32),
        Json::Number(n) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::UInt(n.as_u64().unwrap_or(0))),
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(a) => Value::List(a.iter().map(json_to_value).collect()),
        Json::Object(o) => Value::Map(o.iter().map(|(k, v)| (Value::Str(k.clone()), Value::Dynamic(Box::new(json_to_value(v))))).collect()),
    }
}

// --- model helpers over the lossless XAR tree ---

fn int(d: &Doc, n: NodeId, k: &str) -> i64 {
    d.attr_num(n, k, -1.0) as i64
}

fn inner_diagrams(d: &Doc, b: NodeId) -> Vec<NodeId> {
    let Some(t) = d.kid(b, "Timeline") else { return vec![] };
    if d.attr_raw(t, "enable") == Some("1") {
        return vec![];
    }
    d.kids(t, "BehaviorLayer").filter_map(|l| d.kid(l, "BehaviorKeyframe")).filter_map(|k| d.kid(k, "Diagram")).collect()
}

fn is_timeline(d: &Doc, b: NodeId) -> bool {
    d.kid(b, "Timeline").is_some_and(|t| d.attr_raw(t, "enable") == Some("1"))
}

fn script(d: &Doc, b: NodeId) -> String {
    let v = view::box_view(d, b);
    v.script
}

fn has_script(d: &Doc, b: NodeId) -> bool {
    d.kid(b, "script").and_then(|s| d.kid(s, "content")).is_some_and(|c| d.text(c).contains("class MyClass"))
}

fn port(d: &Doc, b: NodeId, id: i64) -> Option<NodeId> {
    rules::ports(d, b).find(|&p| int(d, p, "id") == id)
}

fn port_named(d: &Doc, b: NodeId, tag: &str, name: &str) -> Option<NodeId> {
    d.kids(b, tag).find(|&p| d.attr(p, "name").as_deref() == Some(name))
}

/// Box ids from the root's diagram down to `b` (the editor's address of a box).
fn id_path(d: &Doc, mut b: NodeId) -> Vec<i64> {
    let mut path = vec![];
    while let Some(diagram) = d.nodes[b].parent.filter(|&p| d.is_el(p, "Diagram")) {
        path.push(int(d, b, "id"));
        match rules::diagram_owner(d, diagram) {
            Some(o) => b = o,
            None => break,
        }
    }
    path.reverse();
    path
}

/// The timeline box whose behavior keyframe holds box `b`.
fn parent_timeline(d: &Doc, b: NodeId) -> Option<NodeId> {
    let diagram = d.nodes[b].parent.filter(|&p| d.is_el(p, "Diagram"))?;
    d.nodes[diagram].parent.filter(|&k| d.is_el(k, "BehaviorKeyframe"))?;
    rules::diagram_owner(d, diagram)
}

/// A box's path in NAOqi's behavior debugger: `/` for the root, then `<name>_<id>` per box, with `<layer>/<keyframe>`
/// after an enabled timeline box whose keyframe holds the next one, e.g.
/// `/Motion_3/my layer/key frame/Tick_1`. Debugger keys are this path, then `#<port>` for a port.
pub fn box_uri(d: &Doc, b: NodeId) -> String {
    let mut parts = vec![];
    let mut cur = b;
    while let Some(diagram) = d.nodes[cur].parent.filter(|&p| d.is_el(p, "Diagram")) {
        parts.push(format!("{}_{}", escaped(&d.attr_or(cur, "name")), int(d, cur, "id")));
        if let Some((layer, kf)) = timeline_keyframe(d, diagram) {
            parts.push(format!("{}/{}", escaped(&d.attr_or(layer, "name")), escaped(&d.attr_or(kf, "name"))));
        }
        match rules::diagram_owner(d, diagram) {
            Some(owner) => cur = owner,
            None => break,
        }
    }
    parts.reverse();
    format!("/{}", parts.join("/"))
}

/// The layer and keyframe holding `diagram` in an enabled timeline.
fn timeline_keyframe(d: &Doc, diagram: NodeId) -> Option<(NodeId, NodeId)> {
    let kf = d.nodes[diagram].parent.filter(|&k| d.is_el(k, "BehaviorKeyframe"))?;
    let layer = d.nodes[kf].parent?;
    d.nodes[layer].parent.filter(|&t| d.attr_raw(t, "enable") == Some("1")).map(|_| (layer, kf))
}

/// A name in a debugger path: `#`, `/` and `\` get a backslash.
fn escaped(name: &str) -> String {
    name.chars().flat_map(|c| if matches!(c, '#' | '/' | '\\') { vec!['\\', c] } else { vec![c] }).collect()
}

/// A name made safe for a debugger key: letters, digits and `_` only, without `__`.
pub fn corrected(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_') {
        if !(c == '_' && out.ends_with('_')) {
            out.push(c);
        }
    }
    out
}

/// Every box's debugger path → its editor address (box ids from the root's diagram down), to
/// place a real robot's debugger events in the diagram.
pub fn debugger_index(d: &Doc) -> HashMap<String, Vec<i64>> {
    (0..d.nodes.len()).filter(|&n| d.is_el(n, "Box")).map(|b| (box_uri(d, b), id_path(d, b))).collect()
}

/// Box-to-box links by the debugger key of their output (`<box path>#<corrected output>`): the
/// target box path and its corrected input, for relaying a box NAOqi did not connect.
pub fn output_links(d: &Doc) -> HashMap<String, Vec<(String, String)>> {
    let mut links: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for dg in (0..d.nodes.len()).filter(|&n| d.is_el(n, "Diagram")) {
        let port = |owner: i64, tag: &str, id: i64| {
            let b = d.kids(dg, "Box").find(|&b| int(d, b, "id") == owner)?;
            let p = d.kids(b, tag).find(|&p| int(d, p, "id") == id)?;
            Some((box_uri(d, b), corrected(&d.attr_or(p, "name"))))
        };
        for l in d.kids(dg, "Link") {
            let (Some((from, output)), Some(to)) = (port(int(d, l, "outputowner"), "Output", int(d, l, "indexofoutput")), port(int(d, l, "inputowner"), "Input", int(d, l, "indexofinput"))) else { continue };
            links.entry(format!("{from}#{output}")).or_default().push(to);
        }
    }
    links
}

/// The box an editor address names: `[layer index, keyframe index, box id]` per level.
pub fn box_at(d: &Doc, path: &[[i64; 3]]) -> Result<NodeId, String> {
    let mut b = view::root_box(d);
    for &[layer, keyframe, box_id] in path {
        let layer = usize::try_from(layer).map_err(|_| "Invalid layer index")?;
        let keyframe = usize::try_from(keyframe).map_err(|_| "Invalid keyframe index")?;
        let diagram = d.kid(b, "Timeline")
            .and_then(|t| d.kids(t, "BehaviorLayer").nth(layer))
            .and_then(|l| d.kids(l, "BehaviorKeyframe").nth(keyframe))
            .and_then(|k| d.kid(k, "Diagram"))
            .ok_or("The input's diagram no longer exists in the running behavior.")?;
        b = d.kids(diagram, "Box").find(|&b| int(d, b, "id") == box_id)
            .ok_or("The input's box no longer exists in the running behavior.")?;
    }
    Ok(b)
}

/// Splits a debugger key (after the behavior id) into the box path and the port, at the first
/// `#` that `escaped` did not protect.
pub fn split_debugger_key(key: &str) -> (&str, Option<&str>) {
    let mut escape = false;
    for (i, c) in key.char_indices() {
        match c {
            '\\' if !escape => escape = true,
            '#' if !escape => return (&key[..i], Some(&key[i + 1..])),
            _ => escape = false,
        }
    }
    (key, None)
}

/// The diagram a debugger key reports loaded, as [`diagram_key`] spells it: `/#onLoad` (root),
/// `/A_1#onLoad`, or a timeline keyframe's `/A_1#layer/keyframe#onLoad`.
pub fn loaded_diagram(key: &str) -> Option<String> {
    let (path, port) = split_debugger_key(key);
    let port = port?;
    let keyframe = if port == "onLoad" { "" } else { port.strip_suffix("#onLoad")? };
    Some(diagram_key(&format!("{path}/{keyframe}")))
}

/// A diagram path (`/A_1/my layer/key frame`) with corrected names, as keyframe load keys give them.
pub fn diagram_key(path: &str) -> String {
    path.split('/').map(corrected).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/")
}

/// NAOqi's `_getBehaviorFrameManagerId`: the behavior's root URI (`<behavior id>:/`).
pub fn frame_manager_id(behavior: &str) -> String {
    format!("_Behavior__{}:/", behavior.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>())
}

/// The box's declared resources and how long it waits for them (`None`: a Wait box waits forever).
/// The boxes enclosing `b`, innermost first.
fn ancestors(d: &Doc, mut b: NodeId) -> Vec<NodeId> {
    let mut out = vec![];
    while let Some(o) = d.nodes[b].parent.filter(|&p| d.is_el(p, "Diagram")).and_then(|diagram| rules::diagram_owner(d, diagram)) {
        out.push(o);
        b = o;
    }
    out
}

fn box_wants(robot: &choren_robot::Robot, d: &Doc, b: NodeId) -> (Vec<Want>, Option<std::time::Duration>) {
    let mut limit = Some(0.0f64);
    let wants = d
        .kids(b, "Resource")
        .map(|r| {
            let (name, mode) = (d.attr_or(r, "name"), Mode::parse(&d.attr_or(r, "type")));
            limit = if mode == Mode::Wait { None } else { limit.map(|l| l.max(d.attr_num(r, "timeout", 0.0))) };
            Want { leaves: robot.resource_leaves(&name), name, mode }
        })
        .collect();
    (wants, limit.map(std::time::Duration::from_secs_f64))
}

/// Waits until every wanted resource is granted to `owner`, asking conflicting holders to stop
/// or release according to their mode (see `resources`). False on timeout.
async fn acquire(d: &Arc<Robotd>, owner: &Owner, ancestors: &[NodeId], wants: &[Want], limit: Option<std::time::Duration>) -> bool {
    let res = &d.runtime.resources;
    let mut deadline = limit.map(|l| tokio::time::Instant::now() + l);
    let mut asked = HashSet::new();
    loop {
        let mut rx = res.watch();
        let conflicts = match res.try_acquire(owner, ancestors, wants) {
            Ok(()) => return true,
            Err(c) => c,
        };
        for c in conflicts {
            if !matches!(c.mode, Mode::StopOnDemand | Mode::CallbackOnDemand) || !asked.insert(c.owner.clone()) {
                continue;
            }
            deadline = deadline.map(|dl| dl.max(tokio::time::Instant::now() + DEMAND_GRACE));
            let holder = d.runtime.running.lock().unwrap().get(&c.owner.behavior).cloned();
            match holder {
                Some(i) if c.mode == Mode::StopOnDemand => {
                    let _ = i.tx.send(Ev::StopBox(c.owner.node));
                }
                Some(i) => {
                    tokio::spawn(async move { i.send(json!({"op": "resource", "box": c.owner.node, "resource": c.resource})).await });
                }
                None => res.release(&c.owner), // left over from a behavior that ended
            }
        }
        let changed = rx.changed();
        let woke = match deadline {
            Some(dl) => tokio::time::timeout_at(dl, changed).await.map(|r| r.is_ok()).unwrap_or(false),
            None => changed.await.is_ok(),
        };
        if !woke {
            return false;
        }
    }
}

fn param_value(d: &Doc, ex: &Exec, b: NodeId, p: NodeId) -> Value {
    let name = d.attr_or(p, "name");
    if let Some(v) = ex.overrides.get(&(b, name.clone())) {
        return v.clone();
    }
    // inherits_from_parent: an ancestor's parameter of the same name wins.
    if d.attr_raw(p, "inherits_from_parent") == Some("1") {
        let mut cur = b;
        while let Some(owner) = d.nodes[cur].parent.filter(|&x| d.is_el(x, "Diagram")).and_then(|dg| rules::diagram_owner(d, dg)) {
            if let Some(ap) = port_named(d, owner, "Parameter", &name) {
                return param_value(d, ex, owner, ap);
            }
            cur = owner;
        }
    }
    let raw = d.attr_or(p, "value");
    match d.attr_raw(p, "content_type") {
        Some("0") => Value::Bool(raw == "1" || raw.eq_ignore_ascii_case("true")),
        Some("1") => Value::Int(raw.trim().parse().unwrap_or(0)),
        Some("2") => Value::Float(raw.trim().parse().unwrap_or(0.0)),
        _ => Value::Str(raw),
    }
}

/// The frame after which a timeline stops: `end_frame`, or when it is absent or -1 the last key of the unmuted curves. `size` and behavior
/// keyframes do not count, so a timeline with behavior layers only has an end below 1.
pub fn timeline_end(d: &Doc, t: NodeId) -> i64 {
    match d.attr_num(t, "end_frame", -1.0) as i64 {
        -1 => d.kids(t, "ActuatorList")
            .flat_map(|l| d.kids(l, "ActuatorCurve").collect::<Vec<_>>())
            .filter(|&c| d.attr_raw(c, "mute") != Some("1"))
            .flat_map(|c| d.kids(c, "Key").map(|k| d.attr_num(k, "frame", 0.0) as i64).collect::<Vec<_>>())
            .max()
            .unwrap_or(0),
        end => end,
    }
}

/// A timeline curve's keys in seconds from `start` and robot units (`scale`), with each
/// segment's interpolation decided exactly as the editor draws it (`curvePath` in
/// app/src/timeline/Curves.tsx): a Constant right tangent holds, a Linear tangent on either side
/// makes the segment straight, anything else is Bezier.
pub fn curve_keys(d: &Doc, curve: NodeId, start: f64, fps: f64, scale: f64) -> Vec<Key> {
    let tangent = |k: NodeId, side: &str| d.kids(k, "Tangent").find(|&tg| d.attr_raw(tg, "side") == Some(side));
    let kind = |k: Option<NodeId>, side: &str| k.and_then(|k| tangent(k, side)).and_then(|t| d.attr(t, "interpType").or_else(|| d.attr(t, "editor:interpType"))).unwrap_or_default();
    let nodes: Vec<NodeId> = d.kids(curve, "Key").collect();
    nodes
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            let handle = |side| tangent(k, side).map(|tg| (d.attr_num(tg, "abscissaParam", 0.0) / fps, d.attr_num(tg, "ordinateParam", 0.0) * scale));
            let (out, next_in) = (kind(Some(k), "right"), kind(nodes.get(i + 1).copied(), "left"));
            let after = match (out.as_str(), next_in.as_str()) {
                ("constant" | "piecewise constant", _) => Segment::Constant,
                ("linear", _) | (_, "linear") => Segment::Linear,
                _ => Segment::Bezier,
            };
            Key { left: handle("left"), right: handle("right"), after, ..Key::new((d.attr_num(k, "frame", 0.0) - start) / fps, d.attr_num(k, "value", 0.0) * scale) }
        })
        .collect()
}

impl Instance {
    fn doc(&self) -> &Doc {
        &self.doc
    }

    async fn send(&self, msg: Json) {
        let _guard = self.writer.lock().await;
        let mut line = msg.to_string();
        line.push('\n');
        let stdin = self.host.lock().unwrap().take();
        if let Some(mut s) = stdin {
            let ok = s.write_all(line.as_bytes()).await.is_ok();
            if ok {
                *self.host.lock().unwrap() = Some(s);
            }
        }
    }

    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            let _ = self.tx.send(Ev::Stop);
        }
    }
}

struct Ctx {
    d: Arc<Robotd>,
    inst: Arc<Instance>,
    ex: Exec,
}

impl Ctx {
    fn doc(&self) -> &Doc {
        self.inst.doc()
    }

    fn emit_stimulated(&self, b: NodeId, port: &str, v: &Value) {
        let path = Value::List(id_path(self.doc(), b).into_iter().map(|i| Value::Int(i)).collect());
        self.d.node.emit("ChorenSim", "stimulated", vec![self.inst.name.as_str().into(), path, port.into()]);
        // NAOqi reports script boxes' outputs but not their inputs (Python calls them directly).
        let d = self.doc();
        if !(has_script(d, b) && port_named(d, b, "Input", port).is_some()) {
            self.d.debug(&self.inst.name, &format!("{}#{}", box_uri(d, b), corrected(port)), v);
        }
    }

    fn owner(&self, b: NodeId) -> Owner {
        Owner { behavior: self.inst.name.clone(), node: b }
    }

    fn release(&self, b: NodeId) {
        self.d.runtime.resources.release(&self.owner(b));
    }

    /// Stop on demand: the box stops as if its onStop fired, and gives its resources back.
    async fn stop_box(&mut self, b: NodeId) {
        let d = self.inst.doc.clone();
        match d.kids(b, "Input").find(|&p| d.attr_raw(p, "nature") == Some("3")) {
            Some(p) => self.stimulate(b, int(&d, p, "id"), Value::Void).await,
            None => self.stop_timeline(b).await,
        }
        self.release(b);
    }

    /// The box could not get its resources: onResourceError runs, then the box reports onStopped.
    async fn resource_failed(&mut self, b: NodeId) {
        let d = self.inst.doc.clone();
        self.d.log("warning", "behavior.box", &format!("{}: resources not available", d.attr_or(b, "name")));
        if has_script(&d, b) {
            // The script box runs onResourceError, then reports its onStopped outputs itself.
            self.inst.send(json!({"op": "resourceError", "box": b})).await;
            return;
        }
        for p in d.kids(b, "Output").filter(|&p| d.attr_raw(p, "nature") == Some("1")).collect::<Vec<_>>() {
            self.output(b, int(&d, p, "id"), Value::Void).await;
        }
    }

    async fn load_box(&mut self, b: NodeId) {
        if !self.ex.loaded_boxes.insert(b) {
            return;
        }
        let d = self.inst.doc.clone();
        for p in d.kids(b, "Input") {
            if d.attr_raw(p, "nature") == Some("4") {
                if let Some(key) = d.attr(p, "stm_value_name").filter(|k| !k.is_empty()) {
                    self.ex.memory_inputs.push((key, b, int(&d, p, "id")));
                }
            }
        }
        if let Some(file) = d.kid(b, "dialogFile").map(|f| d.text(f)).filter(|f| !f.trim().is_empty()) {
            // the `.dlg` is `<behavior folder>/<dialogFile>`
            let ports = rules::ports(&d, b).filter(|&p| !d.is_el(p, "Parameter")).map(|p| d.attr_or(p, "name")).collect();
            match self.d.dialog.load_box(&self.inst.dir.join(file.trim()), ports) {
                Ok(topic) => {
                    for p in d.kids(b, "Output") {
                        self.ex.memory_outputs.push((format!("{topic}{}", d.attr_or(p, "name")), b, int(&d, p, "id")));
                    }
                    self.ex.dialogs.insert(b, topic);
                }
                Err(e) => self.d.log("error", "behavior.box", &format!("{}: {e}", d.attr_or(b, "name"))),
            }
        }
        if has_script(&d, b) {
            let params: serde_json::Map<String, Json> = d.kids(b, "Parameter").map(|p| (d.attr_or(p, "name"), value_to_json(&param_value(&d, &self.ex, b, p)))).collect();
            let ports = |tag: &str| d.kids(b, tag).map(|p| json!({"name": d.attr_or(p, "name"), "nature": int(&d, p, "nature"), "bang": d.attr_raw(p, "type") == Some("1")})).collect::<Vec<_>>();
            let path = id_path(&d, b).iter().map(|i| i.to_string()).collect::<Vec<_>>().join("/");
            self.inst
                .send(json!({
                    "op": "load", "box": b, "name": d.attr_or(b, "name"),
                    "uri": format!("{}:/{}", self.inst.name, path),
                    "script": script(&d, b), "inputs": ports("Input"), "outputs": ports("Output"),
                    "params": params, "path": self.inst.dir.to_string_lossy(),
                    "timeline": is_timeline(&d, b),
                    "parentTimeline": parent_timeline(&d, b).is_some(),
                }))
                .await;
        }
    }

    async fn unload_box(&mut self, b: NodeId) {
        let d = self.inst.doc.clone();
        self.stop_timeline(b).await;
        self.release(b);
        if self.ex.loaded_boxes.remove(&b) {
            self.ex.memory_inputs.retain(|m| m.1 != b);
            self.ex.memory_outputs.retain(|m| m.1 != b);
            if let Some(topic) = self.ex.dialogs.remove(&b) {
                self.d.dialog.deactivate(&topic);
                let _ = self.d.dialog.unload(&topic);
            }
            if has_script(&d, b) {
                self.inst.send(json!({"op": "unload", "box": b})).await;
            }
        }
    }

    async fn load_diagram(&mut self, dg: NodeId) {
        if !self.ex.loaded_diagrams.insert(dg) {
            return;
        }
        let d = self.inst.doc.clone();
        for c in d.kids(dg, "Box").collect::<Vec<_>>() {
            self.load_box(c).await;
        }
        // The owner's inner onLoad input fires once its diagram is loaded.
        if let Some(owner) = rules::diagram_owner(&d, dg) {
            // A box whose timeline is off reports frame 1 when its diagram starts (observed).
            if timeline_keyframe(&d, dg).is_none() && d.kid(owner, "Timeline").is_some() {
                self.d.debug(&self.inst.name, &box_uri(&d, owner), &Value::Int(1));
            }
            let rail = match timeline_keyframe(&d, dg) {
                Some((layer, kf)) => format!("{}/{}#onLoad", corrected(&d.attr_or(layer, "name")), corrected(&d.attr_or(kf, "name"))),
                None => "onLoad".into(),
            };
            self.d.debug(&self.inst.name, &format!("{}#{rail}", box_uri(&d, owner)), &Value::Void);
            for p in d.kids(owner, "Input").filter(|&p| d.attr_raw(p, "nature") == Some("0")).collect::<Vec<_>>() {
                self.follow_rail(dg, int(&d, p, "id"), Value::Void).await;
            }
        }
    }

    async fn unload_diagram(&mut self, dg: NodeId) {
        // Editor activation can load a descendant without entering its enclosing diagram.
        self.ex.loaded_diagrams.remove(&dg);
        let d = self.inst.doc.clone();
        for c in d.kids(dg, "Box").collect::<Vec<_>>() {
            Box::pin(self.unload_box(c)).await;
        }
    }

    /// Delivers a value from the owner's rail port `port` to everything linked to it inside `dg`.
    async fn follow_rail(&mut self, dg: NodeId, port: i64, v: Value) {
        let d = self.inst.doc.clone();
        let targets: Vec<(i64, i64)> = d.kids(dg, "Link").filter(|&l| int(&d, l, "outputowner") == 0 && int(&d, l, "indexofoutput") == port).map(|l| (int(&d, l, "inputowner"), int(&d, l, "indexofinput"))).collect();
        for (owner, id) in targets {
            Box::pin(self.deliver(dg, owner, id, v.clone())).await;
        }
    }

    async fn deliver(&mut self, dg: NodeId, owner: i64, id: i64, v: Value) {
        let d = self.inst.doc.clone();
        if owner == 0 {
            if let Some(o) = rules::diagram_owner(&d, dg) {
                Box::pin(self.output(o, id, v)).await;
            }
        } else if let Some(b) = d.kids(dg, "Box").find(|&b| int(&d, b, "id") == owner) {
            Box::pin(self.stimulate(b, id, v)).await;
        }
    }

    async fn stimulate(&mut self, b: NodeId, id: i64, v: Value) {
        if self.inst.stopped.load(Ordering::SeqCst) {
            return;
        }
        let d = self.inst.doc.clone();
        let Some(p) = port(&d, b, id) else { return };
        let name = d.attr_or(p, "name");
        self.emit_stimulated(b, &name, &v);
        if d.is_el(p, "Parameter") {
            self.ex.overrides.insert((b, name.clone()), v.clone());
            if has_script(&d, b) {
                self.inst.send(json!({"op": "param", "box": b, "name": name, "value": value_to_json(&v)})).await;
            }
            return;
        }
        let nature = d.attr_raw(p, "nature").unwrap_or("1").to_string();
        if nature == "2" {
            // onStart acquires the box's resources first, off the event loop so holders can stop.
            let (wants, limit) = box_wants(&self.d.sim.robot, &d, b);
            let owner = self.owner(b);
            if !wants.is_empty() && !self.d.runtime.resources.holds_all(&owner, &wants) {
                let (rd, inst, up) = (self.d.clone(), self.inst.clone(), ancestors(&d, b));
                tokio::spawn(async move {
                    let ok = acquire(&rd, &owner, &up, &wants, limit).await;
                    let sent = inst.tx.send(if ok { Ev::Granted(b, id, v) } else { Ev::ResourceFailed(b) });
                    if ok && (sent.is_err() || inst.stopped.load(Ordering::SeqCst)) {
                        rd.runtime.resources.release(&owner);
                    }
                });
                return;
            }
        }
        if has_script(&d, b) {
            self.load_box(b).await;
            let has_value = d.attr_raw(p, "type") != Some("1");
            self.inst.send(json!({"op": "input", "box": b, "port": name, "value": value_to_json(&v), "hasValue": has_value})).await;
        }
        // A Dialog box: onStart activates its topic,
        // onStop deactivates it and the box reports onStopped.
        if let Some(topic) = self.ex.dialogs.get(&b).cloned() {
            match nature.as_str() {
                "2" => {
                    let _ = self.d.dialog.activate(&topic);
                }
                "3" => {
                    self.d.dialog.deactivate(&topic);
                    if let Some(p) = port_named(&d, b, "Output", "onStopped") {
                        self.output(b, int(&d, p, "id"), Value::Void).await;
                    }
                }
                _ => {}
            }
            return;
        }
        if nature == "3" {
            // onStop: the box and everything inside it stop.
            self.stop_timeline(b).await;
            return;
        }
        if is_timeline(&d, b) {
            if nature == "2" {
                self.start_timeline(b).await;
            }
            return;
        }
        for dg in inner_diagrams(&d, b) {
            self.load_diagram(dg).await;
            self.follow_rail(dg, id, v.clone()).await;
        }
    }

    async fn output(&mut self, b: NodeId, id: i64, v: Value) {
        let d = self.inst.doc.clone();
        let Some(p) = port(&d, b, id) else { return };
        let name = d.attr_or(p, "name");
        self.emit_stimulated(b, &name, &v);
        if d.attr_raw(p, "nature") == Some("1") {
            // onStopped: the box's own diagram is unloaded before the signal leaves it.
            self.stop_timeline(b).await;
            self.release(b);
            if let Some(topic) = self.ex.dialogs.get(&b) {
                self.d.dialog.deactivate(topic);
            }
        }
        let Some(dg) = d.nodes[b].parent.filter(|&x| d.is_el(x, "Diagram")) else {
            if d.attr_raw(p, "nature") == Some("1") {
                self.inst.stop(); // the root box finished
            }
            return;
        };
        // A keyframe diagram inside a timeline box routes rail outputs to the timeline box.
        let bid = int(&d, b, "id");
        let targets: Vec<(i64, i64)> = d.kids(dg, "Link").filter(|&l| int(&d, l, "outputowner") == bid && int(&d, l, "indexofoutput") == id).map(|l| (int(&d, l, "inputowner"), int(&d, l, "indexofinput"))).collect();
        for (owner, port) in targets {
            self.deliver(dg, owner, port, v.clone()).await;
        }
    }

    async fn start_timeline(&mut self, b: NodeId) {
        self.stop_timeline(b).await;
        let d = self.inst.doc.clone();
        let start = d.attr_num(d.kid(b, "Timeline").unwrap(), "start_frame", 1.0).max(1.0) as i64;
        self.play_timeline(b, start);
    }

    /// Plays timeline `b` from frame `from`: its motion from the current pose, and a frame tick
    /// every 1/fps on the motion clock.
    fn play_timeline(&mut self, b: NodeId, from: i64) {
        self.halt_timeline(b);
        let d = self.inst.doc.clone();
        let t = d.kid(b, "Timeline").unwrap();
        let fps = self.ex.fps.get(&b).copied().unwrap_or_else(|| d.attr_num(t, "fps", 25.0)).max(1.0);
        let start = from;
        let sim = self.d.sim.clone();
        // Motion: one trajectory task for every curve, starting from the current pose.
        let current = sim.angles();
        let mut curves = vec![];
        let end = timeline_end(&d, t);
        for list in d.kids(t, "ActuatorList") {
            for c in d.kids(list, "ActuatorCurve") {
                if d.attr_raw(c, "mute") == Some("1") {
                    continue;
                }
                let Some(j) = sim.robot.joint_index(&d.attr_or(c, "actuator")) else { continue };
                // Joints are stored in degrees, hands as an opening ratio.
                let scale = if sim.robot.joints[j].axis == [0.0; 3] { 1.0 } else { std::f64::consts::PI / 180.0 };
                // shortcut: after a goTo, keys before the new frame are dropped and the joint moves
                // from its current pose to the next key (as on the robot).
                let mut keys: Vec<Key> = curve_keys(&d, c, start as f64, fps, scale).into_iter().filter(|k| k.t >= 0.0).collect();
                if keys.first().is_some_and(|k| k.t > 0.0) {
                    keys.insert(0, Key::new(0.0, current[j]));
                }
                if !keys.is_empty() {
                    curves.push((j, Curve { keys }));
                }
            }
        }
        let mut clock = sim.motion_clock();
        let started = *clock.borrow_and_update();
        if !curves.is_empty() {
            let (id, _) = sim.start_task(curves, None);
            self.ex.motions.insert(b, id);
        }
        let tx = self.inst.tx.clone();
        let generation = self.ex.generations.get(&b).copied().unwrap_or(0);
        let handle = tokio::spawn(async move {
            let mut frame = start;
            loop {
                let due = started + (frame - start) as f64 / fps;
                while *clock.borrow_and_update() + 1e-9 < due {
                    if clock.changed().await.is_err() {
                        return;
                    }
                }
                // the tick after the end frame stops the timeline; an
                // end below 1 (behavior layers only) runs until a diagram stops it.
                if end >= 1 && frame > end {
                    let _ = tx.send(Ev::TimelineDone(b, generation));
                    return;
                }
                if tx.send(Ev::TimelineFrame(b, frame, generation)).is_err() {
                    return;
                }
                frame += 1;
            }
        });
        self.ex.timelines.insert(b, handle);
    }

    /// Stops the frame tick and motion; keyframe diagrams stay loaded (pause).
    fn halt_timeline(&mut self, b: NodeId) {
        *self.ex.generations.entry(b).or_default() += 1;
        if let Some(id) = self.ex.motions.remove(&b) {
            self.d.sim.kill_task(id);
        }
        if let Some(h) = self.ex.timelines.remove(&b) {
            h.abort();
        }
    }

    /// A timeline box's play/pause/stop/goTo/setFPS/getSize/getFPS: play resumes from the current frame, stop rewinds to the start frame and
    /// unloads the keyframes without signalling outputs, goTo(0) means frame 1 and keeps playing
    /// or paused, a name goes to that behavior keyframe, getSize is end − start.
    async fn control_timeline(&mut self, b: NodeId, c: Control) -> Value {
        let d = self.inst.doc.clone();
        let Some(t) = d.kid(b, "Timeline") else { return Value::Void };
        let start = d.attr_num(t, "start_frame", 1.0).max(1.0) as i64;
        let current = self.ex.frames.get(&b).copied().unwrap_or(start);
        let playing = self.ex.timelines.contains_key(&b);
        let loaded = self.ex.loaded_boxes.contains(&b) && is_timeline(&d, b);
        match c {
            Control::Size => return Value::Int((timeline_end(&d, t) - start).max(0)),
            Control::Fps => return Value::Int(self.ex.fps.get(&b).copied().unwrap_or_else(|| d.attr_num(t, "fps", 25.0)) as i64),
            _ if !loaded => {}
            Control::Play if !playing => self.play_timeline(b, current),
            Control::Play => {}
            Control::Pause => self.halt_timeline(b),
            Control::Stop => {
                self.stop_timeline(b).await;
                self.ex.frames.insert(b, start);
            }
            Control::GoTo(to) => {
                let frame = match to {
                    Json::String(name) => d.kids(t, "BehaviorLayer").flat_map(|l| d.kids(l, "BehaviorKeyframe")).find(|&k| d.attr_raw(k, "name") == Some(name.as_str())).map(|k| d.attr_num(k, "index", 1.0) as i64),
                    v => v.as_f64().map(|f| f as i64),
                };
                let Some(frame) = frame.map(|f| if f == 0 { 1 } else { f }) else { return Value::Void };
                if playing {
                    self.play_timeline(b, frame);
                } else {
                    self.timeline_frame(b, frame).await;
                }
            }
            Control::SetFps(fps) if fps >= 1.0 => {
                self.ex.fps.insert(b, fps);
                if playing {
                    self.play_timeline(b, current);
                }
            }
            Control::SetFps(_) => {}
        }
        Value::Void
    }

    async fn stop_timeline(&mut self, b: NodeId) {
        self.halt_timeline(b);
        // NAOqi's debugger shows a stopped box's frame as -1 (observed, gaps doc §6).
        if self.inst.doc.kid(b, "Timeline").is_some() {
            self.d.debug(&self.inst.name, &box_uri(&self.inst.doc, b), &Value::Int(-1));
        }
        self.ex.keyframes.retain(|(tb, _), _| *tb != b);
        // Include manually activated boxes in inactive flow diagrams and timeline keyframes.
        let d = self.inst.doc.clone();
        let diagrams: Vec<NodeId> = d.kid(b, "Timeline").into_iter()
            .flat_map(|t| d.kids(t, "BehaviorLayer"))
            .flat_map(|l| d.kids(l, "BehaviorKeyframe"))
            .filter_map(|k| d.kid(k, "Diagram")).collect();
        for dg in diagrams {
            self.unload_diagram(dg).await;
        }
    }

    /// Loads the keyframe diagram each behavior layer is in at `frame`; the previous one unloads.
    async fn timeline_frame(&mut self, b: NodeId, frame: i64) {
        let d = self.inst.doc.clone();
        let Some(t) = d.kid(b, "Timeline") else { return };
        for (li, l) in d.kids(t, "BehaviorLayer").enumerate() {
            if d.attr_raw(l, "mute") == Some("1") {
                continue;
            }
            let current = d.kids(l, "BehaviorKeyframe").filter(|&k| d.attr_num(k, "index", 1.0) as i64 <= frame).last().and_then(|k| d.kid(k, "Diagram"));
            let prev = self.ex.keyframes.get(&(b, li)).copied();
            if current == prev {
                continue;
            }
            if let Some(p) = prev {
                self.unload_diagram(p).await;
                self.ex.keyframes.remove(&(b, li));
            }
            if let Some(dg) = current {
                self.ex.keyframes.insert((b, li), dg);
                // Loading fires the keyframe diagram's onLoad rail only; NAOqi sends no onStart
                // there (as on NAOqi 2.8).
                self.load_diagram(dg).await;
            }
        }
        self.ex.frames.insert(b, frame);
        self.d.set_data(&format!("ALFrameManager/{}/frame", self.inst.name), Value::Int(frame));
        self.d.debug(&self.inst.name, &box_uri(&d, b), &Value::Int(frame));
    }

    async fn timeline_done(&mut self, b: NodeId) {
        let d = self.inst.doc.clone();
        // NAOqi publishes the frame after the end before it stops the timeline.
        if let Some(t) = d.kid(b, "Timeline") {
            self.d.debug(&self.inst.name, &box_uri(&d, b), &Value::Int(timeline_end(&d, t) + 1));
            // Reaching the end stops the timeline, and stop rewinds to the start frame.
            self.ex.frames.insert(b, d.attr_num(t, "start_frame", 1.0).max(1.0) as i64);
        }
        let stopped: Vec<i64> = d.kids(b, "Output").filter(|&p| d.attr_raw(p, "nature") == Some("1")).map(|p| int(&d, p, "id")).collect();
        self.stop_timeline(b).await;
        for id in stopped {
            self.output(b, id, Value::Void).await;
        }
    }
}

impl Runtime {
    fn robotd(&self) -> Option<Arc<Robotd>> {
        self.d.lock().unwrap().upgrade()
    }

    pub fn subscribe_module(&self, event: &str, module: &str, method: &str) -> Result<Value, String> {
        let mut m = self.modules.lock().unwrap();
        m.retain(|(e, n, _)| !(e == event && n == module));
        m.push((event.into(), module.into(), method.into()));
        Ok(Value::Void)
    }

    pub fn unsubscribe_module(&self, event: &str, module: &str) -> Result<Value, String> {
        self.modules.lock().unwrap().retain(|(e, n, _)| !(e == event && n == module));
        Ok(Value::Void)
    }

    /// Routes an ALMemory change to event inputs, Python subscribers and legacy module callbacks.
    pub fn memory_changed(&self, key: &str, v: &Value) {
        let callbacks: Vec<(String, String)> = self.modules.lock().unwrap().iter().filter(|(e, _, _)| e == key).map(|(_, m, f)| (m.clone(), f.clone())).collect();
        for inst in self.running.lock().unwrap().values() {
            let _ = inst.tx.send(Ev::Memory(key.into(), v.clone()));
            let inst = inst.clone();
            let (key, v, callbacks) = (key.to_string(), value_to_json(v), callbacks.clone());
            tokio::spawn(async move {
                inst.send(json!({"op": "mem", "key": key, "value": v})).await;
                for (module, method) in callbacks {
                    inst.send(json!({"op": "module", "module": module, "method": method, "args": [key, v, ""]})).await;
                }
            });
        }
    }

    pub fn install(&self, path: &str) -> Result<String, String> {
        let p = Path::new(path);
        if p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pkg")) {
            return self.install_pkg(&std::fs::read(p).map_err(|e| format!("Cannot read {path}: {e}"))?);
        }
        let pml = if p.is_dir() {
            std::fs::read_dir(p).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|f| f.extension().is_some_and(|x| x == "pml")).ok_or("No .pml project in that folder")?
        } else {
            p.to_path_buf()
        };
        let project = Project::open(&pml)?;
        let uuid = project.metadata.get("UUID").cloned().unwrap_or_else(|| project.name.to_lowercase());
        self.packages.lock().unwrap().insert(uuid.clone(), pml);
        Ok(uuid)
    }

    /// Installs an application package (`.pkg`) as NAOqi does: behaviors named `<uuid>/<folder>`,
    /// a new version replacing the old one.
    pub fn install_pkg(&self, bytes: &[u8]) -> Result<String, String> {
        let files = choren_core::pkg::unzip(bytes)?;
        let (_, manifest) = files.iter().find(|f| f.0 == "manifest.xml").ok_or("The package has no manifest.xml")?;
        if manifest.len() as u64 > MAX_XML {
            return Err("manifest.xml is too large".into());
        }
        let uuid = {
            let m = Doc::parse(std::str::from_utf8(manifest).map_err(|_| "manifest.xml is not UTF-8")?)?;
            m.attr_or(m.root().ok_or("Empty manifest.xml")?, "uuid")
        };
        // One name, as NAOqi keeps each package in a folder of that name. Dot names stay allowed:
        // Play installs `.lastUploadedChoregrapheBehavior` (libpackagesynchronizer).
        if matches!(uuid.as_str(), "" | "." | "..") || uuid.contains(['/', '\\', ':', '\0']) {
            return Err(format!("Invalid package uuid {uuid:?}"));
        }
        let root = self.pkg_root()?;
        // Each install unpacks into its own numbered folder, so no two uuids or versions ever share
        // one (whatever the file system folds) and the installed version stays until this one
        // proves sound. `create_dir`: once the robot removed its root, installs fail.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = root.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
        let unpacked = std::fs::create_dir(&dir).map_err(|e| format!("Cannot install {uuid}: {e}")).and_then(|_| {
            files.iter().try_for_each(|(name, data)| {
                let f = dir.join(name);
                std::fs::create_dir_all(f.parent().unwrap()).and_then(|_| std::fs::write(&f, data)).map_err(|e| format!("Cannot install {name}: {e}"))
            })
        });
        drop(files);
        let manifest = dir.join("manifest.xml");
        let checked = unpacked.and_then(|_| package_folders(&manifest)?.iter().try_for_each(|f| package_behavior(&manifest, f).map(drop)));
        if let Err(e) = checked {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        let old = self.packages.lock().unwrap().insert(uuid.clone(), manifest);
        // Only an unpacked version is removed, never a project folder `install` registered.
        // shortcut: a reader that looked the old version up just before this fails to read it, as
        // during a reinstall on NAOqi; keep old folders until the robot goes away if that matters.
        if let Some(old) = old.as_deref().and_then(Path::parent).filter(|p| p.parent() == Some(root.as_path())) {
            let _ = std::fs::remove_dir_all(old);
        }
        Ok(uuid)
    }

    /// This robot's folder for unpacked packages, created exclusively (owner-only on Unix) so no
    /// other user can plant files in it.
    fn pkg_root(&self) -> Result<PathBuf, String> {
        let mut slot = self.pkg_dir.lock().unwrap();
        if let Some(root) = slot.as_ref() {
            return Ok(root.clone());
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        for _ in 0..1000 {
            let dir = std::env::temp_dir().join(format!("choren-robot-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            match builder.create(&dir) {
                Ok(()) => return Ok(slot.insert(dir).clone()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("Cannot create a folder for packages: {e}")),
            }
        }
        Err("Cannot create a folder for packages".into())
    }

    /// Removes the unpacked packages (the robot is going away). The folder stays assigned, so an
    /// install still running fails instead of creating a new one, and `Drop` removes what it left.
    pub fn remove_packages(&self) {
        if let Some(dir) = self.pkg_dir.lock().unwrap().as_ref() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn resolve(&self, name: &str) -> Result<(PathBuf, Doc, PathBuf), String> {
        // NAOqi's message for any name that is not an installed behavior.
        let missing = || format!("behavior {name} is not installed");
        let (pkg, folder) = name.split_once('/').ok_or_else(missing)?;
        let pml = self.packages.lock().unwrap().get(pkg).cloned().ok_or_else(missing)?;
        if !package_folders(&pml)?.iter().any(|f| f == folder) {
            return Err(missing());
        }
        let doc = package_behavior(&pml, folder)?;
        let dir = pml.parent().unwrap().join(folder);
        Ok((pml, doc, dir))
    }

    pub fn installed(&self) -> Vec<String> {
        let packages = self.packages.lock().unwrap().clone();
        packages
            .iter()
            .flat_map(|(uuid, pml)| package_folders(pml).map(|f| f.into_iter().map(|folder| format!("{uuid}/{folder}")).collect::<Vec<_>>()).unwrap_or_default())
            .collect()
    }

    pub fn running(&self) -> Vec<String> {
        self.running.lock().unwrap().keys().cloned().collect()
    }

    /// Editor stimulation uses stable diagram addresses, not mutable XML arena indices.
    pub async fn activate_input(&self, name: &str, path: &[[i64; 3]], input: i64, value: Value) -> Result<(), String> {
        let inst = self.running.lock().unwrap().get(name).cloned().ok_or("The behavior is not running.")?;
        if inst.stopped.load(Ordering::SeqCst) {
            return Err("The behavior is not running.".into());
        }
        let d = &inst.doc;
        let b = box_at(d, path)?;
        let p = d.kids(b, "Input").find(|&p| int(d, p, "id") == input).ok_or("Input not found")?;
        let valid = match d.attr_raw(p, "type") {
            Some("0") => true,
            Some("1") => matches!(value.plain(), Value::Void),
            Some("2") => matches!(value.plain(), Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Double(_)) && value.as_f64().is_some_and(f64::is_finite),
            Some("3") => value.as_str().is_some(),
            _ => false,
        };
        if !valid { return Err("Value does not match the input type.".into()); }
        let (tx, rx) = oneshot::channel();
        inst.tx.send(Ev::Input(b, input, value, tx)).map_err(|_| "The behavior is not running.")?;
        rx.await.map_err(|_| "The behavior stopped before the input could be activated.".to_string())?
    }

    /// `ALFrameManager.callBoxInput("<id>:<box path>", method, args)`: what Choregraphe sends a
    /// robot to stimulate a box (`onInput_<input>__`, NAOqi's generated wrapper, value) or update a
    /// parameter (`setParameter`, name, value). The script's own `onInput_<input>` works too.
    pub async fn call_box_input(&self, uri: &str, method: &str, args: &[Value]) -> Result<(), String> {
        let missing = || format!("Cannot find box {uri}");
        let inst = self.running.lock().unwrap().values().find(|i| uri.starts_with(frame_manager_id(&i.name).trim_end_matches('/'))).cloned().ok_or_else(missing)?;
        let path = &uri[frame_manager_id(&inst.name).trim_end_matches('/').len()..];
        let d = &inst.doc;
        let b = (0..d.nodes.len()).find(|&b| d.is_el(b, "Box") && box_uri(d, b) == path).ok_or_else(missing)?;
        let (p, value) = match (method, args) {
            ("setParameter", [name, value]) => (d.kids(b, "Parameter").find(|&p| Some(d.attr_or(p, "name").as_str()) == name.as_str()).ok_or_else(|| format!("Parameter {name:?} does not exist."))?, value),
            (_, [value]) => (d.kids(b, "Input").find(|&p| format!("onInput_{}", corrected(&d.attr_or(p, "name"))) == method.strip_suffix("__").unwrap_or(method)).ok_or_else(|| format!("Can't find method: {method}"))?, value),
            _ => return Err(format!("Can't find method: {method}")),
        };
        let (tx, rx) = oneshot::channel();
        let ev = if method == "setParameter" { Ev::Param(b, int(d, p, "id"), value.clone(), tx) } else { Ev::Input(b, int(d, p, "id"), value.clone(), tx) };
        inst.tx.send(ev).map_err(|_| "The behavior is not running.")?;
        rx.await.map_err(|_| "The behavior stopped before the input could be activated.".to_string())?
    }

    pub fn stop(&self, name: &str) {
        if let Some(i) = self.running.lock().unwrap().get(name) {
            i.stop();
        }
    }

    pub fn stop_all(&self) {
        for i in self.running.lock().unwrap().values() {
            i.stop();
        }
    }

    /// Starts a behavior and returns once it is running; the returned instance notifies on finish.
    pub async fn start(&self, name: &str) -> Result<Arc<Instance>, String> {
        let d = self.robotd().ok_or("robot is shutting down")?;
        if self.running.lock().unwrap().contains_key(name) {
            return Err(format!("Behavior {name} is already running"));
        }
        let (_, doc, dir) = self.resolve(name)?;
        let root = view::root_box(&doc);
        let start = doc.kids(root, "Input").find(|&p| doc.attr_raw(p, "nature") == Some("2") && doc.attr_raw(p, "type") == Some("1"));
        let Some(start) = start else {
            return Err("Flow diagram cannot be played : box does not have an onStart input expecting a Bang type.".into());
        };
        let start_id = int(&doc, start, "id");
        let cfg = self.python.lock().unwrap().clone();
        // -B: no __pycache__ writes, which would break the signature of a packaged app.
        let mut cmd = Command::new(&cfg.exe);
        cmd.args(["-B", "-E", "-s", "-u", "-X", "utf8", "-m", "choren_host"])
            .current_dir(&cfg.dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // CREATE_NO_WINDOW: python.exe is a console program, so the GUI app would open a console for it.
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| format!("Cannot start the Python box host ({}): {e}", cfg.exe))?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let inst = Arc::new(Instance {
            name: name.into(),
            dir,
            doc: Arc::new(doc),
            tx,
            host: Mutex::new(child.stdin.take()),
            child: Mutex::new(None),
            stopped: AtomicBool::new(false),
            finished: Notify::new(),
            tasks: Default::default(),
            next_task: AtomicU64::new(1),
            writer: Default::default(),
        });
        // Host → runtime messages.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        *inst.child.lock().unwrap() = Some(child);
        {
            let (inst, d) = (inst.clone(), d.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(msg) = serde_json::from_str::<Json>(&line) else {
                        d.log("info", "behavior.box", &line);
                        continue;
                    };
                    host_message(&d, &inst, msg).await;
                }
            });
        }
        {
            let d = d.clone();
            let name = name.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    d.log("info", &name, &line);
                }
            });
        }
        self.running.lock().unwrap().insert(name.into(), inst.clone());
        d.node.emit("ChorenSim", "behavior", vec![name.into(), "started".into()]);
        d.log("info", "ALBehaviorManager", &format!("Behavior {name} started"));
        let (d2, i2) = (d.clone(), inst.clone());
        tokio::spawn(async move {
            let mut cx = Ctx { d: d2.clone(), inst: i2.clone(), ex: Exec::default() };
            cx.stimulate(root, start_id, Value::Void).await;
            while let Some(ev) = rx.recv().await {
                match ev {
                    Ev::Input(b, id, v, reply) => {
                        let result = if cx.inst.stopped.load(Ordering::SeqCst) {
                            Err("The behavior is not running.".into())
                        } else {
                            // A direct editor start need not wait for the enclosing flow to reach this box.
                            cx.load_box(b).await;
                            cx.stimulate(b, id, v).await;
                            Ok(())
                        };
                        let _ = reply.send(result);
                    }
                    Ev::Memory(key, v) => {
                        // ALMemory change: stimulate the event inputs bound to that key.
                        let targets: Vec<(NodeId, i64)> = cx.ex.memory_inputs.iter().filter(|m| m.0 == key).map(|m| (m.1, m.2)).collect();
                        for (b, p) in targets {
                            cx.stimulate(b, p, v.clone()).await;
                        }
                        let outputs: Vec<(NodeId, i64)> = cx.ex.memory_outputs.iter().filter(|m| m.0 == key).map(|m| (m.1, m.2)).collect();
                        for (b, p) in outputs {
                            cx.output(b, p, v.clone()).await;
                        }
                    }
                    Ev::Output(b, name, v) => {
                        // A script still running after its box was unloaded (a keyframe left, a
                        // timeline ended) signals nothing: Choregraphe disables the diagram's links
                        // before unloading its boxes.
                        if !cx.ex.loaded_boxes.contains(&b) {
                            continue;
                        }
                        let d = cx.inst.doc.clone();
                        if let Some(p) = port_named(&d, b, "Output", &name) {
                            cx.output(b, int(&d, p, "id"), v).await;
                        }
                    }
                    Ev::TimelineFrame(b, f, g) if cx.ex.generations.get(&b).copied().unwrap_or(0) == g => cx.timeline_frame(b, f).await,
                    Ev::TimelineDone(b, g) if cx.ex.generations.get(&b).copied().unwrap_or(0) == g => cx.timeline_done(b).await,
                    Ev::TimelineFrame(..) | Ev::TimelineDone(..) => {}
                    Ev::Param(b, id, v, reply) => {
                        // Stored as an override (and sent to a loaded script box); unlike an input,
                        // a parameter update does not load the box.
                        cx.stimulate(b, id, v).await;
                        let _ = reply.send(Ok(()));
                    }
                    Ev::Timeline(b, c, done) => {
                        let _ = done.send(cx.control_timeline(b, c).await);
                    }
                    Ev::Unload(b) => cx.unload_box(b).await,
                    Ev::Granted(b, id, v) => cx.stimulate(b, id, v).await,
                    Ev::ResourceFailed(b) => cx.resource_failed(b).await,
                    Ev::StopBox(b) => cx.stop_box(b).await,
                    Ev::Stop => break,
                }
            }
            // Unload everything (onUnload runs), then end the host.
            let root = view::root_box(cx.doc());
            cx.unload_box(root).await;
            cx.inst.send(json!({"op": "shutdown"})).await;
            let child = cx.inst.child.lock().unwrap().take();
            if let Some(mut c) = child {
                if tokio::time::timeout(std::time::Duration::from_secs(2), c.wait()).await.is_err() {
                    let _ = c.kill().await;
                }
            }
            for (_, t) in cx.inst.tasks.lock().unwrap().drain() {
                t.abort.abort();
            }
            d2.runtime.running.lock().unwrap().remove(&cx.inst.name);
            d2.runtime.resources.release_behavior(&cx.inst.name);
            d2.node.emit("ChorenSim", "behavior", vec![cx.inst.name.as_str().into(), "stopped".into()]);
            d2.log("info", "ALBehaviorManager", &format!("Behavior {} stopped", cx.inst.name));
            cx.inst.finished.notify_waiters();
            cx.inst.stopped.store(true, Ordering::SeqCst);
        });
        Ok(inst)
    }
}

async fn reply(inst: &Instance, id: &Json, r: Result<Value, String>) {
    let msg = match r {
        Ok(v) => json!({"op": "reply", "id": id, "ok": value_to_json(&v)}),
        Err(e) => json!({"op": "reply", "id": id, "error": e}),
    };
    inst.send(msg).await;
}

async fn host_message(d: &Arc<Robotd>, inst: &Arc<Instance>, msg: Json) {
    let op = msg["op"].as_str().unwrap_or("");
    let args = || msg["args"].as_array().map(|a| a.iter().map(json_to_value).collect::<Vec<_>>()).unwrap_or_default();
    match op {
        "call" => {
            let (d, inst, msg, args) = (d.clone(), inst.clone(), msg.clone(), args());
            tokio::spawn(async move {
                let r = d.node.call(msg["service"].as_str().unwrap_or(""), msg["method"].as_str().unwrap_or(""), args).await;
                reply(&inst, &msg["id"], r).await;
            });
        }
        "post" => {
            let id = inst.next_task.fetch_add(1, Ordering::Relaxed);
            let (d2, args) = (d.clone(), args());
            let (service, method) = (msg["service"].as_str().unwrap_or("").to_string(), msg["method"].as_str().unwrap_or("").to_string());
            let (done_tx, done) = watch::channel(false);
            let task = tokio::spawn(async move {
                let _ = d2.node.call(&service, &method, args).await;
                let _ = done_tx.send(true);
            });
            inst.tasks.lock().unwrap().insert(id, Task { abort: task.abort_handle(), done });
            reply(inst, &msg["id"], Ok(Value::Int(id as i64))).await;
        }
        "wait" => {
            let task_id = msg["task"].as_u64().unwrap_or(0);
            let done = inst.tasks.lock().unwrap().get(&task_id).map(|t| t.done.clone());
            let (inst, msg) = (inst.clone(), msg.clone());
            tokio::spawn(async move {
                // A stopped task counts as finished, as it does for NAOqi's wait.
                let r = match done {
                    Some(mut done) => {
                        let finished = async move { let _ = done.wait_for(|&d| d).await; };
                        let limit = msg["timeout"].as_f64().filter(|&t| t > 0.0).map(|ms| std::time::Duration::from_secs_f64(ms / 1000.0));
                        match limit {
                            Some(l) => tokio::time::timeout(l, finished).await.is_ok(),
                            None => {
                                finished.await;
                                true
                            }
                        }
                    }
                    None => true,
                };
                if r {
                    inst.tasks.lock().unwrap().remove(&task_id);
                }
                reply(&inst, &msg["id"], Ok(Value::Bool(r))).await;
            });
        }
        "stopTask" => {
            if let Some(t) = inst.tasks.lock().unwrap().remove(&(msg["task"].as_u64().unwrap_or(0))) {
                t.abort.abort();
            }
        }
        "isRunning" => {
            let running = inst.tasks.lock().unwrap().get(&(msg["task"].as_u64().unwrap_or(0))).is_some_and(|t| !*t.done.borrow() && t.done.has_changed().is_ok());
            reply(inst, &msg["id"], Ok(Value::Bool(running))).await;
        }
        "timeline" => {
            // getTimeline(): the box's own timeline; getParentTimeline(): the timeline whose
            // keyframe holds the box.
            let b = msg["box"].as_u64().unwrap_or(0) as usize;
            let target = if msg["parent"].as_bool() == Some(true) { parent_timeline(&inst.doc, b) } else { Some(b).filter(|&b| inst.doc.kid(b, "Timeline").is_some()) };
            let control = match (msg["action"].as_str().unwrap_or(""), &msg["value"]) {
                ("play", _) => Control::Play,
                ("pause", _) => Control::Pause,
                ("stop", _) => Control::Stop,
                ("goTo", v) => Control::GoTo(v.clone()),
                ("setFPS", v) => Control::SetFps(v.as_f64().unwrap_or(0.0)),
                ("size", _) => Control::Size,
                _ => Control::Fps,
            };
            let (tx, rx) = oneshot::channel();
            let sent = target.is_some_and(|b| inst.tx.send(Ev::Timeline(b, control, tx)).is_ok());
            let (inst, id) = (inst.clone(), msg["id"].clone());
            tokio::spawn(async move {
                let r = if sent { rx.await.map_err(|_| "The behavior stopped.".to_string()) } else { Err("This box has no timeline.".into()) };
                reply(&inst, &id, r).await;
            });
        }
        "out" => {
            let _ = inst.tx.send(Ev::Output(msg["box"].as_u64().unwrap_or(0) as usize, msg["port"].as_str().unwrap_or("").into(), json_to_value(&msg["value"])));
        }
        "exit" => {
            let _ = inst.tx.send(Ev::Unload(msg["box"].as_u64().unwrap_or(0) as usize));
        }
        "log" => d.log(msg["level"].as_str().unwrap_or("info"), msg["category"].as_str().unwrap_or("behavior.box"), msg["message"].as_str().unwrap_or("")),
        "error" => {
            let text = format!("{}: {}", msg["name"].as_str().unwrap_or("box"), msg["message"].as_str().unwrap_or(""));
            d.log("error", "behavior.box", &text);
            d.node.emit("ChorenSim", "behavior", vec![inst.name.as_str().into(), format!("error: {text}").into()]);
        }
        "stopBehavior" => inst.stop(),
        "release" => d.runtime.resources.release(&Owner { behavior: inst.name.clone(), node: msg["box"].as_u64().unwrap_or(0) as usize }),
        "resourceFree" => {
            let names: Vec<String> = msg["resources"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default();
            let leaves = names.iter().flat_map(|n| d.sim.robot.resource_leaves(n)).collect();
            let free = d.runtime.resources.is_free(&leaves, None);
            reply(inst, &msg["id"], Ok(Value::Bool(free))).await;
        }
        "acquire" => {
            let b = msg["box"].as_u64().unwrap_or(0) as usize;
            let (d, inst, id) = (d.clone(), inst.clone(), msg["id"].clone());
            tokio::spawn(async move {
                let (wants, limit, up) = if inst.doc.nodes.get(b).is_some_and(|_| inst.doc.is_el(b, "Box")) {
                    let (w, l) = box_wants(&d.sim.robot, &inst.doc, b);
                    (w, l, ancestors(&inst.doc, b))
                } else {
                    (vec![], None, vec![])
                };
                let ok = acquire(&d, &Owner { behavior: inst.name.clone(), node: b }, &up, &wants, limit).await;
                reply(&inst, &id, Ok(Value::Bool(ok))).await;
            });
        }
        _ => {}
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.remove_packages();
    }
}

/// The behavior folders a package lists (a `.pml` project, or the `manifest.xml` of an unpacked
/// `.pkg`), each once and inside the package.
fn package_folders(path: &Path) -> Result<Vec<String>, String> {
    if path.extension().is_some_and(|x| x == "pml") {
        return Ok(Project::open(path)?.behaviors.into_iter().map(|b| b.source.trim_end_matches("/behavior.xar").to_string()).collect());
    }
    let m = Doc::parse(&read_xml(path, "the package manifest")?)?;
    let Some(contents) = m.root().and_then(|r| m.kid(r, "contents")) else { return Ok(vec![]) };
    let mut folders: Vec<String> = vec![];
    for b in m.kids(contents, "behaviorContent") {
        // `.` is the package folder itself (as in Choregraphe's sample apps).
        let path = m.attr_or(b, "path");
        let folder = if path == "." { path } else { choren_core::project::relative(&path)? };
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    Ok(folders)
}

/// What a `manifest.xml` or `behavior.xar` may weigh before it is parsed (Choregraphe's largest
/// behavior is about 1 MB).
/// shortcut: parsing takes up to ~50× the file size in memory; fine while only local clients (who
/// can run any behavior anyway) reach the virtual robot, bound parse memory if it listens wider.
const MAX_XML: u64 = 16 << 20;

fn read_xml(path: &Path, name: &str) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Cannot read {name}: {e}"))?.len();
    if size > MAX_XML {
        return Err(format!("{name} is too large"));
    }
    std::fs::read_to_string(path).map_err(|e| format!("Cannot read {name}: {e}"))
}

/// One behavior of a package listed by `package_folders`.
fn package_behavior(path: &Path, folder: &str) -> Result<Doc, String> {
    if path.extension().is_some_and(|x| x == "pml") {
        let b = Project::open(path)?.behaviors.into_iter().find(|b| b.source.trim_end_matches("/behavior.xar") == folder);
        return b.map(|b| b.doc).ok_or_else(|| format!("No behavior {folder}"));
    }
    let name = format!("{folder}/behavior.xar");
    let xar = path.parent().ok_or("Invalid package")?.join(folder).join("behavior.xar");
    // The checks `Project::open` makes, so a broken behavior never reaches the runtime.
    choren_core::project::parse_xar(&name, read_xml(&xar, &name)?.as_bytes())
}

/// Copies a `qi::File` the way NAOqi's `FileCopyToLocal` does: `isOpen`, `size`, then `read`
/// chunks, reporting through the file's `operationProgress()` notifier.
async fn pull(file: &choren_qi::object::Remote) -> Result<Vec<u8>, String> {
    if file.call("isOpen", vec![]).await?.as_bool() != Some(true) {
        return Err("Cannot read file".into());
    }
    let size = file.call("size", vec![]).await?.as_i64().unwrap_or(0).max(0) as usize;
    if size > choren_core::pkg::MAX_UNPACKED {
        return Err("Not enough space available".into());
    }
    let progress = match file.call("operationProgress", vec![]).await? {
        Value::Object(o) => o.remote().cloned(),
        _ => None,
    };
    let notify = |method: &'static str, args: Vec<Value>| {
        let p = progress.clone();
        async move {
            if let Some(p) = p {
                let _ = p.call(method, args).await;
            }
        }
    };
    notify("notifyRunning", vec![]).await;
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let chunk = file.call("read", vec![Value::Int(bytes.len() as i64), Value::UInt(1 << 20)]).await?;
        match chunk.plain() {
            Value::Raw(c) if !c.is_empty() && bytes.len() + c.len() <= size => bytes.extend_from_slice(c),
            _ => {
                notify("notifyFailed", vec![]).await;
                return Err("The package file does not match its size".into());
            }
        }
        notify("notifyProgressed", vec![Value::Double(bytes.len() as f64 / size as f64)]).await;
    }
    notify("notifyFinished", vec![]).await;
    Ok(bytes)
}

/// ALBehaviorManager, PackageManager and a minimal ALFrameManager.
pub fn register(d: &Arc<Robotd>) {
    *d.runtime.d.lock().unwrap() = Arc::downgrade(d);
    let rt = || Arc::downgrade(d);
    let get = |w: &Weak<Robotd>| w.upgrade().ok_or_else(|| "robot is shutting down".to_string());
    let node = &d.node;
    node.register(
        ServiceBuilder::new("PackageManager")
            .method("install", "(s)", "b", {
                let w = rt();
                move |a| {
                    let d = get(&w)?;
                    let path = a[0].as_str().ok_or("path expected")?;
                    let uuid = d.runtime.install(path)?;
                    d.log("info", "PackageManager", &format!("Installed {uuid} from {path}"));
                    Ok(Value::Bool(true))
                }
            })
            .method("_install", "(s)", "s", {
                let w = rt();
                move |a| Ok(get(&w)?.runtime.install(a[0].as_str().ok_or("path expected")?)?.into())
            })
            // What Choregraphe's PackageInstaller and `install(o)` send: a qi::File of the .pkg.
            // shortcut: the md5 argument is not checked; Choren's editor sends none.
            .method_async("install", "(o)", "b", install_file(rt()))
            .method_async("_install", "(oss)", "b", install_file(rt()))
            .method("hasPackage", "(s)", "b", {
                let w = rt();
                move |a| Ok(Value::Bool(get(&w)?.runtime.packages.lock().unwrap().contains_key(a[0].as_str().unwrap_or(""))))
            })
            .method("removePkg", "(s)", "v", {
                let w = rt();
                move |a| {
                    get(&w)?.runtime.packages.lock().unwrap().remove(a[0].as_str().unwrap_or(""));
                    Ok(Value::Void)
                }
            }),
    );
    fn install_file(w: Weak<Robotd>) -> impl Fn(Vec<Value>) -> choren_qi::node::BoxFuture<Result<Value, String>> + Send + Sync + 'static {
        move |a: Vec<Value>| {
            let w = w.clone();
            Box::pin(async move {
                let Some(Value::Object(file)) = a.first() else { return Err("file expected".to_string()) };
                let bytes = pull(file.remote().ok_or("file expected")?).await?;
                let d = w.upgrade().ok_or("robot is shutting down")?;
                let uuid = d.runtime.install_pkg(&bytes)?;
                d.log("info", "PackageManager", &format!("Installed {uuid} ({} bytes)", bytes.len()));
                Ok(Value::Bool(true))
            })
        }
    }
    let start = |w: Weak<Robotd>, wait: bool| {
        move |a: Vec<Value>| {
            let w = w.clone();
            async move {
                let d = w.upgrade().ok_or("robot is shutting down")?;
                let name = a[0].as_str().ok_or("behavior name expected")?.to_string();
                let inst = d.runtime.start(&name).await?;
                if wait {
                    let done = inst.finished.notified();
                    if !inst.stopped.load(Ordering::SeqCst) {
                        done.await;
                    }
                }
                Ok(Value::Void)
            }
        }
    };
    node.register(
        ServiceBuilder::new("ALBehaviorManager")
            .method_async("startBehavior", "(s)", "v", start(rt(), false))
            .method_async("runBehavior", "(s)", "v", start(rt(), true))
            .method("stopBehavior", "(s)", "v", {
                let w = rt();
                move |a| {
                    get(&w)?.runtime.stop(a[0].as_str().unwrap_or(""));
                    Ok(Value::Void)
                }
            })
            .method("stopAllBehaviors", "()", "v", {
                let w = rt();
                move |_| {
                    get(&w)?.runtime.stop_all();
                    Ok(Value::Void)
                }
            })
            .method("isBehaviorRunning", "(s)", "b", {
                let w = rt();
                move |a| Ok(Value::Bool(get(&w)?.runtime.running().iter().any(|r| Some(r.as_str()) == a[0].as_str())))
            })
            .method("isBehaviorInstalled", "(s)", "b", {
                let w = rt();
                move |a| Ok(Value::Bool(get(&w)?.runtime.installed().iter().any(|r| Some(r.as_str()) == a[0].as_str())))
            })
            .method("getRunningBehaviors", "()", "[s]", {
                let w = rt();
                move |_| Ok(Value::List(get(&w)?.runtime.running().into_iter().map(Value::Str).collect()))
            })
            .method("getInstalledBehaviors", "()", "[s]", {
                let w = rt();
                move |_| Ok(Value::List(get(&w)?.runtime.installed().into_iter().map(Value::Str).collect()))
            })
            .method("preloadBehavior", "(s)", "b", {
                let w = rt();
                move |a| Ok(Value::Bool(get(&w)?.runtime.resolve(a[0].as_str().unwrap_or("")).is_ok()))
            })
            .method("_getBehaviorFrameManagerId", "(s)", "s", {
                let w = rt();
                move |a| {
                    let name = a[0].as_str().unwrap_or("");
                    get(&w)?.runtime.resolve(name)?;
                    Ok(frame_manager_id(name).into())
                }
            }),
    );
    // NAOqi's behavior debugger: one `signal(key, value)` with
    // `<behavior id>:<box path>[#<port>]` keys, as `Robotd::debug` raises them.
    let call_box = |w: Weak<Robotd>| {
        move |a: Vec<Value>| {
            let w = w.clone();
            async move {
                let (uri, method) = (a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string());
                let args: Vec<Value> = a[2..].iter().map(|v| v.plain().clone()).collect();
                get(&w)?.runtime.call_box_input(&uri, &method, &args).await.map(|_| Value::Void)
            }
        }
    };
    node.register(ServiceBuilder::new("ALFrameManager").method_async("callBoxInput", "(ssm)", "m", call_box(rt())).method_async("callBoxInput", "(ssmm)", "m", call_box(rt())).method("getBehaviorDebuggerFor", "(s)", "o", {
        let w = rt();
        move |a| {
            let d = get(&w)?;
            let o = choren_qi::Object::local(ServiceBuilder::new("BehaviorDebugger").signal("signal", "(sm)"), false);
            if let choren_qi::Object::Local(l) = &o {
                d.debuggers.lock().unwrap().push((a[0].as_str().unwrap_or("").to_string(), Arc::downgrade(l)));
            }
            Ok(Value::Object(o))
        }
    }));
}

#[cfg(test)]
mod tests {
    #[test]
    fn timeline_end_follows_end_frame_then_unmuted_keys() {
        let end = |xml: &str| { let d = choren_core::xml::Doc::parse(xml).unwrap(); super::timeline_end(&d, d.root().unwrap()) };
        assert_eq!(end(r#"<Timeline end_frame="-1" size="300"><BehaviorLayer><BehaviorKeyframe index="90" /></BehaviorLayer></Timeline>"#), 0, "layers only: never ends");
        assert_eq!(end(r#"<Timeline size="300"><ActuatorList><ActuatorCurve mute="0"><Key frame="40" /></ActuatorCurve><ActuatorCurve mute="1"><Key frame="80" /></ActuatorCurve><ActuatorCurve actuator="Unknown"><Key frame="60" /></ActuatorCurve></ActuatorList></Timeline>"#), 60);
        assert_eq!(end(r#"<Timeline end_frame="25"><ActuatorList><ActuatorCurve><Key frame="40" /></ActuatorCurve></ActuatorList></Timeline>"#), 25);
    }

    #[test]
    fn timeline_segments_keep_their_interpolation() {
        let xml = r#"<ActuatorCurve actuator="HeadYaw"><Key frame="1" value="0"><Tangent side="right" interpType="constant" abscissaParam="0" ordinateParam="0" /></Key><Key frame="51" value="60"><Tangent side="left" interpType="linear" abscissaParam="0" ordinateParam="0" /></Key><Key frame="101" value="0" /></ActuatorCurve>"#;
        let d = super::Doc::parse(xml).unwrap();
        let keys = super::curve_keys(&d, d.root().unwrap(), 1.0, 25.0, 1.0);
        let c = super::Curve { keys };
        assert_eq!(c.sample(1.0), 0.0, "Constant 0° → 60° holds 0° halfway");
        assert_eq!(c.keys[1].after, super::Segment::Bezier);
        assert!((c.sample(2.0) - 60.0).abs() < 1e-9);
    }

    /// Same precedence as the editor: Constant from the left key's right tangent, Linear from
    /// either side, otherwise Bezier.
    #[test]
    fn mixed_tangents_follow_the_editor() {
        use super::Segment::*;
        let case = |right: &str, left: &str| {
            let t = |side: &str, kind: &str| if kind.is_empty() { String::new() } else { format!(r#"<Tangent side="{side}" interpType="{kind}" abscissaParam="10" ordinateParam="5" />"#) };
            let xml = format!(r#"<ActuatorCurve><Key frame="1" value="0">{}</Key><Key frame="51" value="30">{}</Key></ActuatorCurve>"#, t("right", right), t("left", left));
            let d = super::Doc::parse(&xml).unwrap();
            let c = super::Curve { keys: super::curve_keys(&d, d.root().unwrap(), 1.0, 25.0, 1.0) };
            (c.keys[0].after, c.sample(1.0))
        };
        assert_eq!(case("bezier", "linear"), (Linear, 15.0), "the editor draws a straight segment");
        assert_eq!(case("linear", "bezier"), (Linear, 15.0));
        assert_eq!(case("constant", "linear"), (Constant, 0.0));
        assert_eq!(case("bezier", "constant").0, Bezier, "only the left key's right tangent makes a segment constant");
        assert_eq!(case("", "").0, Bezier);
    }

    /// Debugger names: path names keep spaces, ports keep [A-Za-z0-9_].
    #[test]
    fn debugger_names_escape() {
        assert_eq!(super::escaped("a/b#c\\d e"), "a\\/b\\#c\\\\d e");
        assert_eq!(super::corrected("my layer__x"), "mylayer_x");
        assert_eq!(super::split_debugger_key("/A\\#1_2#mylayer/kf#onLoad"), ("/A\\#1_2", Some("mylayer/kf#onLoad")));
        assert_eq!(super::split_debugger_key("/Motion_3"), ("/Motion_3", None));
        // Keys observed on NAOqi 2.8 (doc §6) against the diagrams of the boxes they load.
        let loaded = |k| super::loaded_diagram(k);
        assert_eq!(loaded("/#onLoad"), Some(super::diagram_key("")));
        assert_eq!(loaded("/Inner_2#onLoad"), Some(super::diagram_key("/Inner_2")));
        assert_eq!(loaded("/Motion_3#mylayer/keyframe#onLoad"), Some(super::diagram_key("/Motion_3/my layer/key frame")));
        assert_eq!(loaded("/Inner_2#onStart"), None);
        assert_eq!(loaded("/Inner_2"), None);
    }
}
