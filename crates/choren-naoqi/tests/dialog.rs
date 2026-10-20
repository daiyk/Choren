//! Dialog boxes and QiChat on the virtual robot.
use choren_core::edit::{self, Command, History, NewBoxKind, PortKind};
use choren_core::project::{Inserted, Project};
use choren_core::view;
use choren_qi::{Client, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn python_missing() -> bool {
    std::process::Command::new("python3").arg("--version").output().is_err()
}

/// The root diagram of a new project's behavior.
fn root_diagram(p: &Project) -> usize {
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap()
}

async fn install(dir: &Path) -> (choren_naoqi::VirtualRobot, Client, String) {
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    (vr, c, format!("{}/behavior_1", uuid.as_str().unwrap()))
}

async fn stopped(c: &Client) -> bool {
    for _ in 0..100 {
        if c.call("ALBehaviorManager", "getRunningBehaviors", vec![]).await.unwrap().items().is_empty() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn next(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>) -> String {
    let v = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.expect("an answer").unwrap();
    v.items()[0].as_str().unwrap().to_string()
}

/// A Dialog box made in Choren: typed input (forceInput and the robot panel's `ChorenSim.hear`),
/// a subrule capture kept in a variable, an ALMemory event, `Dialog/NotUnderstood`, and
/// `$asked=1` firing the box's own output, which ends the behavior.
#[tokio::test(flavor = "multi_thread")]
async fn a_dialog_box_made_in_choren_answers_typed_input() {
    if python_missing() {
        eprintln!("python3 not available, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-dialog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "talk").unwrap();
    let (bid, diagram) = (p.behaviors[0].id.clone(), root_diagram(&p));
    let mut h = History::fresh();
    p.add_dialog(&mut h, "talk", &["en_US".to_string()]).unwrap();
    let b = edit::apply(&mut p, &mut h, &bid, "new", vec![Command::NewBox { diagram, kind: NewBoxKind::Dialog, name: "Talk".into(), x: 0.0, y: 0.0, dialog_file: Some("../talk/talk.dlg".into()) }]).unwrap().created[0];
    let id = p.behaviors[0].doc.attr_num(b, "id", 0.0) as i64;
    let port = |kv: &[(&str, &str)]| kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    edit::apply(&mut p, &mut h, &bid, "edit", vec![
        Command::AddPort { r#box: b, kind: PortKind::Output, name: "asked".into(), values: port(&[("type", "1"), ("nature", "2")]) },
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: id, in_port: 2 },
        // the new output (id 5) ends the behavior
        Command::AddLink { diagram, out_owner: id, out_port: 5, in_owner: 0, in_port: 4 },
    ]).unwrap();
    p.save().unwrap();
    std::fs::write(dir.join("talk/talk_enu.top"), "topic: ~talk()\nlanguage: enu\nconcept:(greeting) [hello hi]\nu:(~greeting) ~greeting, I am NAO. What is your name?\n    u1:(_*) nice to meet you $1 $name=$1\nu:(what is my name) your name is $name\nu:(e:Choren/Bell) someone rang\nu:(e:Dialog/NotUnderstood) sorry?\nu:(stop please) ok $asked=1\n").unwrap();

    let (_vr, c, name) = install(&dir).await;
    let mut answered = c.subscribe("ALDialog", "answered").await.unwrap();
    let mut spoken = c.subscribe("ChorenSim", "spoken").await.unwrap();
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    for _ in 0..100 {
        if !c.call("ALDialog", "getActivatedTopics", vec![]).await.unwrap().items().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(c.call("ALDialog", "getActivatedTopics", vec![]).await.unwrap().items(), vec![Value::from("talk")]);

    c.call("ChorenSim", "hear", vec!["hi there".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "hello , I am NAO. What is your name ?");
    assert_eq!(next(&mut spoken).await, "hello , I am NAO. What is your name ?");
    c.call("ALDialog", "forceInput", vec!["Bob".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "nice to meet you Bob");
    assert_eq!(c.call("ALMemory", "getData", vec!["name".into()]).await.unwrap().as_str(), Some("Bob"));
    assert_eq!(c.call("ALMemory", "getData", vec!["Dialog/LastInput".into()]).await.unwrap().as_str(), Some("Bob"));
    c.call("ALDialog", "forceInput", vec!["what is my name".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "your name is Bob");
    c.call("ALMemory", "raiseEvent", vec!["Choren/Bell".into(), Value::Int(1)]).await.unwrap();
    assert_eq!(next(&mut answered).await, "someone rang");
    c.call("ALDialog", "forceInput", vec!["banana".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "sorry ?", "an input no rule matches raises Dialog/NotUnderstood");
    c.call("ALDialog", "forceInput", vec!["stop please".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "ok");
    assert!(stopped(&c).await, "$asked=1 fired the box output linked to the behavior's onStopped");
    assert!(c.call("ALDialog", "getAllLoadedTopics", vec![]).await.unwrap().items().is_empty(), "the box unloads its topic");
    // NAOqi's wording (measured).
    assert_eq!(c.call("ALDialog", "activateTopic", vec!["talk".into()]).await.unwrap_err(), "\tloadTopic::ALDialog\n\tTopic talk not found");
    assert_eq!(c.call("ALDialog", "unloadTopic", vec!["talk".into()]).await.unwrap_err(), "\tALDialog::Dialog\n\tTopic talk not found");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn share() -> PathBuf {
    std::env::var_os("CHOREGRAPHE_SHARE").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
    })
}

/// The install's Dialog box (Speech/Creation/Dialog, topic ExampleDialog) inserted as the editor
/// does: every answer is what Choregraphe's NAOqi answered to the same inputs (2026-10-19, doc 16
/// §4), including the box outputs `$playAGame=1` and `$onStopped=1`. Skipped without an install.
#[tokio::test(flavor = "multi_thread")]
async fn the_shipped_dialog_box_answers_as_naoqi() {
    let box_file = share().join("libraries/box/Speech/Creation/Dialog/box.xar");
    if !box_file.exists() || python_missing() {
        eprintln!("install or python3 absent, skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("choren-example-dialog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "example").unwrap();
    let (bid, diagram) = (p.behaviors[0].id.clone(), root_diagram(&p));
    let mut h = History::fresh();
    let Inserted::Done(st) = p.insert_library_box(&mut h, &bid, diagram, &box_file, 0.0, 0.0, None).unwrap() else { panic!("conflicts") };
    let id = p.behaviors[0].doc.attr_num(st.created[0], "id", 0.0) as i64;
    edit::apply(&mut p, &mut h, &bid, "links", vec![
        Command::AddLink { diagram, out_owner: 0, out_port: 2, in_owner: id, in_port: 2 },
        Command::AddLink { diagram, out_owner: id, out_port: 4, in_owner: 0, in_port: 4 },
    ]).unwrap();
    p.save().unwrap();

    let (_vr, c, name) = install(&dir).await;
    let mut answered = c.subscribe("ALDialog", "answered").await.unwrap();
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let reference = [
        ("hello", "hello  $Dialog/Tag=howAreYou  how are you today ?"),
        ("I am sad", "Oh , I feel sorry for you. If you feel like talking about it , don't hesitate. $Dialog/Tag=playAGame Do you want to play a game ?"),
        ("yes", "Let's play"),
        ("how are you", "I'm fine , thank you. And you , $Dialog/Tag=howAreYou  how are you today ?"),
        ("fine", "I'm glad you are fine ! $Dialog/Tag=favoriteColor And what is your favorite color between Blue and Red  ?"),
        ("blue", "blue is a very nice color and my prefered is violet. $Dialog/Tag=playAGame Do you want to play a game ?"),
        ("no", "Ok , anytime ."),
        ("thank you", "You're welcome"),
    ];
    for (input, answer) in reference {
        c.call("ALDialog", "forceInput", vec![input.into()]).await.unwrap();
        assert_eq!(next(&mut answered).await, answer, "{input}");
    }
    assert_eq!(c.call("ALMemory", "getData", vec!["ExampleDialogplayAGame".into()]).await.unwrap().as_str(), Some("1"), "$playAGame=1 reaches the box output's key");
    c.call("ALMemory", "raiseEvent", vec!["LeftBumperPressed".into(), Value::Int(1)]).await.unwrap();
    assert_eq!(next(&mut answered).await, "My left foot bumped into something  !");
    c.call("ALDialog", "forceInput", vec!["goodbye".into()]).await.unwrap();
    assert_eq!(next(&mut answered).await, "bye");
    assert!(stopped(&c).await, "$onStopped=1 stops the box, which ends the behavior");
    std::fs::remove_dir_all(&dir).unwrap();
}
