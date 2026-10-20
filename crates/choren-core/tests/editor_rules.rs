//! Editor rules: new-box defaults, and the names and ids a paste gives.

use choren_core::edit::{self, unique_box_name, Command, History, NewBoxKind};
use choren_core::project::Project;
use choren_core::xml::Doc;
use choren_core::{templates, view};

fn inner(d: &Doc, b: usize) -> usize {
    let t = d.kid(b, "Timeline").unwrap();
    d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap()
}

fn names_and_ids(d: &Doc, diagram: usize) -> Vec<(String, String)> {
    d.kids(diagram, "Box").map(|b| (d.attr(b, "name").unwrap(), d.attr(b, "id").unwrap())).collect()
}

#[test]
fn unique_box_name_strips_and_counts() {
    let taken = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(unique_box_name("Say", &taken(&[])), "Say");
    assert_eq!(unique_box_name("Say (3)", &taken(&[])), "Say", "the suffix goes even without a clash");
    assert_eq!(unique_box_name("Say", &taken(&["Say", "Say (1)"])), "Say (2)");
    assert_eq!(unique_box_name("Say (1)", &taken(&["Say"])), "Say (1)");
    assert_eq!(unique_box_name("A (1) (2)", &taken(&["A (1)"])), "A (1) (1)");
    assert_eq!(unique_box_name(" (4)", &taken(&[])), " (4)", "(.+) needs a character before the suffix");
    assert_eq!(unique_box_name("x (4a)", &taken(&[])), "x (4a)");
}

#[test]
fn new_box_templates() {
    let timeline = templates::new_box(NewBoxKind::Timeline, "T");
    assert!(timeline.contains("<Timeline enable=\"1\" fps=\"25\" start_frame=\"1\" end_frame=\"-1\" size=\"-1\" />"));
    assert!(!timeline.contains("BehaviorLayer"), "initTimeline(true, false) creates no layer");
    let diagram = templates::new_box(NewBoxKind::Diagram, "D");
    assert!(diagram.contains("<Timeline enable=\"0\">") && diagram.contains("name=\"behavior_layer1\"") && diagram.contains("name=\"keyframe1\" index=\"1\""));
    for kind in [NewBoxKind::Python, NewBoxKind::Diagram, NewBoxKind::Timeline] {
        let b = templates::new_box(kind, "x");
        assert!(b.contains("localization=\"8\"") && b.contains("name=\"onLoad\"") && b.contains("name=\"onStopped\""));
    }
}

#[test]
fn paste_renames_and_keeps_free_ids() {
    let dir = std::env::temp_dir().join(format!("choren-editor-rules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Project::create(&dir, "rules").unwrap();
    let (mut h, bid) = (History::fresh(), p.behaviors[0].id.clone());
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let mut add = |p: &mut Project, diagram, kind, name: &str| {
        edit::apply(p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind, name: name.into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0]
    };
    let a = add(&mut p, diagram, NewBoxKind::Python, "A");
    let b = add(&mut p, diagram, NewBoxKind::Python, "B");
    let sub = add(&mut p, diagram, NewBoxKind::Diagram, "Sub");
    let clip = edit::copy(&p.behaviors[0].doc, &[a, b]);
    let mut h = History::fresh();

    // into the same diagram: names get " (1)", taken ids 1 and 2 become the first free ones
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::Paste { diagram, xml: clip.clone(), dx: 20.0, dy: 20.0 }]).unwrap();
    let got = names_and_ids(&p.behaviors[0].doc, diagram);
    assert_eq!(got[3..], [("A (1)".into(), "4".into()), ("B (1)".into(), "5".into())]);

    // into an empty diagram: names and ids are kept
    let below = inner(&p.behaviors[0].doc, sub);
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::Paste { diagram: below, xml: clip, dx: 0.0, dy: 0.0 }]).unwrap();
    assert_eq!(names_and_ids(&p.behaviors[0].doc, below), [("A".into(), "1".into()), ("B".into(), "2".into())]);
    std::fs::remove_dir_all(&dir).unwrap();
}
