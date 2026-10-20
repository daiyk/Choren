//! Choren's virtual robot. `start` serves simulated NAOqi services over the libqi protocol, so the
//! editor (and any qi client, such as qicli) talks to it as to a real NAO.

pub mod conformance;
pub mod dialog;
pub mod postures;
pub mod resources;
pub mod runtime;
pub mod services;
pub mod sim;

use choren_qi::{Node, Value};
use choren_robot::Robot;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

pub struct Robotd {
    pub node: Arc<Node>,
    pub sim: Arc<sim::Sim>,
    pub memory: Mutex<HashMap<String, Value>>,
    pub runtime: runtime::Runtime,
    /// The install's full posture graph, when loaded (`load_posture_graph`).
    pub posture_graph: Mutex<Option<postures::Graph>>,
    /// ALMemory event names (`getEventList`): declared, raised, or present at start-up.
    pub events: Mutex<std::collections::BTreeSet<String>>,
    /// ALModule tasks and not-simulated warnings (`conformance`).
    pub modules: conformance::Modules,
    /// `LogManager` listeners handed out to clients (each raises `onLogMessage`).
    pub log_listeners: Mutex<Vec<std::sync::Weak<choren_qi::object::Local>>>,
    /// Behavior debuggers handed out by `ALFrameManager.getBehaviorDebuggerFor`, by frame manager id.
    pub debuggers: Mutex<Vec<(String, std::sync::Weak<choren_qi::object::Local>)>>,
    /// ALDialog's QiChat topics.
    pub dialog: dialog::Dialog,
}

/// ALRobotPosture's step timing: share of a joint's maximum velocity and time rounding (s).
const POSTURE_SPEED_SHARE: f64 = 0.4;
const POSTURE_TIME_STEP: f64 = 0.02;
/// wakeUp and rest speeds (fractions of each joint's maximum velocity) and the wake-up stiffness
/// ramp, as measured on Choregraphe's virtual robot.
const WAKE_UP_SPEED: f64 = 0.15;
const WAKE_UP_STIFFNESS_CYCLES: u32 = 10;
const REST_SPEED: f64 = 0.1;
/// The two sitting-arm variants of the install's posture graph ("Right" and "Left" families).
const SITTING_RIGHT_ARM: i64 = 1007;
const SITTING_LEFT_ARM: i64 = 21007;

pub struct VirtualRobot {
    pub robotd: Arc<Robotd>,
    pub addr: std::net::SocketAddr,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for VirtualRobot {
    fn drop(&mut self) {
        self.robotd.runtime.stop_all();
        // Service handlers hold the Robotd, so the runtime's own Drop may never run.
        self.robotd.runtime.remove_packages();
        self.robotd.node.close();
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Starts a virtual robot of model `robot` listening on `addr` (e.g. `127.0.0.1:0`).
pub async fn start(robot: &str, addr: &str, python: Option<runtime::PythonConfig>) -> Result<VirtualRobot, String> {
    let robot = Robot::load(robot)?;
    let period = robot.control_period_ms;
    let d = Robotd::new(robot);
    // ALMemory starts with NAOqi's declared events (no value until raised) and the default keys.
    for e in &conformance::defaults().events {
        d.declare_event(e)?;
    }
    for (k, v) in &conformance::defaults().memory {
        d.memory.lock().unwrap().insert(k.clone(), runtime::json_to_value(v));
    }
    let weak = Arc::downgrade(&d);
    d.node.set_completer(Arc::new(move |b| conformance::complete(b, &weak)));
    if let Some(p) = python {
        *d.runtime.python.lock().unwrap() = p;
    }
    // RobotConfig is served by ALRobotModel.getConfig: the virtual robot's ALMemory has no
    // RobotConfig keys.
    services::register(&d);
    conformance::register_missing(&d);
    let addr = d.node.listen(addr).await.map_err(|e| format!("Cannot listen on {addr}: {e}"))?;
    let sim = d.sim.clone();
    let tick = tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_millis(period));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            sim.step(period as f64 / 1000.0);
        }
    });
    Ok(VirtualRobot { robotd: d, addr, tasks: vec![tick] })
}

impl Robotd {
    /// A robot that serves nothing and moves only when its simulator is stepped (tests drive it
    /// cycle by cycle); `start` serves one and steps it in real time.
    pub fn new(robot: Robot) -> Arc<Robotd> {
        Arc::new(Robotd { node: Node::new(), sim: Arc::new(sim::Sim::new(robot)), memory: Default::default(), runtime: Default::default(), posture_graph: Default::default(), events: Default::default(), modules: Default::default(), log_listeners: Default::default(), debuggers: Default::default(), dialog: Default::default() })
    }

    /// ALMemory value, with joint and sensor keys computed from the simulator.
    pub fn get_data(&self, key: &str) -> Result<Value, String> {
        if let Some(rest) = key.strip_prefix("Device/SubDeviceList/") {
            let mut parts = rest.split('/');
            if let (Some(joint), Some(kind)) = (parts.next(), parts.next()) {
                if let Some(j) = self.sim.robot.joint_index(joint) {
                    let st = self.sim.state.lock().unwrap();
                    return Ok(Value::Float(match kind {
                        "Stiffness" => st.stiffness[j],
                        "Temperature" => 30.0,
                        "ElectricCurrent" => 0.0,
                        _ => st.angles[j],
                    } as f32));
                }
            }
        }
        self.memory.lock().unwrap().get(key).cloned().ok_or_else(|| format!("Data not found {key}"))
    }

    /// ALMemory.declareEvent: the event exists (value None) until it is raised.
    pub fn declare_event(&self, key: &str) -> Result<Value, String> {
        self.events.lock().unwrap().insert(key.to_string());
        self.memory.lock().unwrap().entry(key.to_string()).or_insert(Value::Void);
        Ok(Value::Void)
    }

    pub fn set_data(&self, key: &str, v: Value) {
        self.memory.lock().unwrap().insert(key.into(), v.clone());
        self.node.emit("ALMemory", "_changed", vec![key.into(), Value::Dynamic(Box::new(v.clone()))]);
        self.runtime.memory_changed(key, &v);
        self.dialog.memory_changed(key);
    }

    /// Raises `key` (a box path, `#<port>` for a port) on the debuggers of `behavior`.
    pub fn debug(&self, behavior: &str, key: &str, v: &Value) {
        let id = runtime::frame_manager_id(behavior);
        let listeners: Vec<_> = {
            let mut l = self.debuggers.lock().unwrap();
            l.retain(|(_, w)| w.strong_count() > 0);
            l.iter().filter(|(i, _)| *i == id).filter_map(|(_, w)| w.upgrade()).collect()
        };
        let key = format!("{}{key}", id.trim_end_matches('/'));
        for l in listeners {
            l.emit("signal", vec![key.as_str().into(), Value::Dynamic(Box::new(v.plain().clone()))]);
        }
    }

    pub fn log(&self, level: &str, category: &str, message: &str) {
        self.node.emit("ChorenSim", "log", vec![level.into(), category.into(), message.into()]);
        let listeners: Vec<_> = {
            let mut l = self.log_listeners.lock().unwrap();
            l.retain(|w| w.strong_count() > 0);
            l.iter().filter_map(|w| w.upgrade()).collect()
        };
        if listeners.is_empty() {
            return;
        }
        // qi::LogMessage; qi::LogLevel numbers fatal 1 … debug 6.
        static ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        let level = ["fatal", "error", "warning", "info", "verbose", "debug"].iter().position(|l| *l == level).map_or(4, |i| i as i64 + 1);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let id = ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let msg = Value::Tuple(vec!["choren-naoqi".into(), Value::Int(level), category.into(), "".into(), message.into(), Value::UInt(id as u64), Value::UInt(now), Value::UInt(now)]);
        for l in listeners {
            l.emit("onLogMessage", vec![msg.clone()]);
        }
    }

    /// The actuators of an ALLeds name, or NAOqi's error for `method`. NAOqi 2.8's wording differs
    /// by method (measured): `fade` and `setIntensity` (which `on`/`off` go through) misplace the
    /// closing quote, and `reset` has no prefix.
    pub fn led_actuators(&self, method: &str, name: &str) -> Result<Vec<String>, String> {
        self.sim.robot.led_actuators(name).ok_or_else(|| {
            let quote = format!("Could not parse '{name} 'as a LED Device, RGB-LED or Group");
            match method {
                "reset" => quote,
                "on" | "off" | "setIntensity" => format!("ALLeds: setIntensity: {quote}"),
                "fade" => format!("ALLeds: fade: {quote}"),
                _ => format!("ALLeds: {method}: Could not parse '{name}' as a LED Device, RGB-LED or Group"),
            }
        })
    }

    /// Sets each actuator of `name` to the matching channel of 0xRRGGBB. Single-colour LEDs (ears,
    /// head) take the blue channel [inf].
    pub fn set_led(&self, method: &str, name: &str, rgb: u32) -> Result<(), String> {
        let actuators = self.led_actuators(method, name)?;
        let mut st = self.sim.state.lock().unwrap();
        for a in actuators {
            let shift = if a.contains("/Red/") { 16 } else if a.contains("/Green/") { 8 } else { 0 };
            st.leds.insert(a, (rgb >> shift & 0xff) as f32 / 255.0);
        }
        Ok(())
    }

    pub fn set_led_intensity(&self, method: &str, name: &str, level: f32) -> Result<(), String> {
        let actuators = self.led_actuators(method, name)?;
        let mut st = self.sim.state.lock().unwrap();
        for a in actuators {
            st.leds.insert(a, level.clamp(0.0, 1.0));
        }
        Ok(())
    }

    /// Mean intensity of one LED (not a group), as `ALLeds.getIntensity` returns it.
    pub fn led_intensity(&self, name: &str) -> Result<f32, String> {
        if self.sim.robot.leds.groups.contains_key(name) {
            return Err("ALLeds: xGetGroupIntensity: function not implemented in this version...".into());
        }
        let actuators = self.led_actuators("getIntensity", name)?;
        let st = self.sim.state.lock().unwrap();
        Ok(actuators.iter().map(|a| st.leds.get(a).copied().unwrap_or(0.0)).sum::<f32>() / actuators.len().max(1) as f32)
    }

    /// LED colours for the editor: every group and LED as 0xRRGGBB, the mean of its actuators per
    /// channel (single-colour actuators count as blue).
    pub fn led_colors(&self) -> BTreeMap<String, u32> {
        let st = self.sim.state.lock().unwrap();
        let leds = &self.sim.robot.leds;
        leds.groups
            .iter()
            .chain(leds.leds.iter())
            .map(|(name, actuators)| {
                let mean = |pick: &dyn Fn(&str) -> bool| {
                    let v: Vec<f32> = actuators.iter().filter(|a| pick(a)).map(|a| st.leds.get(a).copied().unwrap_or(0.0)).collect();
                    if v.is_empty() { 0 } else { (v.iter().sum::<f32>() / v.len() as f32 * 255.0).round() as u32 }
                };
                let blue = |a: &str| a.contains("/Blue/") || !(a.contains("/Red/") || a.contains("/Green/"));
                (name.clone(), mean(&|a| a.contains("/Red/")) << 16 | mean(&|a| a.contains("/Green/")) << 8 | mean(&blue))
            })
            .collect()
    }

    pub fn posture(&self) -> choren_robot::Posture {
        let st = self.sim.state.lock().unwrap();
        self.sim.robot.classify(&st.angles, st.torso_pitch).clone()
    }

    /// Loads the install's posture graph (e.g. `share/naoqi/robot_posture/postures_naoh25.xml`).
    pub fn load_posture_graph(&self, path: &std::path::Path) -> Result<usize, String> {
        let xml = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let g = postures::parse(&xml, self.sim.robot.joints.len())?;
        let n = g.nodes.len();
        *self.posture_graph.lock().unwrap() = Some(g);
        Ok(n)
    }

    /// The moves ALRobotPosture makes to reach `name`, each a list of steps (angles, torso pitch,
    /// speed factor) played as one Bezier: straight to it between standing and crouching postures,
    /// otherwise onto the nearest posture of the graph, then along the graph. The install's graph
    /// when loaded, else the descriptor's named postures.
    fn posture_plan(&self, name: &str) -> Result<Vec<Vec<(Vec<f64>, f64, f64)>>, String> {
        let robot = &self.sim.robot;
        let direct = |a: &str, b: &str| [a, b].iter().all(|f| matches!(*f, "Standing" | "Crouching"));
        if let Some(g) = self.posture_graph.lock().unwrap().as_ref() {
            let hands: Vec<usize> = (0..robot.joints.len()).filter(|&i| robot.joints[i].axis == [0.0; 3]).collect();
            let (angles, pitch) = { let st = self.sim.state.lock().unwrap(); (st.angles.clone(), st.torso_pitch) };
            let step = |n: usize, factor: f64| (g.nodes[n].angles.clone(), g.nodes[n].inertial[1], factor);
            let from = g.nearest(&angles, pitch, &hands);
            let to = g.nodes.iter().position(|n| n.name == name).ok_or_else(|| format!("Unknown posture {name}"))?;
            if direct(&g.nodes[from].family, &g.nodes[to].family) {
                return Ok(vec![vec![step(to, 1.0)]]);
            }
            // With neither arm hot, ALRobotPosture picks the right- or left-arm sitting variant at
            // random by making the other one expensive.
            let avoid = if std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) & 1 == 0 { SITTING_RIGHT_ARM } else { SITTING_LEFT_ARM };
            let path = g.path_avoiding(from, name, &[avoid]).ok_or_else(|| format!("No transition to {name}"))?;
            let along: Vec<_> = path.windows(2).map(|w| step(w[1], g.speed(w[0], w[1]))).collect();
            return Ok([vec![step(from, 1.0)], along].into_iter().filter(|m| !m.is_empty()).collect());
        }
        let target = robot.posture(name).ok_or_else(|| format!("Unknown posture {name}"))?;
        let from = self.posture();
        let step = |p: &choren_robot::Posture| (p.angles.clone(), p.torso_pitch, 1.0);
        if direct(&from.family, &target.family) {
            return Ok(vec![vec![step(target)]]);
        }
        let path = robot.posture_path(&from.name, name).ok_or_else(|| format!("No transition from {} to {name}", from.name))?;
        let along: Vec<_> = path[1..].iter().map(|p| step(robot.posture(p).unwrap())).collect();
        Ok([vec![step(&from)], along].into_iter().filter(|m| !m.is_empty()).collect())
    }

    /// ALMotion.wakeUp: stiffness on and, if the robot was resting, after the stiffness ramp the
    /// Stand pose it starts in, every joint on a cubic at 15 % of its maximum velocity.
    pub async fn wake_up(&self) -> Result<bool, String> {
        let resting = self.sim.state.lock().unwrap().stiffness.iter().all(|&s| s == 0.0);
        self.sim.state.lock().unwrap().stiffness.iter_mut().for_each(|x| *x = 1.0);
        let target = self.sim.robot.initial_angles.clone();
        if !resting || target.len() != self.sim.robot.joints.len() {
            return Ok(true);
        }
        let all: Vec<usize> = (0..target.len()).collect();
        let (_, done) = self.sim.go_to_state(&all, &target, WAKE_UP_SPEED, WAKE_UP_STIFFNESS_CYCLES, Some(0.0));
        Ok(done.await.unwrap_or(false))
    }

    /// ALMotion.rest: every joint to Crouch with bang-bang moves at 10 % of its maximum velocity,
    /// then stiffness off.
    pub async fn rest(&self) -> Result<(), String> {
        let robot = &self.sim.robot;
        let graph = self.posture_graph.lock().unwrap().as_ref().and_then(|g| g.nodes.iter().find(|n| n.name == "Crouch").map(|n| (n.angles.clone(), n.inertial[1])));
        let (angles, pitch) = graph.or_else(|| robot.posture("Crouch").map(|p| (p.angles.clone(), p.torso_pitch))).ok_or("No Crouch posture")?;
        let all: Vec<usize> = (0..robot.joints.len()).collect();
        let (_, done) = self.sim.move_with_speed(&all, &angles, REST_SPEED, true);
        if done.await.unwrap_or(false) {
            let mut st = self.sim.state.lock().unwrap();
            st.torso_pitch = pitch;
            st.stiffness.iter_mut().for_each(|x| *x = 0.0);
        }
        Ok(())
    }

    /// ALRobotPosture.goToPosture: each step lasts, for its slowest joint, the cubic time at
    /// (0.9·speed·edge coefficient + 0.01) of 40 % of the joint's maximum velocity, rounded to
    /// 20 ms; the steps of a move are Bezier keys with automatic tangents.
    pub async fn go_to_posture(&self, name: &str, speed: f64) -> Result<bool, String> {
        if self.sim.robot.posture(name).is_none() {
            return Ok(false); // NAOqi answers false for an unknown posture
        }
        let robot = &self.sim.robot;
        for steps in self.posture_plan(name)? {
            let (now, pitch) = { let st = self.sim.state.lock().unwrap(); (st.angles.clone(), st.torso_pitch) };
            let mut keys: Vec<Vec<sim::Key>> = now.iter().map(|&a| vec![sim::Key::new(0.0, a)]).collect();
            let mut torso = vec![sim::Key::new(0.0, pitch)];
            let (mut prev, mut t) = (now, 0.0);
            for (angles, torso_pitch, factor) in steps {
                let s = speed * factor;
                let s = if s > 0.0 && s <= 1.0 { s } else { s.clamp(0.01, 1.0) };
                let step = (0..angles.len()).map(|j| sim::cubic_time(prev[j], angles[j], 0.0, 0.0, (0.9 * s + 0.01) * POSTURE_SPEED_SHARE * robot.joints[j].max_velocity, POSTURE_TIME_STEP)).fold(0.0, f64::max);
                t += step;
                for (j, k) in keys.iter_mut().enumerate() {
                    k.push(sim::Key::new(t, angles[j]));
                }
                torso.push(sim::Key::new(t, torso_pitch));
                prev = angles;
            }
            let curves = keys.into_iter().enumerate().map(|(j, keys)| (j, sim::Curve { keys })).collect();
            let (_, done) = self.sim.start_task(curves, Some(sim::Curve { keys: torso }));
            if !done.await.unwrap_or(false) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Simulates a touch: the sensor key goes to 1 then 0, raising its event both times.
    pub fn press(&self, sensor: &str) -> Result<(), String> {
        let t = self.sim.robot.touch.iter().find(|t| t.name == sensor || t.event == sensor).ok_or_else(|| format!("Unknown touch sensor {sensor}"))?.clone();
        self.set_data(&t.key, Value::Float(1.0));
        self.set_data(&t.event, Value::Float(1.0));
        self.set_data(&t.key, Value::Float(0.0));
        self.set_data(&t.event, Value::Float(0.0));
        self.log("info", "ChorenSim", &format!("Touched {}", t.name));
        Ok(())
    }

    /// Typed speech input for ALSpeechRecognition: raises WordRecognized (the dialog gets it from
    /// `dialog::input`).
    pub fn hear(&self, text: &str) {
        self.set_data("WordRecognized", Value::List(vec![text.into(), Value::Float(1.0)]));
        self.set_data("SpeechDetected", Value::Bool(false));
    }
}
