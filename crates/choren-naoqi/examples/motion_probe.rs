//! Measures joint motions: for each case, puts the robot in a start pose, sends one ALMotion or
//! ALRobotPosture call and samples `getAngles(joints, true)` as fast as the connection allows.
//! Prints one JSON line per case: `{"case", "joints", "done", "samples": [[t, a0, a1, …], …]}`,
//! with `t` in seconds from sending the call and `done` the time its reply arrived.
//!
//! `cargo run -p choren-naoqi --example motion_probe -- tcp://127.0.0.1:19559 [case-prefix]`
//! measures Choregraphe's virtual robot; `choren` instead of a URL
//! measures Choren's own virtual robot.
use choren_qi::{Client, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn strs(v: &[&str]) -> Value {
    Value::List(v.iter().map(|s| Value::from(*s)).collect())
}
fn fl(v: &[f64]) -> Value {
    Value::floats(v.iter().copied())
}
fn lists(v: &[&[f64]]) -> Value {
    Value::List(v.iter().map(|l| fl(l)).collect())
}
fn f(x: f64) -> Value {
    Value::Float(x as f32)
}

/// A Bezier key of angleInterpolationBezier: value with flat handles a third of the way.
fn key(v: f64, dt_left: f64, dt_right: f64) -> Value {
    let handle = |dt: f64| Value::List(vec![Value::Int(2), f(dt), f(0.0)]);
    Value::List(vec![f(v), handle(-dt_left), handle(dt_right)])
}

struct Case {
    name: &'static str,
    joints: &'static [&'static str],
    /// Start pose (radians), reached with angleInterpolation over 1.5 s.
    start: &'static [f64],
    /// Calls sent in order; each is (delay before sending, service, method, args).
    calls: Vec<(f64, &'static str, &'static str, Vec<Value>)>,
    /// How long to keep sampling after the last reply (s).
    tail: f64,
}

fn cases() -> Vec<Case> {
    let head = &["HeadYaw"][..];
    let speed = |j: &'static [&'static str], to: &[f64], frac: f64| ("ALMotion", "angleInterpolationWithSpeed", vec![strs(j), fl(to), f(frac)]);
    let set = |j: &'static [&'static str], to: &[f64], frac: f64| ("ALMotion", "setAngles", vec![strs(j), fl(to), f(frac)]);
    let at = |d: f64, c: (&'static str, &'static str, Vec<Value>)| (d, c.0, c.1, c.2);
    let mut v = vec![];
    for frac in [0.1, 0.2, 0.5, 1.0] {
        v.push(Case { name: leak(format!("speed_yaw_{frac}")), joints: head, start: &[0.0], calls: vec![at(0.0, speed(head, &[1.0], frac))], tail: 0.4 });
    }
    v.push(Case { name: "speed_yaw_short", joints: head, start: &[0.0], calls: vec![at(0.0, speed(head, &[0.1], 1.0))], tail: 0.4 });
    v.push(Case { name: "speed_yaw_neg", joints: head, start: &[0.5], calls: vec![at(0.0, speed(head, &[-1.0], 0.3))], tail: 0.4 });
    let two = &["HeadYaw", "HeadPitch"][..];
    v.push(Case { name: "speed_sync", joints: two, start: &[0.0, 0.0], calls: vec![at(0.0, speed(two, &[1.0, 0.3], 0.3))], tail: 0.4 });
    let arm = &["LShoulderPitch", "LElbowRoll"][..];
    v.push(Case { name: "speed_arm", joints: arm, start: &[1.4, -0.5], calls: vec![at(0.0, speed(arm, &[0.0, -1.2], 0.4))], tail: 0.4 });
    v.push(Case { name: "speed_limit", joints: head, start: &[0.0], calls: vec![at(0.0, speed(head, &[2.08], 1.0))], tail: 0.4 });
    v.push(Case { name: "speed_interrupt", joints: head, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "post.angleInterpolationWithSpeed", vec![strs(head), fl(&[1.0]), f(0.2)])), at(0.3, speed(head, &[-0.5], 0.2))], tail: 0.4 });
    for frac in [0.1, 0.5] {
        v.push(Case { name: leak(format!("set_yaw_{frac}")), joints: head, start: &[0.0], calls: vec![at(0.0, set(head, &[1.0], frac))], tail: 1.6 / frac.min(1.0) / 8.0 + 0.6 });
    }
    v.push(Case { name: "set_arm", joints: arm, start: &[1.4, -0.5], calls: vec![at(0.0, set(arm, &[0.0, -1.2], 0.2))], tail: 1.6 });
    v.push(Case { name: "set_retarget", joints: head, start: &[0.0], calls: vec![at(0.0, set(head, &[1.0], 0.2)), at(0.25, set(head, &[-0.5], 0.2))], tail: 1.5 });
    v.push(Case { name: "change_yaw", joints: head, start: &[0.2], calls: vec![at(0.0, ("ALMotion", "changeAngles", vec![strs(head), fl(&[0.6]), f(0.3)]))], tail: 1.0 });
    v.push(Case { name: "interp_yaw_keys", joints: head, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "angleInterpolation", vec![strs(head), fl(&[0.5, 1.0, 0.2]), fl(&[0.5, 1.0, 1.7]), Value::Bool(true)]))], tail: 0.4 });
    v.push(Case { name: "interp_yaw_one", joints: head, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "angleInterpolation", vec![strs(head), fl(&[1.0]), fl(&[1.0]), Value::Bool(true)]))], tail: 0.4 });
    v.push(Case { name: "interp_two", joints: two, start: &[0.0, 0.0], calls: vec![at(0.0, ("ALMotion", "angleInterpolation", vec![strs(two), lists(&[&[0.8], &[0.2, -0.2]]), lists(&[&[1.2], &[0.5, 1.0]]), Value::Bool(true)]))], tail: 0.4 });
    let bez = |vals: &[(f64, f64)]| {
        let mut times = vec![];
        let mut keys = vec![];
        let mut prev = 0.0;
        for &(t, v) in vals {
            let d = (t - prev) / 3.0;
            times.push(t);
            keys.push(key(v, d, d));
            prev = t;
        }
        vec![strs(head), Value::List(vec![fl(&times)]), Value::List(vec![Value::List(keys)])]
    };
    v.push(Case { name: "bezier_yaw", joints: head, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "angleInterpolationBezier", bez(&[(0.6, 0.8), (1.2, -0.4)])))], tail: 0.4 });
    v.push(Case { name: "bezier_fast", joints: head, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "angleInterpolationBezier", bez(&[(0.12, 1.5)])))], tail: 0.6 });
    let hand = &["LHand"][..];
    v.push(Case { name: "hand_open", joints: hand, start: &[0.0], calls: vec![at(0.0, ("ALMotion", "openHand", vec!["LHand".into()]))], tail: 0.6 });
    v.push(Case { name: "hand_close", joints: hand, start: &[1.0], calls: vec![at(0.0, ("ALMotion", "closeHand", vec!["LHand".into()]))], tail: 0.6 });
    v
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Whole-body cases, sampled on all joints: postures, rest and wakeUp.
fn body_cases() -> Vec<(&'static str, Vec<(f64, &'static str, &'static str, Vec<Value>)>)> {
    let posture = |name: &str, speed: f64| (0.0, "ALRobotPosture", "goToPosture", vec![Value::from(name), f(speed)]);
    vec![
        ("posture_standinit_1.0", vec![posture("StandInit", 1.0)]),
        ("posture_crouch_0.5", vec![posture("Crouch", 0.5)]),
        ("posture_stand_0.8", vec![posture("Stand", 0.8)]),
        ("posture_sit_0.6", vec![posture("Sit", 0.6)]),
        ("posture_standinit_0.6", vec![posture("StandInit", 0.6)]),
        ("rest", vec![(0.0, "ALMotion", "rest", vec![])]),
        ("wakeup", vec![(0.0, "ALMotion", "wakeUp", vec![])]),
    ]
}

async fn sample(c: Arc<Client>, joints: Value, calls: Vec<(f64, &'static str, &'static str, Vec<Value>)>, tail: f64) -> Result<(f64, Vec<Vec<f64>>), String> {
    let t0 = Instant::now();
    let caller = {
        let c = c.clone();
        tokio::spawn(async move {
            for (delay, service, method, args) in calls {
                tokio::time::sleep_until((t0 + Duration::from_secs_f64(delay)).into()).await;
                let (method, post) = match method.strip_prefix("post.") { Some(m) => (m, true), None => (method, false) };
                if post {
                    let c = c.clone();
                    tokio::spawn(async move { let _ = c.call(service, method, args).await; });
                } else if let Err(e) = c.call(service, method, args).await {
                    eprintln!("{service}.{method}: {e}");
                }
            }
            t0.elapsed().as_secs_f64()
        })
    };
    let mut out = vec![];
    let mut done = None;
    loop {
        let t = t0.elapsed().as_secs_f64();
        let a = c.call("ALMotion", "getAngles", vec![joints.clone(), Value::Bool(true)]).await?;
        let mut row = vec![t];
        row.extend(a.items().iter().filter_map(Value::as_f64));
        out.push(row);
        if done.is_none() && caller.is_finished() {
            done = Some(t);
        }
        if done.is_some_and(|d| t > d + tail) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let done = caller.await.map_err(|e| e.to_string())?;
    Ok((done, out))
}

fn row_json(r: &[f64]) -> String {
    format!("[{}]", r.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join(","))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "tcp://127.0.0.1:19559".into());
    let filter = args.next().unwrap_or_default();
    let _vr;
    let c = Arc::new(if url == "choren" {
        let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await?;
        // The install's posture graph, as on Choregraphe's virtual robot (CHOREGRAPHE_SHARE or the
        // checkout's bundle).
        let share = std::env::var_os("CHOREGRAPHE_SHARE").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
        });
        if let Err(e) = vr.robotd.load_posture_graph(&share.join("../naoqi/robot_posture/postures_naoh25.xml")) {
            eprintln!("posture graph not loaded: {e}");
        }
        let c = Client::connect(&vr.addr.to_string()).await?;
        _vr = vr;
        c
    } else {
        let c = Client::connect(&url).await?;
        // Autonomous life and its idle motions would move the head during the measurements.
        let _ = c.call("ALAutonomousLife", "setState", vec!["disabled".into()]).await;
        for chain in ["Body", "Arms", "Legs", "Head"] {
            let _ = c.call("ALMotion", "setBreathEnabled", vec![chain.into(), Value::Bool(false)]).await;
            let _ = c.call("ALMotion", "setIdlePostureEnabled", vec![chain.into(), Value::Bool(false)]).await;
        }
        c
    });
    c.call("ALMotion", "wakeUp", vec![]).await?;
    eprintln!("cycle {:?}", c.call("ALMotion", "getMotionCycleTime", vec![]).await?);
    for case in cases().into_iter().filter(|c| c.name.starts_with(&filter)) {
        c.call("ALRobotPosture", "goToPosture", vec!["Stand".into(), f(0.8)]).await?;
        c.call("ALMotion", "angleInterpolation", vec![strs(case.joints), Value::List(case.start.iter().map(|&a| f(a)).collect()), Value::List(case.start.iter().map(|_| f(1.5)).collect()), Value::Bool(true)]).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (done, rows) = sample(c.clone(), strs(case.joints), case.calls, case.tail).await?;
        println!("{{\"case\":\"{}\",\"joints\":{:?},\"done\":{done:.5},\"samples\":[{}]}}", case.name, case.joints, rows.iter().map(|r| row_json(r)).collect::<Vec<_>>().join(","));
    }
    let names: Vec<String> = c.call("ALMotion", "getBodyNames", vec!["Body".into()]).await?.items().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    let body = Value::List(names.iter().map(|s| Value::from(s.as_str())).collect());
    for (name, calls) in body_cases().into_iter().filter(|c| c.0.starts_with(&filter)) {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (done, rows) = sample(c.clone(), body.clone(), calls, 0.3).await?;
        println!("{{\"case\":\"{name}\",\"joints\":{names:?},\"done\":{done:.5},\"samples\":[{}]}}", rows.iter().map(|r| row_json(r)).collect::<Vec<_>>().join(","));
    }
    Ok(())
}
