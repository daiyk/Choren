//! Resource arbitration end to end: two script boxes made with the edit engine compete for a
//! motor resource. The holder's mode decides what happens to the second box.
use choren_core::edit::{self, Command, History, NewBoxKind, PortKind};
use choren_core::project::Project;
use choren_core::view;
use choren_qi::{Client, Value};
use std::time::Duration;

const HOLDER: &str = r#"import threading
class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self)

    def onLoad(self):
        self.memory = self.session().service("ALMemory")

    def onInput_onStart(self):
        self.memory.insertData("A/started", 1)
        if self.getParameter("ReleaseLater"):
            threading.Timer(0.5, self.releaseResource).start()
        self.started()

    def onResource(self, name):
        self.memory.insertData("A/asked", name)
        self.releaseResource()

    def onInput_onStop(self):
        self.memory.insertData("A/stopped", 1)
"#;

const TAKER: &str = r#"class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self)

    def onInput_onStart(self):
        self.session().service("ALMemory").insertData("B/started", 1)
        self.onStopped()

    def onResourceError(self):
        self.session().service("ALMemory").insertData("B/failed", 1)
"#;

/// Holder `A` (Left arm, `holder_mode`) starts first and hands over to taker `B` (All motors,
/// `taker_mode`, `timeout` s). B's onStopped ends the behavior.
fn project(dir: &std::path::Path, holder_mode: &str, taker_mode: &str, timeout: u32, release_later: bool) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut p = Project::create(dir, "res").unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let r = view::root_box(d);
    let t = d.kid(r, "Timeline").unwrap();
    let diagram = d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let mut h = History::fresh();
    let mut new_box = |p: &mut Project, name: &str| {
        let n = edit::apply(p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: name.into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
        (n, p.behaviors[0].doc.attr_num(n, "id", 0.0) as i64)
    };
    let (a, aid) = new_box(&mut p, "Holder");
    let (b, bid_) = new_box(&mut p, "Taker");
    let res = |m: &str, t: u32, name: &str| format!(r#"<Resource name="{name}" type="{m}" timeout="{t}" />"#);
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: a, text: HOLDER.into() },
        Command::SetScript { r#box: b, text: TAKER.into() },
        Command::AddPort { r#box: a, kind: PortKind::Output, name: "started".into(), values: Default::default() },
        Command::AddPort { r#box: a, kind: PortKind::Parameter, name: "ReleaseLater".into(), values: [("value".to_string(), if release_later { "1" } else { "0" }.to_string()), ("content_type".to_string(), "0".to_string())].into() },
        Command::InsertXml { parent: a, xml: res(holder_mode, 0, "Left arm") },
        Command::InsertXml { parent: b, xml: res(taker_mode, timeout, "All motors") },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: aid, in_port: 2 },
        Command::AddLink { diagram, out_owner: aid, out_port: 5, in_owner: bid_, in_port: 2 },
        Command::AddLink { diagram, out_owner: bid_, out_port: 4, in_owner: 0, in_port: 4 },
    ])
    .unwrap();
    p.save().unwrap();
}

async fn run(dir: &std::path::Path) -> (choren_naoqi::VirtualRobot, Client) {
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    c.call("PackageManager", "install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = c.call("ALBehaviorManager", "getInstalledBehaviors", vec![]).await.unwrap().items()[0].as_str().unwrap().to_string();
    tokio::time::timeout(Duration::from_secs(20), c.call("ALBehaviorManager", "runBehavior", vec![name.as_str().into()])).await.expect("behavior finished").unwrap();
    (vr, c)
}

async fn mem(c: &Client, key: &str) -> Option<Value> {
    c.call("ALMemory", "getData", vec![key.into()]).await.ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn holders_stop_release_or_keep_their_resources() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not available, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-res-{}", std::process::id()));

    // Stop on demand: the holder is stopped, the taker runs.
    project(&dir, "Stop on demand", "Lock", 0, false);
    let (vr, c) = run(&dir).await;
    assert!(mem(&c, "A/stopped").await.is_some());
    assert!(mem(&c, "B/started").await.is_some());
    assert!(vr.robotd.runtime.resources.held().is_empty(), "released at the end");
    drop(vr);

    // Callback on demand: onResource runs and releases.
    project(&dir, "Callback on demand", "Lock", 0, false);
    let (_vr, c) = run(&dir).await;
    assert_eq!(mem(&c, "A/asked").await.and_then(|v| v.as_str().map(String::from)).as_deref(), Some("Left arm"));
    assert!(mem(&c, "A/stopped").await.is_none());
    assert!(mem(&c, "B/started").await.is_some());

    // Lock with no timeout: the taker fails, onResourceError runs, and it reports onStopped.
    project(&dir, "Lock", "Lock", 0, false);
    let (_vr, c) = run(&dir).await;
    assert!(mem(&c, "B/failed").await.is_some());
    assert!(mem(&c, "B/started").await.is_none());

    // Wait: the taker waits for the holder's releaseResource() half a second later.
    project(&dir, "Lock", "Wait", 0, true);
    let (_vr, c) = run(&dir).await;
    assert!(mem(&c, "B/started").await.is_some());
    assert!(mem(&c, "B/failed").await.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Stand Up › Goto Posture: a diagram box locks "All motors" and the script box inside it locks
/// "All motors" again. The inner box runs with its parent's resources.
#[tokio::test(flavor = "multi_thread")]
async fn a_box_inside_a_holder_uses_its_resources() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not available, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-res-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "nested").unwrap();
    let bid = p.behaviors[0].id.clone();
    let inner_diagram = |d: &choren_core::xml::Doc, b| d.kid(d.kid(d.kid(d.kid(b, "Timeline").unwrap(), "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
    let diagram = inner_diagram(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let mut h = History::fresh();
    let outer = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Diagram, name: "Stand Up".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let inside = inner_diagram(&p.behaviors[0].doc, outer);
    let inner = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram: inside, kind: NewBoxKind::Python, name: "Goto Posture".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let (oid, iid) = {
        let d = &p.behaviors[0].doc;
        (d.attr_num(outer, "id", 0.0) as i64, d.attr_num(inner, "id", 0.0) as i64)
    };
    let lock = r#"<Resource name="All motors" type="Lock" timeout="0" />"#;
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::SetScript { r#box: inner, text: TAKER.into() },
        Command::InsertXml { parent: outer, xml: lock.into() },
        Command::InsertXml { parent: inner, xml: lock.into() },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: oid, in_port: 2 },
        Command::AddLink { diagram: inside, out_owner: 0, out_port: 2, in_owner: iid, in_port: 2 },
        Command::AddLink { diagram: inside, out_owner: iid, out_port: 4, in_owner: 0, in_port: 4 },
        Command::AddLink { diagram, out_owner: oid, out_port: 4, in_owner: 0, in_port: 4 },
    ])
    .unwrap();
    p.save().unwrap();
    let (vr, c) = run(&dir).await;
    assert!(mem(&c, "B/started").await.is_some(), "the inner box ran");
    assert!(mem(&c, "B/failed").await.is_none());
    assert!(vr.robotd.runtime.resources.held().is_empty(), "released at the end");
    std::fs::remove_dir_all(&dir).unwrap();
}
