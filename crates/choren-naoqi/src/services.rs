//! Simulated NAOqi services. Method names and signatures follow the public NAOqi 2.8 API; the
//! behavior is the virtual robot's: motion moves the kinematic simulator, speech and LEDs are
//! reported to the editor through `ChorenSim` signals.

use crate::sim::{Curve, Key, MoveCmd};
use crate::Robotd;
use choren_qi::{ServiceBuilder, Value};
use std::sync::Arc;
use std::time::Duration;

fn s(a: &[Value], i: usize) -> Result<String, String> {
    a.get(i).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("argument {} must be a string", i + 1))
}

fn f(a: &[Value], i: usize) -> Result<f64, String> {
    a.get(i).and_then(Value::as_f64).ok_or_else(|| format!("argument {} must be a number", i + 1))
}

fn names(v: &Value) -> Vec<String> {
    v.items().iter().filter_map(|x| x.as_str().map(String::from)).collect()
}

fn floats(v: &Value) -> Vec<f64> {
    v.items().iter().filter_map(Value::as_f64).collect()
}

const VOID: Result<Value, String> = Ok(Value::Void);

/// What `ALMotion.getMotionCycleTime` reports on Choregraphe's virtual robot.
const MOTION_CYCLE_MS: i64 = 24;

/// Named colors accepted by ALLeds.fadeRGB(name, colorName, duration).
fn color(name: &str) -> Option<u32> {
    Some(match name {
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x00ff00,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "magenta" => 0xff00ff,
        "cyan" => 0x00ffff,
        _ => return None,
    })
}

pub fn register(d: &Arc<Robotd>) {
    let node = &d.node;
    let ctx = || d.clone();

    // ALMemory: a key/value store; every change also raises the internal `_changed` signal that
    // the behavior runtime and the editor use for event inputs and the memory watcher.
    let (c1, c2, c3, c4, c5, c6, c7) = (ctx(), ctx(), ctx(), ctx(), ctx(), ctx(), ctx());
    node.register(
        ServiceBuilder::new("ALMemory")
            .method("getData", "(s)", "m", move |a| c1.get_data(&s(&a, 0)?))
            .method("getData", "(si)", "m", {
                let c = ctx();
                move |a| c.get_data(&s(&a, 0)?)
            })
            .method("getListData", "(m)", "m", move |a| Ok(Value::List(names(&a[0]).iter().map(|k| c2.get_data(k).unwrap_or(Value::Void)).collect())))
            .method("insertData", "(sm)", "v", move |a| {
                c3.set_data(&s(&a, 0)?, a[1].plain().clone());
                VOID
            })
            .method("insertListData", "(m)", "v", move |a| {
                for pair in a[0].items() {
                    let p = pair.items();
                    if let (Some(k), Some(v)) = (p.first().and_then(Value::as_str), p.get(1)) {
                        c4.set_data(k, v.plain().clone());
                    }
                }
                VOID
            })
            .method("raiseEvent", "(sm)", "v", move |a| {
                let key = s(&a, 0)?;
                c5.events.lock().unwrap().insert(key.clone());
                c5.set_data(&key, a[1].plain().clone());
                VOID
            })
            .method("raiseMicroEvent", "(sm)", "v", {
                let c = ctx();
                move |a| {
                    c.set_data(&s(&a, 0)?, a[1].plain().clone());
                    VOID
                }
            })
            .method("declareEvent", "(s)", "v", {
                let c = ctx();
                move |a| c.declare_event(&s(&a, 0)?)
            })
            .method("declareEvent", "(ss)", "v", {
                let c = ctx();
                move |a| c.declare_event(&s(&a, 0)?)
            })
            .method("removeData", "(s)", "v", move |a| {
                c6.memory.lock().unwrap().remove(&s(&a, 0)?);
                VOID
            })
            .method("getDataList", "(s)", "[s]", move |a| {
                let filter = s(&a, 0)?;
                Ok(Value::List(c7.memory.lock().unwrap().keys().filter(|k| k.contains(&filter)).map(|k| k.as_str().into()).collect()))
            })
            .method("getDataListName", "()", "[s]", {
                let c = ctx();
                move |_| Ok(Value::List(c.memory.lock().unwrap().keys().map(|k| k.as_str().into()).collect()))
            })
            .method("getEventList", "()", "[s]", {
                let c = ctx();
                move |_| Ok(Value::List(c.events.lock().unwrap().iter().map(|k| k.as_str().into()).collect()))
            })
            // Legacy module callbacks are delivered by the behavior runtime (Python host).
            .method("subscribeToEvent", "(sss)", "v", {
                let c = ctx();
                move |a| c.runtime.subscribe_module(&s(&a, 0)?, &s(&a, 1)?, &s(&a, 2)?)
            })
            .method("unsubscribeToEvent", "(ss)", "v", {
                let c = ctx();
                move |a| c.runtime.unsubscribe_module(&s(&a, 0)?, &s(&a, 1)?)
            })
            .method("subscribeToMicroEvent", "(ssss)", "v", {
                let c = ctx();
                move |a| c.runtime.subscribe_module(&s(&a, 0)?, &s(&a, 1)?, &s(&a, 3).unwrap_or_else(|_| "onEvent".into()))
            })
            .method("unsubscribeToMicroEvent", "(ss)", "v", {
                let c = ctx();
                move |a| c.runtime.unsubscribe_module(&s(&a, 0)?, &s(&a, 1)?)
            })
            .signal("_changed", "(sm)"),
    );

    // ALMotion
    let sim = || d.sim.clone();
    node.register(
        ServiceBuilder::new("ALMotion")
            .method_async("angleInterpolation", "(mmmb)", "v", {
                let sim = sim();
                move |a| {
                    let sim = sim.clone();
                    async move {
                        let joints = sim.robot.expand(&names(&a[0]))?;
                        let current = sim.angles();
                        let absolute = a[3].as_bool().unwrap_or(true);
                        // One joint: a value or a list. Several joints: one entry (value or list) each.
                        let per = |v: &Value, n: usize| -> Vec<f64> { if joints.len() == 1 { floats(v) } else { v.items().get(n).map(floats).unwrap_or_default() } };
                        let mut keys = vec![];
                        for (n, &j) in joints.iter().enumerate() {
                            let (angles, times) = (per(&a[1], n), per(&a[2], n));
                            if angles.len() != times.len() || angles.is_empty() {
                                return Err("angleInterpolation: angles and times must have the same length".into());
                            }
                            keys.push(times.into_iter().zip(angles).map(|(t, v)| (t, if absolute { v } else { current[j] + v })).collect::<Vec<_>>());
                        }
                        let (_, done) = sim.interpolate(&joints, &keys)?;
                        let _ = done.await;
                        VOID
                    }
                }
            })
            .method_async("angleInterpolationWithSpeed", "(mmf)", "v", {
                let sim = sim();
                move |a| {
                    let sim = sim.clone();
                    async move {
                        let joints = sim.robot.expand(&names(&a[0]))?;
                        let targets = broadcast(floats(&a[1]), joints.len())?;
                        let (_, done) = sim.move_with_speed(&joints, &targets, f(&a, 2)?, false);
                        let _ = done.await;
                        VOID
                    }
                }
            })
            .method_async("angleInterpolationBezier", "([s]mm)", "v", {
                let sim = sim();
                move |a| {
                    let sim = sim.clone();
                    async move {
                        let joints = sim.robot.expand(&names(&a[0]))?;
                        let (times, keys) = (a[1].items(), a[2].items());
                        let current = sim.angles();
                        let mut curves = vec![];
                        for (n, &j) in joints.iter().enumerate() {
                            let ts = floats(times.get(n).ok_or("missing times")?);
                            let mut out = vec![Key::new(0.0, current[j])];
                            for (k, t) in keys.get(n).ok_or("missing keys")?.items().iter().zip(ts) {
                                let k = k.items();
                                let handle = |i: usize| k.get(i).map(|h| h.items()).filter(|h| h.len() == 3).map(|h| (h[1].as_f64().unwrap_or(0.0), h[2].as_f64().unwrap_or(0.0)));
                                out.push(Key { left: handle(1), right: handle(2), ..Key::new(t, k.first().and_then(Value::as_f64).unwrap_or(0.0)) });
                            }
                            curves.push((j, Curve { keys: out }));
                        }
                        let (_, done) = sim.start_task(curves, None);
                        let _ = done.await;
                        VOID
                    }
                }
            })
            .method("setAngles", "(mmf)", "v", {
                let sim = sim();
                move |a| {
                    let joints = sim.robot.expand(&names(&a[0]))?;
                    let targets = broadcast(floats(&a[1]), joints.len())?;
                    sim.set_angles(&joints, &targets, f(&a, 2)?);
                    VOID
                }
            })
            .method("changeAngles", "(mmf)", "v", {
                let sim = sim();
                move |a| {
                    let joints = sim.robot.expand(&names(&a[0]))?;
                    let cur = sim.angles();
                    let targets: Vec<f64> = broadcast(floats(&a[1]), joints.len())?.iter().zip(&joints).map(|(d, &j)| cur[j] + d).collect();
                    sim.set_angles(&joints, &targets, f(&a, 2)?);
                    VOID
                }
            })
            .method("getAngles", "(mb)", "[f]", {
                let sim = sim();
                move |a| {
                    let joints = sim.robot.expand(&names(&a[0]))?;
                    let cur = sim.angles();
                    Ok(Value::floats(joints.iter().map(|&j| cur[j])))
                }
            })
            .method("setStiffnesses", "(mm)", "v", {
                let sim = sim();
                move |a| {
                    let joints = sim.robot.expand(&names(&a[0]))?;
                    let v = broadcast(floats(&a[1]), joints.len())?;
                    let mut st = sim.state.lock().unwrap();
                    for (j, x) in joints.iter().zip(v) {
                        st.stiffness[*j] = x.clamp(0.0, 1.0);
                    }
                    VOID
                }
            })
            .method_async("stiffnessInterpolation", "(mmm)", "v", {
                let sim = sim();
                move |a| {
                    let sim = sim.clone();
                    async move {
                        let joints = sim.robot.expand(&names(&a[0]))?;
                        let last = |v: &Value| floats(v).last().copied().or_else(|| v.items().last().and_then(|x| floats(x).last().copied())).unwrap_or(0.0);
                        tokio::time::sleep(Duration::from_secs_f64(last(&a[2]).clamp(0.0, 30.0))).await;
                        let v = last(&a[1]).clamp(0.0, 1.0);
                        let mut st = sim.state.lock().unwrap();
                        for j in joints {
                            st.stiffness[j] = v;
                        }
                        VOID
                    }
                }
            })
            .method("getStiffnesses", "(m)", "[f]", {
                let sim = sim();
                move |a| {
                    let joints = sim.robot.expand(&names(&a[0]))?;
                    let st = sim.state.lock().unwrap();
                    Ok(Value::floats(joints.iter().map(|&j| st.stiffness[j])))
                }
            })
            .method_async("wakeUp", "()", "b", {
                let d = ctx();
                move |_| {
                    let d = d.clone();
                    async move { d.wake_up().await.map(Value::Bool) }
                }
            })
            .method_async("rest", "()", "v", {
                let d = ctx();
                move |_| {
                    let d = d.clone();
                    async move {
                        d.rest().await?;
                        VOID
                    }
                }
            })
            .method("robotIsWakeUp", "()", "b", {
                let sim = sim();
                move |_| Ok(Value::Bool(sim.state.lock().unwrap().stiffness.iter().any(|&x| x > 0.0)))
            })
            // Walking: every overload of move/moveToward/setWalkTargetVelocity/moveTo/walkTo, with
            // move configurations accepted and ignored.
            .method("move", "(fff)", "v", velocity(sim(), false))
            .method("move", "(fffm)", "v", velocity(sim(), false))
            .method("moveToward", "(fff)", "v", velocity(sim(), true))
            .method("moveToward", "(fffm)", "v", velocity(sim(), true))
            .method("setWalkTargetVelocity", "(ffff)", "v", velocity(sim(), true))
            .method("setWalkTargetVelocity", "(ffffm)", "v", velocity(sim(), true))
            .method("setWalkTargetVelocity", "(ffffmm)", "v", velocity(sim(), true))
            .method_async("moveTo", "(fff)", "b", move_to(sim()))
            .method_async("moveTo", "(ffff)", "b", move_to(sim()))
            .method_async("moveTo", "(fffm)", "b", move_to(sim()))
            .method_async("moveTo", "(ffffm)", "b", move_to(sim()))
            .method_async("moveTo", "(m)", "b", move_to(sim()))
            .method_async("moveTo", "(mm)", "b", move_to(sim()))
            .method_async("walkTo", "(fff)", "b", move_to(sim()))
            .method_async("walkTo", "(fffm)", "b", move_to(sim()))
            .method_async("walkTo", "(m)", "b", move_to(sim()))
            .method_async("walkTo", "(mm)", "b", move_to(sim()))
            .method("moveInit", "()", "v", |_| VOID)
            .method("walkInit", "()", "v", |_| VOID)
            .method("stopMove", "()", "b", stop_moving(sim(), Value::Bool(true)))
            .method("stopWalk", "()", "b", stop_moving(sim(), Value::Bool(true)))
            .method("killMove", "()", "v", stop_moving(sim(), Value::Void))
            .method("killWalk", "()", "v", stop_moving(sim(), Value::Void))
            .method_async("waitUntilMoveIsFinished", "()", "v", wait_moving(sim()))
            .method_async("waitUntilWalkIsFinished", "()", "v", wait_moving(sim()))
            .method("moveIsActive", "()", "b", {
                let sim = sim();
                move |_| Ok(Value::Bool(sim.state.lock().unwrap().moving.is_some()))
            })
            .method("walkIsActive", "()", "b", {
                let sim = sim();
                move |_| Ok(Value::Bool(sim.state.lock().unwrap().moving.is_some()))
            })
            .method("getRobotVelocity", "()", "[f]", {
                let sim = sim();
                move |_| {
                    let v = sim.state.lock().unwrap().moving.as_ref().map(|m| m.velocity).unwrap_or_default();
                    Ok(Value::floats([v.0, v.1, v.2]))
                }
            })
            .method("getRobotPosition", "(b)", "[f]", {
                let sim = sim();
                move |_| {
                    let (x, y, t) = sim.state.lock().unwrap().pose;
                    Ok(Value::floats([x, y, t]))
                }
            })
            .method("getBodyNames", "(s)", "[s]", {
                let sim = sim();
                move |a| {
                    let name = s(&a, 0)?;
                    if name == "Chains" {
                        return Ok(Value::List(sim.robot.chain_order.iter().map(|c| c.as_str().into()).collect()));
                    }
                    Ok(Value::List(sim.robot.expand(&[name])?.iter().map(|&j| sim.robot.joints[j].name.as_str().into()).collect()))
                }
            })
            .method("getJointNames", "(s)", "[s]", {
                let sim = sim();
                move |a| Ok(Value::List(sim.robot.expand(&[s(&a, 0)?])?.iter().map(|&j| sim.robot.joints[j].name.as_str().into()).collect()))
            })
            .method("getLimits", "(s)", "m", {
                let sim = sim();
                move |a| {
                    let js = sim.robot.expand(&[s(&a, 0)?])?;
                    Ok(Value::List(js.iter().map(|&j| { let x = &sim.robot.joints[j]; Value::floats([x.min, x.max, x.max_velocity, x.max_torque]) }).collect()))
                }
            })
            .method_async("openHand", "(s)", "v", move_hand(sim(), 1.0))
            .method_async("closeHand", "(s)", "v", move_hand(sim(), 0.0))
            .method("killAll", "()", "v", {
                let sim = sim();
                move |_| {
                    sim.kill_all();
                    VOID
                }
            })
            .method("killTasksUsingResources", "([s])", "v", |_| VOID)
            .method("getTaskList", "()", "m", {
                let sim = sim();
                move |_| Ok(Value::List(sim.state.lock().unwrap().tasks.iter().map(|t| Value::List(vec!["task".into(), Value::Int(t.id as i64)])).collect()))
            })
            // Settings with no effect on a kinematic robot: accepted and remembered nowhere.
            .method("setBreathEnabled", "(sb)", "v", |_| VOID)
            .method("getBreathEnabled", "(s)", "b", |_| Ok(Value::Bool(false)))
            .method("setIdlePostureEnabled", "(sb)", "v", |_| VOID)
            .method("setMoveArmsEnabled", "(bb)", "v", |_| VOID)
            .method("getMoveArmsEnabled", "(s)", "b", |_| Ok(Value::Bool(true)))
            .method("setExternalCollisionProtectionEnabled", "(sb)", "v", |_| VOID)
            .method("setFallManagerEnabled", "(b)", "v", |_| VOID)
            .method("setSmartStiffnessEnabled", "(b)", "v", |_| VOID)
            .method("setCollisionProtectionEnabled", "(sb)", "b", |_| Ok(Value::Bool(true)))
            // NAOqi's text table: body joints, then the hands; no tasks are listed.
            .method("getSummary", "()", "s", {
                let sim = sim();
                move |_| {
                    let r = &sim.robot;
                    let st = sim.state.lock().unwrap();
                    let mut out = String::from("---------------------- Model ---------------------------\n        BodyName   Stiffness     Command      Sensor\n");
                    let order = r.body_groups.get("BodyJoints").into_iter().chain(r.body_groups.get("BodyActuators")).flatten();
                    for name in order {
                        if let Some(j) = r.joint_index(name) {
                            out += &format!("{name:>16}    {:.6}   {:>9.6}   {:>9.6}\n", st.stiffness[j], st.angles[j], st.angles[j]);
                        }
                    }
                    out += &format!("---------------------- Tasks  --------------------------\n            Name         ID\n----------------- Motion Cycle Time --------------------\n{:>16} ms", MOTION_CYCLE_MS);
                    Ok(out.into())
                }
            }),
    );

    node.register(
        ServiceBuilder::new("ALRobotPosture")
            .method_async("goToPosture", "(sf)", "b", {
                let d = ctx();
                move |a| {
                    let d = d.clone();
                    async move { d.go_to_posture(&s(&a, 0)?, f(&a, 1)?).await.map(Value::Bool) }
                }
            })
            // An unknown posture name returns false, as NAOqi does.
            .method_async("applyPosture", "(sf)", "b", {
                let d = ctx();
                move |a| {
                    let d = d.clone();
                    async move { d.go_to_posture(&s(&a, 0)?, f(&a, 1)?).await.map(Value::Bool) }
                }
            })
            .method("getPosture", "()", "s", {
                let d = ctx();
                move |_| Ok(d.posture().name.as_str().into())
            })
            .method("getPostureFamily", "()", "s", {
                let d = ctx();
                move |_| Ok(d.posture().family.as_str().into())
            })
            .method("getPostureList", "()", "[s]", {
                let d = ctx();
                move |_| {
                    let mut names: Vec<&str> = d.sim.robot.postures.iter().map(|p| p.name.as_str()).collect();
                    names.sort();
                    Ok(Value::List(names.into_iter().map(Value::from).collect()))
                }
            })
            .method("stopMove", "()", "v", {
                let d = ctx();
                move |_| {
                    d.sim.kill_all();
                    VOID
                }
            })
            .method("setMaxTryNumber", "(i)", "v", |_| VOID),
    );
    node.register(ServiceBuilder::new("ALRobotPose").method("getActualPoseAndTime", "()", "m", {
        let d = ctx();
        move |_| Ok(Value::List(vec![d.posture().name.as_str().into(), Value::Float(1.0)]))
    }));
    node.register(
        ServiceBuilder::new("ALRobotModel")
            .method("getRobotType", "()", "s", {
                let d = ctx();
                move |_| Ok(d.sim.robot.family.as_str().into())
            })
            .method("hasLegs", "()", "b", |_| Ok(Value::Bool(true)))
            .method("hasArms", "()", "b", |_| Ok(Value::Bool(true)))
            .method("hasHands", "()", "b", |_| Ok(Value::Bool(true)))
            // The robot's RobotConfig preferences as NAOqi's ModulePreference XML.
            .method("getConfig", "()", "s", {
                let d = ctx();
                move |_| {
                    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" ?><ModulePreference name="aldebaran-robotics.com@Device_Head" xmlns="http://www.aldebaran-robotics.com/ns/ALPreference" schemaLocation="ModulePreference.xsd">"#);
                    for (i, (k, v)) in d.sim.robot.robot_config.iter().enumerate() {
                        xml += &format!(r#"<Preference name="Key{}" memoryName="{k}" value="{v}" type="string" />"#, i + 1);
                    }
                    Ok((xml + "</ModulePreference>").into())
                }
            }),
    );

    // Speech: the text is reported to the editor; its duration is estimated from its length.
    let say = |d: Arc<Robotd>| {
        move |a: Vec<Value>| {
            let d = d.clone();
            async move {
                let text = s(&a, 0)?;
                let clean = strip_annotations(&text);
                d.set_data("ALTextToSpeech/TextStarted", Value::Int(1));
                d.set_data("ALTextToSpeech/CurrentSentence", clean.as_str().into());
                d.node.emit("ChorenSim", "spoken", vec![clean.as_str().into()]);
                d.log("info", "ALTextToSpeech", &format!("Say: {clean}"));
                let secs = (clean.chars().count() as f64 * 0.065).clamp(0.4, 30.0);
                tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                d.set_data("ALTextToSpeech/TextDone", Value::Int(1));
                VOID
            }
        }
    };
    let lang = Arc::new(std::sync::Mutex::new("English".to_string()));
    let (l1, l2) = (lang.clone(), lang.clone());
    let volume = Arc::new(std::sync::Mutex::new(1.0f32));
    let (v1, v2) = (volume.clone(), volume);
    node.register(
        ServiceBuilder::new("ALTextToSpeech")
            .method_async("say", "(s)", "v", say(ctx()))
            .method_async("say", "(ss)", "v", say(ctx()))
            // Languages are those getAvailableLanguages lists (api/nao6-defaults.json).
            .method("setLanguage", "(s)", "v", move |a| {
                let name = s(&a, 0)?;
                let available = crate::conformance::answer("ALTextToSpeech", "getAvailableLanguages", &[]).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                if !available.iter().any(|l| l.as_str() == Some(name.as_str())) {
                    let low = name.to_lowercase();
                    return Err(format!("\tALTextToSpeechCore::setLanguage\n\t{name} language is not supported: \tALTextToSpeechCore::xFormatLanguage\n\t{low} language is not supported."));
                }
                *l1.lock().unwrap() = name;
                VOID
            })
            .method("getLanguage", "()", "s", move |_| Ok(l2.lock().unwrap().as_str().into()))
            // NAOqi keeps the volume and only rejects negative values (1.5 is accepted).
            .method("setVolume", "(f)", "v", move |a| {
                let v = f(&a, 0)? as f32;
                if v < 0.0 {
                    return Err("\tALTextToSpeechCore::setVolume\n\tWrong value for volume, must be between 0.0 and 1.0".into());
                }
                *v1.lock().unwrap() = v;
                VOID
            })
            .method("getVolume", "()", "f", move |_| Ok(Value::Float(*v2.lock().unwrap())))
            // Without a speech engine NAOqi keeps no parameters: it reports these values.
            .method("setParameter", "(sf)", "v", |_| VOID)
            .method("getParameter", "(s)", "f", |a| {
                Ok(Value::Float(match s(&a, 0)?.as_str() {
                    "defaultVoiceSpeed" => 100.0,
                    "pitchShift" | "doubleVoice" | "doubleVoiceLevel" | "doubleVoiceTimeShift" => 0.0,
                    _ => -1.0,
                }))
            })
            .method("stopAll", "()", "v", |_| VOID),
    );
    node.register(
        ServiceBuilder::new("ALAnimatedSpeech")
            .method_async("say", "(s)", "v", say(ctx()))
            .method_async("say", "(sm)", "v", say(ctx()))
            .method("setBodyLanguageMode", "(I)", "v", |_| VOID)
            .method("setBodyLanguageModeFromStr", "(s)", "v", |_| VOID),
    );

    // LEDs: every ALLeds name (groups, LEDs, actuators) from the descriptor's NAOqi 2.8 table sets
    // actuator intensities; the editor shows group colours. Fades jump to the target after the
    // duration.
    let fade = |d: Arc<Robotd>| {
        move |a: Vec<Value>| {
            let d = d.clone();
            async move {
                let name = s(&a, 0)?;
                let rgb = match a.len() {
                    3 => match a[1].plain() {
                        Value::Str(n) => color(n).ok_or_else(|| format!("Color given as parameter {n} is not supported. Supported colors are: white, red, green, blue, yellow, magenta, cyan"))?,
                        v => v.as_i64().ok_or("color must be a number")? as u32,
                    },
                    5 => ((f(&a, 1)?.clamp(0.0, 1.0) * 255.0) as u32) << 16 | ((f(&a, 2)?.clamp(0.0, 1.0) * 255.0) as u32) << 8 | (f(&a, 3)?.clamp(0.0, 1.0) * 255.0) as u32,
                    _ => return Err("fadeRGB expects (name, rgb, duration) or (name, r, g, b, duration)".into()),
                };
                d.led_actuators("fadeRGB", &name)?;
                let secs = f(&a, a.len() - 1)?.clamp(0.0, 60.0);
                tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                d.set_led("fadeRGB", &name, rgb)?;
                VOID
            }
        }
    };
    let set_level = |d: Arc<Robotd>, method: &'static str, level: Option<f32>| {
        move |a: Vec<Value>| {
            let level = match level { Some(l) => l, None => f(&a, 1)? as f32 };
            d.set_led_intensity(method, &s(&a, 0)?, level).map(|_| Value::Void)
        }
    };
    let wait = |i: usize| move |a: Vec<Value>| async move {
        tokio::time::sleep(Duration::from_secs_f64(f(&a, i)?.clamp(0.0, 60.0))).await;
        VOID
    };
    node.register(
        ServiceBuilder::new("ALLeds")
            .method_async("fadeRGB", "(sif)", "v", fade(ctx()))
            .method_async("fadeRGB", "(ssf)", "v", fade(ctx()))
            .method_async("fadeRGB", "(sffff)", "v", fade(ctx()))
            .method_async("fade", "(sff)", "v", {
                let d = ctx();
                move |a| {
                    let d = d.clone();
                    async move {
                        let name = s(&a, 0)?;
                        d.led_actuators("fade", &name)?;
                        tokio::time::sleep(Duration::from_secs_f64(f(&a, 2)?.clamp(0.0, 60.0))).await;
                        d.set_led_intensity("fade", &name, f(&a, 1)? as f32)?;
                        VOID
                    }
                }
            })
            .method_async("fadeListRGB", "(smm)", "v", {
                let d = ctx();
                move |a| {
                    let d = d.clone();
                    async move {
                        let name = s(&a, 0)?;
                        d.led_actuators("fadeListRGB", &name)?;
                        let (colors, times) = (a[1].plain().items().to_vec(), floats(a[2].plain()));
                        let mut elapsed = 0.0;
                        for (c, t) in colors.iter().zip(&times) {
                            tokio::time::sleep(Duration::from_secs_f64((t - elapsed).clamp(0.0, 60.0))).await;
                            elapsed = *t;
                            d.set_led("fadeListRGB", &name, c.as_i64().unwrap_or(0) as u32)?;
                        }
                        VOID
                    }
                }
            })
            .method("on", "(s)", "v", set_level(ctx(), "on", Some(1.0)))
            .method("off", "(s)", "v", set_level(ctx(), "off", Some(0.0)))
            .method("setIntensity", "(sf)", "v", set_level(ctx(), "setIntensity", None))
            .method("reset", "(s)", "v", set_level(ctx(), "reset", Some(1.0)))
            .method("getIntensity", "(s)", "m", {
                let d = ctx();
                move |a| Ok(Value::Float(d.led_intensity(&s(&a, 0)?)?))
            })
            .method("createGroup", "(s[s])", "v", |a| Err(format!("ALLeds: createGroup: groups cannot be created on the virtual robot ({})", s(&a, 0)?)))
            .method_async("earLedsSetAngle", "(ifb)", "v", wait(1))
            .method_async("rasta", "(f)", "v", wait(0))
            .method_async("rotateEyes", "(iff)", "v", wait(2))
            .method_async("randomEyes", "(f)", "v", wait(0))
            .method("listGroups", "()", "[s]", {
                let d = ctx();
                move |_| Ok(Value::List(d.sim.robot.leds.groups.keys().map(|k| k.as_str().into()).collect()))
            })
            .method("listLEDs", "()", "[s]", {
                let d = ctx();
                move |_| Ok(Value::List(d.sim.robot.leds.leds.keys().map(|k| k.as_str().into()).collect()))
            })
            .method("listGroup", "(s)", "[s]", {
                let d = ctx();
                move |a| {
                    let group = d.sim.robot.leds.groups.get(&s(&a, 0)?).ok_or("ALLeds: listGroup: Could not parse pName as a group name")?;
                    Ok(Value::List(group.iter().map(|k| k.as_str().into()).collect()))
                }
            })
            .method("listLED", "(s)", "[s]", {
                let d = ctx();
                move |a| Ok(Value::List(d.sim.robot.leds.leds.get(&s(&a, 0)?).into_iter().flatten().map(|k| k.as_str().into()).collect()))
            }),
    );

    // Audio: playback is simulated by waiting for the file's duration (WAV header, else 1 s). A
    // missing file fails as on NAOqi; loops run until the task is stopped.
    let play = |d: Arc<Robotd>, looped: bool, from_position: bool| {
        move |a: Vec<Value>| {
            let d = d.clone();
            async move {
                let path = s(&a, 0)?;
                if !std::path::Path::new(&path).is_file() {
                    return Err(format!("\tALAudioPlayerTask::xIsAWavFile()\n\tCannot open the wav file \"{path}\""));
                }
                d.log("info", "ALAudioPlayer", &format!("Play {path}"));
                let length = wav_seconds(&path).unwrap_or(1.0);
                let start = if from_position { f(&a, 1)?.clamp(0.0, length) } else { 0.0 };
                loop {
                    tokio::time::sleep(Duration::from_secs_f64(length - start)).await;
                    if !looped {
                        return VOID;
                    }
                }
            }
        }
    };
    node.register(
        ServiceBuilder::new("ALAudioPlayer")
            .method_async("playFile", "(s)", "v", play(ctx(), false, false))
            .method_async("playFile", "(sff)", "v", play(ctx(), false, false))
            .method_async("playFileInLoop", "(s)", "v", play(ctx(), true, false))
            .method_async("playFileInLoop", "(sff)", "v", play(ctx(), true, false))
            .method_async("playFileFromPosition", "(sf)", "v", play(ctx(), false, true))
            .method_async("playFileFromPosition", "(sfff)", "v", play(ctx(), false, true))
            .method("pause", "(i)", "v", |_| VOID),
    );
    node.register(
        ServiceBuilder::new("ALAudioDevice")
            .method("setOutputVolume", "(i)", "v", |_| VOID)
            .method("getOutputVolume", "()", "i", |_| Ok(Value::Int(70)))
            .method("muteAudioOut", "(b)", "v", |_| VOID),
    );
    node.register(
        ServiceBuilder::new("ALSystem")
            .method("robotName", "()", "s", |_| Ok("Choren virtual NAO".into()))
            .method("systemVersion", "()", "s", |_| Ok(crate::conformance::NAOQI_VERSION.into())),
    );
    // LogManager listeners, as Choregraphe's log viewer uses them: getListener(), clearFilters(),
    // setCategory("*", level), then the onLogMessage signal.
    // shortcut: filters are accepted but not applied; every robot log line reaches every listener.
    let listener = |d: Arc<Robotd>| {
        move |_: Vec<Value>| {
            let o = choren_qi::Object::local(
                ServiceBuilder::new("LogListener")
                    .method("setCategory", "(si)", "v", |_| Ok(Value::Void))
                    .method("addFilter", "(si)", "v", |_| Ok(Value::Void))
                    .method("clearFilters", "()", "v", |_| Ok(Value::Void))
                    .method("setLevel", "(i)", "v", |_| Ok(Value::Void))
                    .signal("onLogMessage", "((sisssILL)<LogMessage,source,level,category,location,message,id,date,systemDate>)"),
                false,
            );
            if let choren_qi::Object::Local(l) = &o {
                d.log_listeners.lock().unwrap().push(Arc::downgrade(l));
            }
            Ok(Value::Object(o))
        }
    };
    node.register(ServiceBuilder::new("LogManager").method("getListener", "()", "o", listener(ctx())).method("createListener", "()", "o", listener(ctx())));
    // ALResourceManager: what behavior boxes hold (`resources.rs`); the rest replays NAOqi.
    node.register(ServiceBuilder::new("ALResourceManager").method("areResourcesFree", "([s])", "b", {
        let d = ctx();
        move |a| {
            let leaves = names(&a[0]).iter().flat_map(|n| d.sim.robot.resource_leaves(n)).collect();
            Ok(Value::Bool(d.runtime.resources.is_free(&leaves, None)))
        }
    }));
    // Choregraphe's virtual robot starts with autonomous life "solitary" (it then idles).
    let life = Arc::new(std::sync::Mutex::new("solitary".to_string()));
    let (l1, l2) = (life.clone(), life.clone());
    node.register(
        ServiceBuilder::new("ALAutonomousLife")
            .method("getState", "()", "s", move |_| Ok(l1.lock().unwrap().as_str().into()))
            .method("setState", "(s)", "v", move |a| {
                *l2.lock().unwrap() = s(&a, 0)?;
                VOID
            })
            .method("switchFocus", "(s)", "v", |_| VOID)
            .method("setAutonomousAbilityEnabled", "(sb)", "v", |_| VOID),
    );
    node.register(
        ServiceBuilder::new("ALBasicAwareness")
            .method("startAwareness", "()", "v", |_| VOID)
            .method("stopAwareness", "()", "v", |_| VOID)
            .method("isAwarenessRunning", "()", "b", |_| Ok(Value::Bool(false)))
            .method("setEngagementMode", "(s)", "v", |_| VOID)
            .method("setTrackingMode", "(s)", "v", |_| VOID),
    );

    // Speech input on the virtual robot is typed: `ChorenSim.hear` and `ALDialog.forceInput`.
    let vocab = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let v1 = vocab.clone();
    node.register(
        ServiceBuilder::new("ALSpeechRecognition")
            .method("setVocabulary", "([s]b)", "v", move |a| {
                *v1.lock().unwrap() = names(&a[0]);
                VOID
            })
            .method("setLanguage", "(s)", "v", |_| VOID)
            .method("getLanguage", "()", "s", |_| Ok("English".into()))
            .method("subscribe", "(s)", "v", |_| VOID)
            .method("unsubscribe", "(s)", "v", |_| VOID)
            .method("pause", "(b)", "v", |_| VOID)
            .method("setParameter", "(sf)", "v", |_| VOID)
            .method("setAudioExpression", "(b)", "v", |_| VOID)
            .method("setVisualExpression", "(b)", "v", |_| VOID),
    );
    crate::dialog::register(d);

    crate::runtime::register(d);

    // Choren's own service: what the editor needs to draw and drive the virtual robot.
    node.register(
        ServiceBuilder::new("ChorenSim")
            .method("getState", "()", "m", {
                let d = ctx();
                move |_| {
                    let leds = d.led_colors();
                    let st = d.sim.state.lock().unwrap();
                    Ok(Value::Map(vec![
                        ("angles".into(), Value::floats(st.angles.iter().copied())),
                        ("motionSpeed".into(), Value::Double(st.motion_speed)),
                        ("torsoPitch".into(), Value::Float(st.torso_pitch as f32)),
                        ("pose".into(), Value::floats([st.pose.0, st.pose.1, st.pose.2])),
                        ("stiffness".into(), Value::floats(st.stiffness.iter().copied())),
                        ("leds".into(), Value::Map(leds.iter().map(|(k, v)| (k.as_str().into(), Value::Int(*v as i64))).collect())),
                    ]))
                }
            })
            .method("setMotionSpeed", "(f)", "v", {
                let d = ctx();
                move |a| d.sim.set_motion_speed(f(&a, 0)?).map(|_| Value::Void)
            })
            .method("robot", "()", "s", {
                let d = ctx();
                move |_| Ok(d.sim.robot.id.as_str().into())
            })
            .method("press", "(s)", "v", {
                let d = ctx();
                move |a| d.press(&s(&a, 0)?).map(|_| Value::Void)
            })
            .method("hear", "(s)", "v", {
                let d = ctx();
                move |a| {
                    let text = s(&a, 0)?;
                    let known = vocab.lock().unwrap().iter().any(|w| w.eq_ignore_ascii_case(&text));
                    d.hear(&text);
                    tokio::spawn(crate::dialog::input(d.clone(), text.clone()));
                    if !known {
                        d.log("info", "ALSpeechRecognition", &format!("Heard \"{text}\" (not in the current vocabulary)"));
                    }
                    VOID
                }
            })
            .method("setAngles", "([s][f])", "v", {
                let d = ctx();
                move |a| {
                    let joints = d.sim.robot.expand(&names(&a[0]))?;
                    let values = broadcast(floats(&a[1]), joints.len())?;
                    // Direct placement (joint sliders, teach mode): no speed limit, no stiffness.
                    let mut st = d.sim.state.lock().unwrap();
                    for (j, v) in joints.iter().zip(values) {
                        st.angles[*j] = d.sim.robot.clamp(*j, v);
                    }
                    VOID
                }
            })
            .method_async("activateInput", "(s[[i]]im)", "v", {
                let d = ctx();
                move |a| {
                    let d = d.clone();
                    async move {
                        let name = s(&a, 0)?;
                        let path = a[1].items().iter().map(|step| {
                            let fields = step.items();
                            if fields.len() != 3 { return Err("Expected [layer, keyframe, box ID]".to_string()); }
                            let number = |i: usize| fields[i].as_i64().ok_or_else(|| "Invalid box address".to_string());
                            Ok([number(0)?, number(1)?, number(2)?])
                        }).collect::<Result<Vec<_>, String>>()?;
                        let input = a[2].as_i64().ok_or("Input ID must be an integer")?;
                        d.runtime.activate_input(&name, &path, input, a[3].plain().clone()).await?;
                        VOID
                    }
                }
            })
            .signal("spoken", "(s)")
            .signal("log", "(sss)")
            .signal("behavior", "(ss)")
            .signal("stimulated", "(s[I]s)"),
    );
}

fn broadcast(v: Vec<f64>, n: usize) -> Result<Vec<f64>, String> {
    match v.len() {
        1 => Ok(vec![v[0]; n]),
        m if m == n => Ok(v),
        m => Err(format!("expected {n} values, got {m}")),
    }
}

/// Removes `^start(...)`-style animation tags, `\\pau=…\\` markup and the `$key=value` memory
/// annotations ALDialog adds (`$Dialog/Tag=label`) from spoken text.
pub fn strip_annotations(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            '\\' => {
                for c in chars.by_ref() {
                    if c == '\\' {
                        break;
                    }
                }
            }
            c => out.push(c),
        }
    }
    out.split_whitespace().filter(|w| !(w.starts_with('$') && w.contains('='))).collect::<Vec<_>>().join(" ")
}

/// move / moveToward / setWalkTargetVelocity: a body velocity, absolute (m/s, rad/s) or
/// normalised to the walk limits.
fn velocity(sim: Arc<crate::sim::Sim>, normalised: bool) -> impl Fn(Vec<Value>) -> Result<Value, String> + Send + Sync + 'static {
    move |a| {
        let w = &sim.robot.walk;
        let (x, y, t) = (f(&a, 0)?, f(&a, 1)?, f(&a, 2)?);
        let v = if normalised { (x.clamp(-1.0, 1.0) * w.max_x, y.clamp(-1.0, 1.0) * w.max_y, t.clamp(-1.0, 1.0) * w.max_theta) } else { (x, y, t) };
        sim.state.lock().unwrap().moving = if v == (0.0, 0.0, 0.0) { None } else { Some(MoveCmd { velocity: v, target: None, done: None }) };
        VOID
    }
}

/// moveTo / walkTo: (x, y, theta[, time][, config]) or a list of such relative poses, walked in turn.
fn move_to(sim: Arc<crate::sim::Sim>) -> impl Fn(Vec<Value>) -> crate::services::BoxFut + Send + Sync + 'static {
    move |a| {
        let sim = sim.clone();
        Box::pin(async move {
            let targets: Vec<(f64, f64, f64)> = if a[0].as_f64().is_some() {
                vec![(f(&a, 0)?, f(&a, 1)?, f(&a, 2)?)]
            } else {
                let items = a[0].plain().items();
                let poses: Vec<Value> = if items.first().is_some_and(|i| i.as_f64().is_some()) { vec![a[0].plain().clone()] } else { items };
                poses.iter().map(|p| { let v = floats(p); (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)) }).collect()
            };
            for target in targets {
                let (tx, rx) = tokio::sync::oneshot::channel();
                sim.state.lock().unwrap().moving = Some(MoveCmd { velocity: (0.0, 0.0, 0.0), target: Some(target), done: Some(tx) });
                if !rx.await.unwrap_or(false) {
                    return Ok(Value::Bool(false));
                }
            }
            Ok(Value::Bool(true))
        })
    }
}

/// openHand / closeHand: returns once the hand is open (1) or closed (0).
fn move_hand(sim: Arc<crate::sim::Sim>, target: f64) -> impl Fn(Vec<Value>) -> crate::services::BoxFut + Send + Sync + 'static {
    move |a| {
        let sim = sim.clone();
        Box::pin(async move {
            for j in sim.robot.expand(&[s(&a, 0)?])? {
                let _ = sim.move_hand(j, target).1.await;
            }
            VOID
        })
    }
}

fn stop_moving(sim: Arc<crate::sim::Sim>, ret: Value) -> impl Fn(Vec<Value>) -> Result<Value, String> + Send + Sync + 'static {
    move |_| {
        sim.state.lock().unwrap().moving = None;
        Ok(ret.clone())
    }
}

fn wait_moving(sim: Arc<crate::sim::Sim>) -> impl Fn(Vec<Value>) -> crate::services::BoxFut + Send + Sync + 'static {
    move |_| {
        let sim = sim.clone();
        Box::pin(async move {
            while sim.state.lock().unwrap().moving.as_ref().is_some_and(|m| m.target.is_some()) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            VOID
        })
    }
}

pub type BoxFut = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>>;

fn wav_seconds(path: &str) -> Option<f64> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" {
        return None;
    }
    let byte_rate = u32::from_le_bytes(b[28..32].try_into().ok()?) as f64;
    (byte_rate > 0.0).then(|| (b.len() - 44) as f64 / byte_rate)
}

#[cfg(test)]
mod tests {
    #[test]
    fn annotations_are_not_spoken() {
        assert_eq!(super::strip_annotations("^start(animations/Stand/Gestures/Hey_1) Hello \\pau=200\\ there"), "Hello there");
        assert_eq!(super::strip_annotations("$Dialog/Tag=ask question one \\last\\?"), "question one ?");
    }
}
