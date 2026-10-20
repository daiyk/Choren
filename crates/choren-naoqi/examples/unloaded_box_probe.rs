//! Stimulates a box whose diagram the behavior never entered, as the editor's start arrow does.
//! Called directly, its script misses what `onLoad` sets up (NAOqi logs the AttributeError and stops
//! the behavior); `ALFrameManager.getBox(uri).__onLoad__()` first, as `robot_activate_input` does,
//! makes it run. Run against a NAOqi robot
//! (Choregraphe's local `naoqi-bin` is enough; nothing moves).
//!
//! `cargo run -p choren-naoqi --example unloaded_box_probe -- tcp://127.0.0.1:19559`
use choren_core::edit::{self, Command, History, NewBoxKind};
use choren_core::project::Project;
use choren_core::view;
use choren_qi::{Client, Value};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), String> {
    let url = std::env::args().nth(1).unwrap_or_else(|| "tcp://127.0.0.1:19559".into());
    let dir = std::env::temp_dir().join(format!("choren-unloaded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut p = Project::create(&dir, "unloaded")?;
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    // "Later" is a diagram box nothing links to, like Gesture (Anger) v2 while another gesture plays.
    let later = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram, kind: NewBoxKind::Diagram, name: "Later".into(), x: 0.0, y: 0.0, dialog_file: None }])?.created[0];
    let later_id = p.behaviors[0].doc.attr_num(later, "id", 0.0) as i64;
    let d = &p.behaviors[0].doc;
    let inner = d.kid(later, "Timeline").and_then(|t| d.kid(t, "BehaviorLayer")).and_then(|l| d.kid(l, "BehaviorKeyframe")).and_then(|k| d.kid(k, "Diagram")).ok_or("no inner diagram")?;
    let say = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram: inner, kind: NewBoxKind::Python, name: "Say".into(), x: 0.0, y: 0.0, dialog_file: None }])?.created[0];
    let say_id = p.behaviors[0].doc.attr_num(say, "id", 0.0) as i64;
    // Like Animated Say: onLoad creates state that onInput_onStart needs.
    let script = "class MyClass(GeneratedClass):\n    def onLoad(self):\n        self.ids = []\n\n    def onInput_onStart(self):\n        self.ids.append(1)\n        self.onStopped(len(self.ids))\n";
    let next = edit::apply(&mut p, &mut h, &bid, "box", vec![Command::NewBox { diagram: inner, kind: NewBoxKind::Python, name: "Next".into(), x: 0.0, y: 0.0, dialog_file: None }])?.created[0];
    let next_id = p.behaviors[0].doc.attr_num(next, "id", 0.0) as i64;
    edit::apply(&mut p, &mut h, &bid, "script", vec![
        Command::SetScript { r#box: say, text: script.into() },
        Command::SetScript { r#box: next, text: script.replace("len(self.ids)", "10 + len(self.ids)") },
        // Say.onStopped → Next.onStart, which NAOqi does not connect while Later is not loaded.
        Command::AddLink { diagram: inner, out_owner: say_id, out_port: 4, in_owner: next_id, in_port: 2 },
    ])?;
    let links = choren_naoqi::runtime::output_links(&p.behaviors[0].doc);
    let name = format!("{}/behavior_1", p.manifest_fields().ok_or("no manifest")?.uuid);
    let c = Client::connect(&url).await?;
    let file = Value::Object(choren_qi::object::file(p.package()?));
    println!("install: {:?}", c.call("PackageManager", "_install", vec![file, "choren".into(), "".into()]).await?);
    for _ in 0..50 {
        if c.call("ALBehaviorManager", "isBehaviorInstalled", vec![name.as_str().into()]).await?.as_bool() == Some(true) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    c.call("ALBehaviorManager", "preloadBehavior", vec![name.as_str().into()]).await?;
    let id = c.call("ALBehaviorManager", "_getBehaviorFrameManagerId", vec![name.as_str().into()]).await?;
    let prefix = id.as_str().unwrap_or("").trim_end_matches('/').to_string();
    let Value::Object(o) = c.call("ALFrameManager", "getBehaviorDebuggerFor", vec![id.clone()]).await? else { return Err("no debugger".into()) };
    let mut events = o.remote().ok_or("remote object expected")?.subscribe("signal").await?;
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await?;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let uri = format!("{prefix}/Later_{later_id}/Say_{say_id}");
    let call = || c.call("ALFrameManager", "callBoxInput", vec![uri.as_str().into(), "onInput_onStart__".into(), Value::Void]);
    // What robot_activate_input decides: loaded diagrams come from the debugger's #onLoad keys.
    let mut loaded = std::collections::HashSet::new();
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_millis(300), events.recv()).await {
        if let Some(d) = v.items().first().and_then(Value::as_str).and_then(|k| k.strip_prefix(prefix.as_str())).and_then(choren_naoqi::runtime::loaded_diagram) {
            loaded.insert(d);
        }
    }
    let diagram_of = |path: &str| choren_naoqi::runtime::diagram_key(path.rsplit_once('/').map_or("", |p| p.0));
    println!("loaded diagrams: {loaded:?}, running: {:?}", c.call("ALBehaviorManager", "isBehaviorRunning", vec![name.as_str().into()]).await);
    println!("a root box needs onLoad: {}", !loaded.contains(&diagram_of("/Root_9")));
    println!("Later/Say needs onLoad: {}", !loaded.contains(&diagram_of(&format!("/Later_{later_id}/Say_{say_id}"))));
    let later_uri = format!("{prefix}/Later_{later_id}");
    if let Ok(Value::Object(l)) = c.call("ALFrameManager", "getBox", vec![later_uri.as_str().into()]).await {
        let mut m: Vec<_> = l.remote().unwrap().meta.methods.values().filter(|m| m.uid >= 100).map(|m| format!("{}{}", m.name, m.params)).collect();
        m.sort();
        println!("diagram box methods: {m:?}");
    }
    let Value::Object(b) = c.call("ALFrameManager", "getBox", vec![uri.as_str().into()]).await? else { return Err("no box".into()) };
    println!("__onLoad__: {:?}", b.remote().ok_or("remote box expected")?.call("__onLoad__", vec![]).await);
    println!("callBoxInput after __onLoad__: {:?}", call().await);
    // Relay as robot.rs's debugger does: the output's key → linked inputs, each loaded alone first.
    while let Ok(Some(v)) = tokio::time::timeout(Duration::from_secs(3), events.recv()).await {
        let Some(key) = v.items().first().and_then(Value::as_str).and_then(|k| k.strip_prefix(prefix.as_str())).map(String::from) else { continue };
        println!("signal {key} {:?}", v.items().get(1));
        let (path, port) = choren_naoqi::runtime::split_debugger_key(&key);
        for (to, input) in port.and_then(|port| links.get(&format!("{path}#{}", choren_naoqi::runtime::corrected(port)))).into_iter().flatten() {
            let target = format!("{prefix}{to}");
            let Value::Object(n) = c.call("ALFrameManager", "getBox", vec![target.as_str().into()]).await? else { return Err("no box".into()) };
            n.remote().unwrap().call("__onLoad__", vec![]).await?;
            println!("relay to {to}.{input}: {:?}", c.call("ALFrameManager", "callBoxInput", vec![target.as_str().into(), format!("onInput_{input}__").into(), Value::Void]).await);
        }
        if key.contains("Next") { break; }
    }
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        tokio::select! {
            Some(v) = events.recv() => println!("signal {v:?}"),
            _ = tokio::time::sleep_until(end) => break,
        }
    }
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await?;
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}
