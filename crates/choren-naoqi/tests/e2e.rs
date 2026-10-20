//! End to end over the libqi protocol: editor-made project → install → run on the virtual NAO 6.
use choren_core::edit::{self, Command, History, NewBoxKind};
use choren_core::project::Project;
use choren_core::view;
use choren_qi::{Client, Value};
use std::time::Duration;

async fn robot() -> (choren_naoqi::VirtualRobot, Client) {
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    (vr, c)
}

#[tokio::test(flavor = "multi_thread")]
async fn motion_postures_and_memory_over_qi() {
    let (_vr, c) = robot().await;
    let services = c.services().await.unwrap();
    for s in ["ALMemory", "ALMotion", "ALRobotPosture", "ALTextToSpeech", "ALLeds", "ALBehaviorManager", "PackageManager", "ChorenSim"] {
        assert!(services.contains(&s.to_string()), "{s}");
    }
    assert_eq!(c.call("ALRobotPosture", "getPosture", vec![]).await.unwrap().as_str(), Some("Stand"), "starts where Choregraphe's virtual robot starts");
    c.call("ALMotion", "angleInterpolation", vec!["HeadYaw".into(), Value::Float(0.5), Value::Float(0.3), true.into()]).await.unwrap();
    let yaw = c.call("ALMotion", "getAngles", vec!["HeadYaw".into(), true.into()]).await.unwrap();
    assert!((yaw.items()[0].as_f64().unwrap() - 0.5).abs() < 0.02, "{yaw:?}");
    assert_eq!(c.call("ALRobotPosture", "goToPosture", vec!["Sit".into(), Value::Float(1.0)]).await.unwrap(), Value::Bool(true));
    assert_eq!(c.call("ALRobotPosture", "getPosture", vec![]).await.unwrap().as_str(), Some("Sit"));
    c.call("ALMemory", "insertData", vec!["Choren/Test".into(), Value::Int(7)]).await.unwrap();
    assert_eq!(c.call("ALMemory", "getData", vec!["Choren/Test".into()]).await.unwrap().as_i64(), Some(7));
    // overloads: fadeRGB by colour name and by integer
    c.call("ALLeds", "fadeRGB", vec!["FaceLeds".into(), "red".into(), Value::Float(0.0)]).await.unwrap();
    c.call("ALLeds", "fadeRGB", vec!["ChestLeds".into(), Value::Int(0x00ff00), Value::Float(0.0)]).await.unwrap();
    let state = c.call("ChorenSim", "getState", vec![]).await.unwrap();
    let Value::Map(fields) = state.plain() else { panic!() };
    let leds = fields.iter().find(|(k, _)| k.as_str() == Some("leds")).unwrap().1.clone();
    let Value::Map(leds) = leds.plain() else { panic!() };
    assert!(leds.iter().any(|(k, v)| k.as_str() == Some("LeftFaceLeds") && v.as_i64() == Some(0xff0000)));
}

async fn leds(c: &Client) -> std::collections::BTreeMap<String, i64> {
    let state = c.call("ChorenSim", "getState", vec![]).await.unwrap();
    let Value::Map(fields) = state.plain() else { panic!() };
    let leds = fields.iter().find(|(k, _)| k.as_str() == Some("leds")).unwrap().1.clone();
    let Value::Map(leds) = leds.plain() else { panic!() };
    leds.iter().map(|(k, v)| (k.as_str().unwrap().to_string(), v.as_i64().unwrap())).collect()
}

/// ALLeds names as NAOqi 2.8 lists them: eye segments, the both-eyes `FaceLed0`–`7`, ear and
/// brain LEDs, colour-channel groups and DCM actuators.
#[tokio::test(flavor = "multi_thread")]
async fn single_leds_and_colour_channels() {
    let (_vr, c) = robot().await;
    let fade = |name: &str, level: f32| vec![name.into(), Value::Float(level), Value::Float(0.0)];
    c.call("ALLeds", "fade", fade("LeftFaceLed3", 0.0)).await.unwrap();
    let l = leds(&c).await;
    assert_eq!(l["LeftFaceLed3"], 0);
    assert_eq!(l["LeftFaceLed4"], 0xffffff);
    assert_eq!(l["LeftFaceLeds"], 0xdfdfdf, "a group shows the mean of its LEDs");
    c.call("ALLeds", "fade", fade("LeftFootLeds", 0.0)).await.unwrap();
    c.call("ALLeds", "fade", fade("LeftFootLedsRed", 1.0)).await.unwrap();
    c.call("ALLeds", "fadeRGB", vec!["Brain0".into(), Value::Int(0xffffff), Value::Float(0.0)]).await.unwrap();
    c.call("ALLeds", "fade", fade("RightEarLed10", 0.5)).await.unwrap();
    c.call("ALLeds", "fadeRGB", vec!["FaceLed0".into(), Value::Int(0), Value::Float(0.0)]).await.unwrap();
    let l = leds(&c).await;
    assert_eq!(l["LeftFootLeds"], 0xff0000);
    assert_eq!(l["Brain0"], 0x0000ff, "brain LEDs are single-colour");
    assert_eq!(l["RightEarLed10"], 0x000080);
    let intensity = |n: &str| { let c = &c; let n = n.to_string(); async move { c.call("ALLeds", "getIntensity", vec![n.as_str().into()]).await } };
    // FaceLed0 is the left eye's 0° segment and the right eye's 315° one.
    assert_eq!(intensity("Face/Led/Red/Left/0Deg/Actuator/Value").await.unwrap().as_f64(), Some(0.0));
    assert_eq!(intensity("Face/Led/Green/Right/315Deg").await.unwrap().as_f64(), Some(0.0));
    assert_eq!(intensity("RightFaceLed1").await.unwrap().as_f64(), Some(1.0));
    assert!(intensity("ChestLeds").await.unwrap_err().contains("xGetGroupIntensity"));
    let err = c.call("ALLeds", "fade", fade("LeftFaceLed9", 1.0)).await.unwrap_err();
    assert_eq!(err, "ALLeds: fade: Could not parse 'LeftFaceLed9 'as a LED Device, RGB-LED or Group", "NAOqi 2.8's wording for fade");
    let err = c.call("ALLeds", "fadeRGB", vec!["FaceLeds".into(), "purple".into(), Value::Float(0.0)]).await.unwrap_err();
    assert!(err.contains("Supported colors are: white, red, green, blue, yellow, magenta, cyan"), "{err}");
    assert_eq!(c.call("ALLeds", "listGroups", vec![]).await.unwrap().items().len(), 80);
    assert_eq!(c.call("ALLeds", "listLEDs", vec![]).await.unwrap().items().len(), 57);
}

/// Animated Say (box library): `pCall("say", …)` returns a task id, `wait(id)` blocks on it, and
/// `stop(id)` from another proxy ends a waited task early.
#[tokio::test(flavor = "multi_thread")]
async fn pcall_wait_and_stop_on_qi_services() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not available, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-pcall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "pcall").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let b = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Animated Say".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let script = r#"import threading, time
class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self, False)
    def onLoad(self):
        self.tts = self.session().service("ALAnimatedSpeech")
        self.ttsStop = self.session().service("ALAnimatedSpeech")
        self.memory = self.session().service("ALMemory")
    def onInput_onStart(self):
        id = self.tts.pCall("say", "Hello", {"speakingMovementMode": "contextual"})
        self.memory.insertData("Say/id", id)
        self.tts.wait(id)
        self.memory.insertData("Say/done", 1)
        long = self.tts.pCall("say", " ".join(["word"] * 200), {"speakingMovementMode": "random"})
        self.memory.insertData("Say/running", self.tts.isRunning(long))
        threading.Timer(0.3, lambda: self.ttsStop.stop(long)).start()
        start = time.time()
        self.tts.wait(long)
        self.memory.insertData("Say/stoppedAfter", time.time() - start)
        self.memory.insertData("Say/runningAfterStop", self.tts.isRunning(long))
        self.onStopped()
"#;
    let id = p.behaviors[0].doc.attr_num(b, "id", 0.0) as i64;
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: b, text: script.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: id, in_port: 2 },
        Command::AddLink { diagram, out_owner: id, out_port: 4, in_owner: 0, in_port: 4 },
    ])
    .unwrap();
    p.save().unwrap();
    let (_vr, c) = robot().await;
    c.call("PackageManager", "install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = c.call("ALBehaviorManager", "getInstalledBehaviors", vec![]).await.unwrap().items()[0].as_str().unwrap().to_string();
    tokio::time::timeout(Duration::from_secs(20), c.call("ALBehaviorManager", "runBehavior", vec![name.as_str().into()])).await.expect("behavior finished").unwrap();
    let get = |k: &'static str| { let c = &c; async move { c.call("ALMemory", "getData", vec![k.into()]).await.unwrap() } };
    assert!(get("Say/id").await.as_i64().is_some());
    assert_eq!(get("Say/done").await.as_i64(), Some(1));
    assert_eq!(get("Say/running").await.plain(), &Value::Bool(true));
    assert!(get("Say/stoppedAfter").await.as_f64().unwrap() < 5.0, "stop ended the waited say early");
    assert_eq!(get("Say/runningAfterStop").await.plain(), &Value::Bool(false));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_behavior_made_in_the_editor_runs_on_the_virtual_robot() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not available, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "e2e").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let r = view::root_box(d);
    let t = d.kid(r, "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let created = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Hello".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let script = r#"class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self)

    def onLoad(self):
        self.memory = self.session().service("ALMemory")

    def onInput_onStart(self):
        print "py2 print still works"
        self.session().service("ALTextToSpeech").say("Hello from Choren")
        self.memory.insertData("Choren/Done", self.getParameter("Greeting"))
        self.onStopped()
"#;
    let box_id = p.behaviors[0].doc.attr_num(created, "id", 0.0) as i64;
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: created, text: script.into() },
        Command::AddPort { r#box: created, kind: edit::PortKind::Parameter, name: "Greeting".into(), values: [("value".to_string(), "hi".to_string()), ("content_type".to_string(), "3".to_string())].into() },
        // root onStart (id 2) → box onStart (id 2); box onStopped (id 4) → root onStopped (id 4)
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: box_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: box_id, out_port: 4, in_owner: 0, in_port: 4 },
    ]).unwrap();
    p.save().unwrap();

    let (_vr, c) = robot().await;
    let mut spoken = c.subscribe("ChorenSim", "spoken").await.unwrap();
    let mut log = c.subscribe("ChorenSim", "log").await.unwrap();
    c.call("PackageManager", "install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let installed = c.call("ALBehaviorManager", "getInstalledBehaviors", vec![]).await.unwrap();
    let name = installed.items()[0].as_str().unwrap().to_string();
    assert!(name.ends_with("/behavior_1"), "{name}");
    tokio::time::timeout(Duration::from_secs(20), c.call("ALBehaviorManager", "runBehavior", vec![name.as_str().into()])).await.expect("behavior finished").unwrap();
    assert_eq!(spoken.recv().await.unwrap().items()[0].as_str(), Some("Hello from Choren"));
    assert_eq!(c.call("ALMemory", "getData", vec!["Choren/Done".into()]).await.unwrap().as_str(), Some("hi"));
    assert!(c.call("ALBehaviorManager", "getRunningBehaviors", vec![]).await.unwrap().items().is_empty());
    let mut lines = vec![];
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_millis(200), log.recv()).await {
        lines.push(format!("{:?}", v));
    }
    assert!(lines.iter().any(|l| l.contains("py2 print still works")), "{lines:#?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_behavior_cancels_its_timeline_motion_only() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "stop").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let tl = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Timeline, name: "Slow".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let tl_id = p.behaviors[0].doc.attr_num(tl, "id", 0.0) as i64;
    let timeline = p.behaviors[0].doc.kid(tl, "Timeline").unwrap();
    edit::apply(&mut p, &mut h, &bid, "motion", vec![
        Command::InsertXml { parent: timeline, xml: r#"<ActuatorList model=""><ActuatorCurve name="value" actuator="HeadYaw" recordable="1" mute="0" unit="0"><Key frame="1" value="0" /><Key frame="250" value="60" /></ActuatorCurve></ActuatorList>"#.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: tl_id, in_port: 2 },
    ]).unwrap();
    p.save().unwrap();

    let (_vr, c) = robot().await;
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    // Another client's motion that must survive the Stop.
    let other = {
        let addr = _vr.addr.to_string();
        tokio::spawn(async move {
            let c2 = Client::connect(&addr).await.unwrap();
            c2.call("ALMotion", "angleInterpolation", vec!["LShoulderPitch".into(), Value::Float(0.5), Value::Float(3.0), true.into()]).await.unwrap();
        })
    };
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let tasks = |v: Value| v.items().len();
    assert_eq!(tasks(c.call("ALMotion", "getTaskList", vec![]).await.unwrap()), 2, "timeline and other motion run");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    for _ in 0..50 {
        if c.call("ALBehaviorManager", "getRunningBehaviors", vec![]).await.unwrap().items().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(tasks(c.call("ALMotion", "getTaskList", vec![]).await.unwrap()), 1, "only the other client's motion is left");
    let yaw = c.call("ALMotion", "getAngles", vec!["HeadYaw".into(), true.into()]).await.unwrap().items()[0].as_f64().unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let later = c.call("ALMotion", "getAngles", vec!["HeadYaw".into(), true.into()]).await.unwrap().items()[0].as_f64().unwrap();
    assert!((later - yaw).abs() < 1e-6, "HeadYaw stopped moving ({yaw} → {later})");
    other.abort();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_virtual_robot_frees_its_port() {
    let (vr, c) = robot().await;
    let addr = vr.addr;
    drop(vr);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(c.call("ALMotion", "getAngles", vec!["HeadYaw".into(), true.into()]).await.is_err(), "connections are closed");
    let again = choren_naoqi::start("nao6", &addr.to_string(), None).await.unwrap();
    assert_eq!(again.addr, addr, "the same address can be bound again");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_lets_on_unload_finish_its_robot_calls() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-unload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "unload").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let walker = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Walk".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let id = p.behaviors[0].doc.attr_num(walker, "id", 0.0) as i64;
    let script = "class MyClass(GeneratedClass):\n    def __init__(self):\n        GeneratedClass.__init__(self)\n\n    def onUnload(self):\n        import time\n        time.sleep(0.3)\n        self.session().service(\"ALMotion\").stopMove()\n\n    def onInput_onStart(self):\n        self.session().service(\"ALMotion\").move(0.1, 0, 0)\n";
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: walker, text: script.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: id, in_port: 2 },
    ]).unwrap();
    p.save().unwrap();
    let (_vr, c) = robot().await;
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    let active = || async { c.call("ALMotion", "moveIsActive", vec![]).await.unwrap() == Value::Bool(true) };
    for _ in 0..100 {
        if active().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(active().await, "the box started walking");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    for _ in 0..100 {
        if c.call("ALBehaviorManager", "getRunningBehaviors", vec![]).await.unwrap().items().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!active().await, "onUnload's stopMove ran before the host exited");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A script that outlives its keyframe (an LED fade longer than the timeline, as in the
/// Choregraphe animation library) must not fire the timeline box's onStopped a second time.
#[tokio::test(flavor = "multi_thread")]
async fn outputs_of_unloaded_boxes_are_not_routed() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-late-output-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = Project::create(&dir, "late").unwrap();
    std::fs::write(dir.join(&p.behaviors[0].source), r#"
<ChoregrapheProject><Box id="-1" name="Root"><Input id="2" name="onStart" type="1" nature="2"/>
<Timeline enable="0"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram>
<Box id="10" name="Short timeline"><Input id="1" name="onLoad" type="1" nature="0" inner="1"/><Input id="2" name="onStart" type="1" nature="2"/><Output id="4" name="onStopped" type="1" nature="1"/>
<Timeline enable="1" fps="25" size="5"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram>
<Box id="20" name="Slow fade"><Input id="2" name="onSet" type="1" nature="2"/><Output id="3" name="onReady" type="1" nature="1"/>
<script><content><![CDATA[class MyClass(GeneratedClass):
    def onInput_onSet(self):
        import time
        time.sleep(0.6)
        self.onReady()
]]></content></script></Box>
<Link outputowner="0" indexofoutput="1" inputowner="20" indexofinput="2"/>
<Link outputowner="20" indexofoutput="3" inputowner="0" indexofinput="4"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box>
<Box id="11" name="Counter"><Input id="2" name="onStart" type="1" nature="2"/><script><content><![CDATA[class MyClass(GeneratedClass):
    def onLoad(self):
        self.count = 0
    def onInput_onStart(self):
        self.count += 1
        self.session().service("ALMemory").insertData("Late/Stopped", self.count)
]]></content></script></Box>
<Link outputowner="0" indexofoutput="2" inputowner="10" indexofinput="2"/>
<Link outputowner="10" indexofoutput="4" inputowner="11" indexofinput="2"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>
"#).unwrap();
    let (_vr, c) = robot().await;
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    let stopped = || async { c.call("ALMemory", "getData", vec!["Late/Stopped".into()]).await.ok().and_then(|v| v.as_i64()) };
    for _ in 0..100 {
        if stopped().await.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(stopped().await, Some(1), "the timeline ended");
    // The fade finishes 0.6 s after the keyframe was unloaded; its onReady goes nowhere.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(stopped().await, Some(1), "a late output of an unloaded box was routed");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn editor_activates_running_inputs_including_unloaded_nested_boxes() {
    let dir = std::env::temp_dir().join(format!("choren-e2e-input-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "inputs").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let b = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Probe".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let script = r#"class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self)
    def onInput_onStart(self):
        self.session().service("ALMemory").insertData("Input/Bang", "activated")
    def onInput_text(self, value):
        self.session().service("ALMemory").insertData("Input/Text", value)
    def onInput_number(self, value):
        self.session().service("ALMemory").insertData("Input/Number", value)
    def onInput_dynamic(self, value):
        self.session().service("ALMemory").insertData("Input/Dynamic", value)
"#;
    let mut commands = vec![Command::SetScript { r#box: b, text: script.into() }];
    for (name, typ) in [("text", "3"), ("number", "2"), ("dynamic", "0")] {
        commands.push(Command::AddPort { r#box: b, kind: edit::PortKind::Input, name: name.into(), values: [("type".into(), typ.into()), ("nature".into(), "1".into())].into() });
    }
    edit::apply(&mut p, &mut h, &bid, "inputs", commands).unwrap();
    let container = edit::apply(&mut p, &mut h, &bid, "nested diagram", vec![Command::NewBox { diagram, kind: NewBoxKind::Diagram, name: "Container".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let d = &p.behaviors[0].doc;
    let t = d.kid(container, "Timeline").unwrap();
    let inner = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let nested = edit::apply(&mut p, &mut h, &bid, "nested box", vec![Command::NewBox { diagram: inner, kind: NewBoxKind::Python, name: "Nested".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    edit::apply(&mut p, &mut h, &bid, "nested script", vec![Command::SetScript { r#box: nested, text: script.replace("Input/Bang", "Input/Nested") }]).unwrap();
    let d = &p.behaviors[0].doc;
    let container_id = d.attr_num(container, "id", 0.0) as i64;
    let nested_id = d.attr_num(nested, "id", 0.0) as i64;
    let box_id = d.attr_num(b, "id", 0.0) as i64;
    let port_id = |name| d.attr_num(d.kids(b, "Input").find(|&n| d.attr_raw(n, "name") == Some(name)).unwrap(), "id", 0.0) as i64;
    let text_id = port_id("text");
    let number_id = port_id("number");
    let dynamic_id = port_id("dynamic");
    p.save().unwrap();
    let (_vr, c) = robot().await;
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    let path = Value::List(vec![Value::List(vec![0.into(), 0.into(), Value::Int(box_id)])]);
    let args = |id, v| vec![name.as_str().into(), path.clone(), Value::Int(id), v];
    assert!(c.call("ChorenSim", "activateInput", args(2, Value::Void)).await.unwrap_err().contains("not running"));
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    assert!(c.call("ChorenSim", "activateInput", args(text_id, Value::Int(2))).await.unwrap_err().contains("input type"));
    assert!(c.call("ChorenSim", "activateInput", args(999, Value::Void)).await.unwrap_err().contains("Input not found"));
    let mut invalid = args(2, Value::Void);
    invalid[1] = Value::List(vec![Value::List(vec![99.into(), 0.into(), Value::Int(box_id)])]);
    assert!(c.call("ChorenSim", "activateInput", invalid).await.unwrap_err().contains("diagram"));
    for (id, value, key, expected) in [
        (2, Value::Void, "Input/Bang", Value::from("activated")),
        (text_id, Value::from("hello"), "Input/Text", Value::from("hello")),
        (number_id, Value::Float(12.5), "Input/Number", Value::Float(12.5)),
        (dynamic_id, Value::List(vec![Value::Int(7), Value::from("hi")]), "Input/Dynamic", Value::List(vec![Value::Int(7), Value::from("hi")])),
    ] {
        c.call("ChorenSim", "activateInput", args(id, value)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if c.call("ALMemory", "getData", vec![key.into()]).await.is_ok_and(|v| choren_naoqi::runtime::value_to_json(&v) == choren_naoqi::runtime::value_to_json(&expected)) { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap_or_else(|_| panic!("{key}: input did not reach the Python box"));
    }
    let nested_path = Value::List(vec![Value::List(vec![0.into(), 0.into(), Value::Int(container_id)]), Value::List(vec![0.into(), 0.into(), Value::Int(nested_id)])]);
    let nested_args = vec![name.as_str().into(), nested_path, Value::Int(2), Value::Void];
    c.call("ChorenSim", "activateInput", nested_args).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if c.call("ALMemory", "getData", vec!["Input/Nested".into()]).await.is_ok_and(|v| v.as_str() == Some("activated")) { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("nested input loads and starts its Python box without starting the parent");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    assert!(c.call("ChorenSim", "activateInput", args(2, Value::Void)).await.unwrap_err().contains("not running"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Play as the editor does it on any robot: the unsaved project as a `.pkg`, sent as a `qi::File`
/// to `PackageManager._install(oss)` (Choregraphe's PackageInstaller), then started by name.
#[tokio::test(flavor = "multi_thread")]
async fn play_uploads_the_project_as_a_package() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-pkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "pkg").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let created = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Mark".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let box_id = p.behaviors[0].doc.attr_num(created, "id", 0.0) as i64;
    let script = "class MyClass(GeneratedClass):\n    def onInput_onStart(self):\n        self.session().service(\"ALMemory\").insertData(\"Choren/Pkg\", 1)\n        self.onStopped()\n";
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: created, text: script.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: box_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: box_id, out_port: 4, in_owner: 0, in_port: 4 },
    ]).unwrap();
    let uuid = p.manifest_fields().unwrap().uuid;
    let pkg = p.package().unwrap();

    let (_vr, c) = robot().await;
    let file = Value::Object(choren_qi::object::file(pkg));
    assert_eq!(c.call("PackageManager", "_install", vec![file, "choren".into(), "".into()]).await.unwrap(), Value::Bool(true));
    let name = format!("{uuid}/behavior_1");
    assert_eq!(c.call("ALBehaviorManager", "isBehaviorInstalled", vec![name.as_str().into()]).await.unwrap(), Value::Bool(true));
    tokio::time::timeout(Duration::from_secs(20), c.call("ALBehaviorManager", "runBehavior", vec![name.as_str().into()])).await.expect("behavior finished").unwrap();
    assert_eq!(c.call("ALMemory", "getData", vec!["Choren/Pkg".into()]).await.unwrap().as_i64(), Some(1));
    // The same package from a path, as `PackageManager.install(s)` takes it.
    let path = dir.join("pkg.pkg");
    std::fs::write(&path, p.package().unwrap()).unwrap();
    assert_eq!(c.call("PackageManager", "install", vec![path.to_string_lossy().as_ref().into()]).await.unwrap(), Value::Bool(true));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `getParentTimeline().goTo(name)`, `getSize()` and `pause()` from box scripts drive the
/// enclosing timeline.
#[tokio::test(flavor = "multi_thread")]
async fn scripts_control_their_parent_timeline() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-goto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "goto").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let tl = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Timeline, name: "Slow".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let tl_id = p.behaviors[0].doc.attr_num(tl, "id", 0.0) as i64;
    let timeline = p.behaviors[0].doc.kid(tl, "Timeline").unwrap();
    edit::apply(&mut p, &mut h, &bid, "layers", vec![
        Command::InsertXml { parent: timeline, xml: r#"<ActuatorList model=""><ActuatorCurve name="value" actuator="HeadYaw" recordable="1" mute="0" unit="0"><Key frame="1" value="0" /><Key frame="250" value="60" /></ActuatorCurve></ActuatorList>"#.into() },
        Command::InsertXml { parent: timeline, xml: r#"<BehaviorLayer name="behavior_layer1"><BehaviorKeyframe name="A" index="1"><Diagram /></BehaviorKeyframe><BehaviorKeyframe name="B" index="200"><Diagram /></BehaviorKeyframe></BehaviorLayer>"#.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: tl_id, in_port: 2 },
    ]).unwrap();
    let doc = &p.behaviors[0].doc;
    let layer = doc.kid(doc.kid(tl, "Timeline").unwrap(), "BehaviorLayer").unwrap();
    let kf: Vec<_> = doc.kids(layer, "BehaviorKeyframe").map(|k| doc.kid(k, "Diagram").unwrap()).collect();
    for (dg, name, body) in [
        (kf[0], "Jump", "self.getParentTimeline().goTo(\"B\")"),
        (kf[1], "Mark", "self.getParentTimeline().pause()\n        self.session().service(\"ALMemory\").insertData(\"Choren/Size\", self.getParentTimeline().getSize())"),
    ] {
        let b = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram: dg, kind: NewBoxKind::Python, name: name.into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
        let id = p.behaviors[0].doc.attr_num(b, "id", 0.0) as i64;
        let script = format!("class MyClass(GeneratedClass):\n    def onInput_onStart(self):\n        {body}\n");
        edit::apply(&mut p, &mut h, &bid, "wire", vec![
            Command::SetScript { r#box: b, text: script },
            // Keyframe diagrams start from their onLoad rail (id 1), as on NAOqi.
            Command::AddLink { diagram: dg, out_owner: 0, out_port: 1, in_owner: id, in_port: 2 },
        ]).unwrap();
    }
    let (_vr, c) = robot().await;
    let file = Value::Object(choren_qi::object::file(p.package().unwrap()));
    assert_eq!(c.call("PackageManager", "install", vec![file]).await.unwrap(), Value::Bool(true));
    let name = format!("{}/behavior_1", p.manifest_fields().unwrap().uuid);
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    // Frame 200 is 8 s away at 25 fps; the goTo reaches it at once.
    let mut size = None;
    for _ in 0..60 {
        if let Ok(v) = c.call("ALMemory", "getData", vec!["Choren/Size".into()]).await {
            size = v.as_i64();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(size, Some(249), "keyframe B ran and getSize is end − start");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(c.call("ALMotion", "getTaskList", vec![]).await.unwrap().items().is_empty(), "pause stops the timeline's motion");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The log path the editor uses on real robots (Choregraphe's log viewer): a listener object from
/// `LogManager.getListener()`, its filters, then its `onLogMessage` signal.
#[tokio::test(flavor = "multi_thread")]
async fn log_listener_objects_receive_robot_logs() {
    let (vr, c) = robot().await;
    let Value::Object(o) = c.call("LogManager", "getListener", vec![]).await.unwrap() else { panic!("object expected") };
    let listener = o.remote().unwrap();
    listener.call("clearFilters", vec![]).await.unwrap();
    listener.call("setCategory", vec!["*".into(), Value::Int(6)]).await.unwrap();
    let mut messages = listener.subscribe("onLogMessage").await.unwrap();
    vr.robotd.log("warning", "Choren.Test", "hello listeners");
    let m = tokio::time::timeout(Duration::from_secs(2), messages.recv()).await.unwrap().unwrap().items()[0].items();
    assert_eq!((m[1].as_i64(), m[2].as_str(), m[4].as_str()), (Some(3), Some("Choren.Test"), Some("hello listeners")));
}

/// The behavior debugger reports what NAOqi 2.8's does (keys recorded from Choregraphe's robot
/// with `examples/debugger_probe.rs`, 2026-10-19): `<id>:<box path>#<port>` for ports, and the
/// timeline's frame under its box path. Keyframe diagrams start from their onLoad rail only.
#[tokio::test(flavor = "multi_thread")]
async fn debugger_keys_match_naoqi() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-dbg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "dbg").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let mut new_box = |p: &mut Project, dg, kind, name: &str| {
        let b = edit::apply(p, &mut h, &bid, "box", vec![Command::NewBox { diagram: dg, kind, name: name.into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
        (b, p.behaviors[0].doc.attr_num(b, "id", 0.0) as i64)
    };
    let (hello, hello_id) = new_box(&mut p, diagram, NewBoxKind::Python, "Hello World");
    let (motion, motion_id) = new_box(&mut p, diagram, NewBoxKind::Timeline, "Motion");
    let timeline = p.behaviors[0].doc.kid(motion, "Timeline").unwrap();
    let mut h = History::fresh();
    edit::apply(&mut p, &mut h, &bid, "layers", vec![
        Command::InsertXml { parent: timeline, xml: r#"<ActuatorList model=""><ActuatorCurve name="value" actuator="HeadYaw" recordable="1" mute="0" unit="0"><Key frame="1" value="0" /><Key frame="3" value="5" /></ActuatorCurve></ActuatorList>"#.into() },
        Command::InsertXml { parent: timeline, xml: r#"<BehaviorLayer name="my layer"><BehaviorKeyframe name="key frame" index="1"><Diagram /></BehaviorKeyframe></BehaviorLayer>"#.into() },
    ]).unwrap();
    let doc = &p.behaviors[0].doc;
    let kf = doc.kid(doc.kid(doc.kid(timeline, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let tick = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram: kf, kind: NewBoxKind::Python, name: "Tick".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let tick_id = p.behaviors[0].doc.attr_num(tick, "id", 0.0) as i64;
    let script = "class MyClass(GeneratedClass):\n    def onInput_onStart(self):\n        self.onStopped(42)\n";
    edit::apply(&mut p, &mut h, &bid, "wire", vec![
        Command::SetScript { r#box: hello, text: script.into() },
        Command::SetScript { r#box: tick, text: script.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: hello_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: hello_id, out_port: 4, in_owner: motion_id, in_port: 2 },
        Command::AddLink { diagram: kf, out_owner: 0, out_port: 1, in_owner: tick_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: motion_id, out_port: 4, in_owner: 0, in_port: 4 },
    ]).unwrap();
    let (_vr, c) = robot().await;
    let file = Value::Object(choren_qi::object::file(p.package().unwrap()));
    c.call("PackageManager", "_install", vec![file, "choren".into(), "".into()]).await.unwrap();
    let name = format!("{}/behavior_1", p.manifest_fields().unwrap().uuid);
    let id = c.call("ALBehaviorManager", "_getBehaviorFrameManagerId", vec![name.as_str().into()]).await.unwrap();
    let id = id.as_str().unwrap().trim_end_matches('/').to_string();
    let Value::Object(o) = c.call("ALFrameManager", "getBehaviorDebuggerFor", vec![format!("{id}/").into()]).await.unwrap() else { panic!() };
    let mut events = o.remote().unwrap().subscribe("signal").await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), c.call("ALBehaviorManager", "runBehavior", vec![name.as_str().into()])).await.unwrap().unwrap();
    let mut keys = vec![];
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_millis(300), events.recv()).await {
        let f = v.items();
        keys.push((f[0].as_str().unwrap().strip_prefix(&id).unwrap().to_string(), f[1].plain().as_i64()));
    }
    for (key, value) in [
        ("/#onLoad", None), ("/#onStart", None), ("/Hello World_1#onStopped", Some(42)), ("/Motion_2#onStart", Some(42)), ("/Motion_2", Some(1)),
        ("/Motion_2#mylayer/keyframe#onLoad", None), ("/Motion_2/my layer/key frame/Tick_1#onStopped", Some(42)), ("/Motion_2", Some(4)), ("/Motion_2#onStopped", None), ("/#onStopped", None),
        ("/Motion_2", Some(-1)), ("/", Some(1)),
    ] {
        assert!(keys.contains(&(key.to_string(), value)), "{key} {value:?} in {keys:#?}");
    }
    assert!(!keys.iter().any(|k| k.0.ends_with("Hello World_1#onStart")), "script boxes' inputs are not reported");
    // The editor places every key in the diagram (robot.rs `debug`).
    let index = choren_naoqi::runtime::debugger_index(&p.behaviors[0].doc);
    for (key, _) in &keys {
        assert!(index.contains_key(choren_naoqi::runtime::split_debugger_key(key).0), "{key}");
    }
    assert_eq!(index["/Motion_2/my layer/key frame/Tick_1"], [motion_id, tick_id]);
    // The editor relays a box's output by its debugger key to the linked boxes (robot.rs `debug`).
    let links = choren_naoqi::runtime::output_links(&p.behaviors[0].doc);
    assert_eq!(links["/Hello World_1#onStopped"], [("/Motion_2".to_string(), "onStart".to_string())]);
    assert_eq!(links.len(), 1, "links to and from diagram rails are NAOqi's own");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Choregraphe's manual stimulation on a robot: `ALFrameManager.callBoxInput(uri, "onInput_x__", v)`.
#[tokio::test(flavor = "multi_thread")]
async fn call_box_input_stimulates_a_box_by_its_debugger_path() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-e2e-call-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "call").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let manual = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Manual".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let script = "class MyClass(GeneratedClass):\n    def onInput_onStart(self, p):\n        self.onStopped(p if p else self.getParameter(\"Greeting\"))\n";
    edit::apply(&mut p, &mut h, &bid, "script", vec![
        Command::SetScript { r#box: manual, text: script.into() },
        Command::AddPort { r#box: manual, kind: edit::PortKind::Parameter, name: "Greeting".into(), values: [("value".to_string(), "hi".to_string()), ("content_type".to_string(), "3".to_string())].into() },
    ]).unwrap();
    // A box inside a diagram the flow never enters: updating its parameter must not load it.
    let later = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram, kind: NewBoxKind::Diagram, name: "Later".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let d = &p.behaviors[0].doc;
    let later_dg = d.kid(d.kid(d.kid(d.kid(later, "Timeline").unwrap(), "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let sleeper = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram: later_dg, kind: NewBoxKind::Python, name: "Sleeper".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    edit::apply(&mut p, &mut h, &bid, "sleeper", vec![
        Command::SetScript { r#box: sleeper, text: "class MyClass(GeneratedClass):\n    def onLoad(self):\n        self.session().service(\"ALMemory\").insertData(\"Choren/Loaded\", 1)\n".into() },
        Command::AddPort { r#box: sleeper, kind: edit::PortKind::Parameter, name: "Greeting".into(), values: [("value".to_string(), "x".to_string()), ("content_type".to_string(), "3".to_string())].into() },
    ]).unwrap();
    let (_vr, c) = robot().await;
    c.call("PackageManager", "install", vec![Value::Object(choren_qi::object::file(p.package().unwrap()))]).await.unwrap();
    let name = format!("{}/behavior_1", p.manifest_fields().unwrap().uuid);
    let id = c.call("ALBehaviorManager", "_getBehaviorFrameManagerId", vec![name.as_str().into()]).await.unwrap();
    let Value::Object(o) = c.call("ALFrameManager", "getBehaviorDebuggerFor", vec![id.clone()]).await.unwrap() else { panic!() };
    let mut events = o.remote().unwrap().subscribe("signal").await.unwrap();
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    let uri = format!("{}/Manual_1", id.as_str().unwrap().trim_end_matches('/'));
    c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "onInput_onStart__".into(), Value::Int(7)]).await.unwrap();
    let mut got = None;
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
        let f = v.items();
        if f[0].as_str() == Some(&format!("{uri}#onStopped")) {
            got = f[1].plain().as_i64();
            break;
        }
    }
    assert_eq!(got, Some(7));
    assert!(c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "onInput_nothing".into(), Value::Void]).await.unwrap_err().contains("Can't find method"));
    // "Auto-update parameters on robot": callBoxInput(uri, "setParameter", name, value).
    assert!(c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "setParameter".into(), "Missing".into(), Value::Int(1)]).await.unwrap_err().contains("does not exist"));
    c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "setParameter".into(), "Greeting".into(), "updated".into()]).await.unwrap();
    c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "onInput_onStart".into(), Value::Int(0)]).await.unwrap();
    let mut greeting = None;
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
        let f = v.items();
        if f[0].as_str() == Some(&format!("{uri}#onStopped")) {
            greeting = f[1].plain().as_str().map(String::from);
            break;
        }
    }
    assert_eq!(greeting.as_deref(), Some("updated"));
    let later_uri = format!("{}/Later_2/Sleeper_1", id.as_str().unwrap().trim_end_matches('/'));
    c.call("ALFrameManager", "callBoxInput", vec![later_uri.as_str().into(), "setParameter".into(), "Greeting".into(), "y".into()]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(c.call("ALMemory", "getData", vec!["Choren/Loaded".into()]).await.is_err(), "the box was not loaded");
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
