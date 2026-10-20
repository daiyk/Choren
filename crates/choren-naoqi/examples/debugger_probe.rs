//! Uploads a Choren-made package to a robot as Play does (`qi::File` to
//! `PackageManager._install(oss)`) and prints what the robot's behavior debugger reports
//! (`ALFrameManager.getBehaviorDebuggerFor(id)` → `signal(key, value)`) with the editor box each
//! key maps to, after stimulating an unlinked box with `ALFrameManager.callBoxInput` as Choregraphe
//! does. Run against a NAOqi robot to compare with Choren's virtual robot.
//!
//! `cargo run -p choren-naoqi --example debugger_probe -- tcp://127.0.0.1:19559 [seconds]`
use choren_core::edit::{self, Command, History, NewBoxKind};
use choren_core::project::Project;
use choren_core::view;
use choren_qi::{Client, Value};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "tcp://127.0.0.1:19559".into());
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let dir = std::env::temp_dir().join(format!("choren-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut p = Project::create(&dir, "probe")?;
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let mut new_box = |p: &mut Project, dg, kind, name: &str| -> Result<(usize, i64), String> {
        let b = edit::apply(p, &mut h, &bid, "box", vec![Command::NewBox { diagram: dg, kind, name: name.into(), x: 0.0, y: 0.0, dialog_file: None }])?.created[0];
        Ok((b, p.behaviors[0].doc.attr_num(b, "id", 0.0) as i64))
    };
    let (hello, hello_id) = new_box(&mut p, diagram, NewBoxKind::Python, "Hello World")?;
    let (inner, inner_id) = new_box(&mut p, diagram, NewBoxKind::Diagram, "Inner")?;
    let d = &p.behaviors[0].doc;
    let inner_dg = d.kid(inner, "Timeline").and_then(|t| d.kid(t, "BehaviorLayer")).and_then(|l| d.kid(l, "BehaviorKeyframe")).and_then(|k| d.kid(k, "Diagram")).ok_or("no inner diagram")?;
    let (deep, deep_id) = new_box(&mut p, inner_dg, NewBoxKind::Python, "Deep")?;
    let (motion, motion_id) = new_box(&mut p, diagram, NewBoxKind::Timeline, "Motion")?;
    let timeline = p.behaviors[0].doc.kid(motion, "Timeline").ok_or("no timeline")?;
    let mut h2 = History::fresh();
    edit::apply(&mut p, &mut h2, &bid, "layers", vec![
        Command::InsertXml { parent: timeline, xml: r#"<ActuatorList model=""><ActuatorCurve name="value" actuator="HeadYaw" recordable="1" mute="0" unit="0"><Key frame="1" value="0" /><Key frame="100" value="5" /></ActuatorCurve></ActuatorList>"#.into() },
        Command::InsertXml { parent: timeline, xml: r#"<BehaviorLayer name="my layer"><BehaviorKeyframe name="key frame" index="1"><Diagram /></BehaviorKeyframe></BehaviorLayer>"#.into() },
    ])?;
    let d = &p.behaviors[0].doc;
    let kf_dg = d.kid(d.kid(d.kid(timeline, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let (tick, tick_id) = new_box(&mut p, kf_dg, NewBoxKind::Python, "Tick")?;
    let (manual, manual_id) = new_box(&mut p, diagram, NewBoxKind::Python, "Manual")?;
    let script = "class MyClass(GeneratedClass):\n    def onInput_onStart(self):\n        self.onStopped(42)\n";
    let mut h = History::fresh();
    edit::apply(&mut p, &mut h, &bid, "wire", vec![
        Command::SetScript { r#box: hello, text: script.into() },
        Command::SetScript { r#box: manual, text: script.into() },
        Command::SetScript { r#box: deep, text: script.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: hello_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: hello_id, out_port: 4, in_owner: inner_id, in_port: 2 },
        Command::AddLink { diagram: inner_dg, out_owner: 0, out_port: 2, in_owner: deep_id, in_port: 2 },
        Command::AddLink { diagram: inner_dg, out_owner: deep_id, out_port: 4, in_owner: 0, in_port: 4 },
        Command::SetScript { r#box: tick, text: script.into() },
        Command::AddLink { diagram: kf_dg, out_owner: 0, out_port: 1, in_owner: tick_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: inner_id, out_port: 4, in_owner: motion_id, in_port: 2 },
        Command::AddLink { diagram, out_owner: motion_id, out_port: 4, in_owner: 0, in_port: 4 },
    ])?;
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
    println!("frame manager id: {id:?}");
    let Value::Object(o) = c.call("ALFrameManager", "getBehaviorDebuggerFor", vec![id.clone()]).await? else { return Err("no debugger".into()) };
    let debugger = o.remote().ok_or("remote object expected")?;
    println!("debugger: methods {:?}, signals {:?}", debugger.meta.methods.values().filter(|m| m.uid >= 100).map(|m| (&m.name, &m.params)).collect::<Vec<_>>(), debugger.meta.signals.values().map(|s| (&s.name, &s.sig)).collect::<Vec<_>>());
    let mut events = debugger.subscribe("signal").await?;
    let index = choren_naoqi::runtime::debugger_index(&p.behaviors[0].doc);
    let prefix = id.as_str().unwrap_or("").trim_end_matches('/').to_string();
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await?;
    let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let manual = format!("{prefix}/Manual_{manual_id}");
    let c = std::sync::Arc::new(c);
    let c2 = c.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        // A bang input as the editor sends it: no value, to a script method without a parameter.
        let r = c2.call("ALFrameManager", "callBoxInput", vec![manual.as_str().into(), "onInput_onStart__".into(), Value::Void]).await;
        println!("callBoxInput({manual:?}, onInput_onStart__, void): {r:?}");
    });
    loop {
        tokio::select! {
            Some(v) = events.recv() => {
                let key = v.items().first().and_then(Value::as_str).unwrap_or("").to_string();
                let path = key.strip_prefix(prefix.as_str()).map(|k| choren_naoqi::runtime::split_debugger_key(k).0.to_string());
                println!("signal {v:?} → editor box {:?}", path.and_then(|p| index.get(&p).cloned()));
            }
            _ = tokio::time::sleep_until(end) => break,
        }
    }
    println!("running after {seconds} s: {:?}", c.call("ALBehaviorManager", "isBehaviorRunning", vec![name.as_str().into()]).await?);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
