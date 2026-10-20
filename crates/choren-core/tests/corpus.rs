use choren_core::xml::Doc;
use std::path::{Path, PathBuf};

pub fn share() -> PathBuf {
    std::env::var_os("CHOREGRAPHE_SHARE").map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
    })
}

pub fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { walk(&p, ext, out) } else if p.extension().is_some_and(|x| x == ext) { out.push(p) }
    }
}

#[test]
fn every_xml_file_round_trips_byte_for_byte() {
    let mut files = vec![];
    for ext in ["xar", "xml", "xap", "ts", "pml"] { walk(&share(), ext, &mut files) }
    if files.is_empty() { eprintln!("corpus absent, skipped"); return }
    let mut bad = vec![];
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        match Doc::parse(&src) {
            Ok(d) if d.write() == src => {}
            Ok(_) => bad.push(format!("differs: {}", f.display())),
            Err(e) => bad.push(format!("{e}: {}", f.display())),
        }
    }
    assert!(bad.is_empty(), "{} of {} files: {:#?}", bad.len(), files.len(), &bad[..bad.len().min(10)]);
    eprintln!("{} files round-trip", files.len());
}

use choren_core::edit::{self, Command, History, NewBoxKind, PortKind};
use choren_core::project::Project;
use choren_core::{rules, view};
use std::collections::BTreeMap;

fn ref_project() -> PathBuf {
    std::env::var_os("REF_PROJECT").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap()).join("Downloads/Robot_activities_demos/Robot_activities_demos.pml")
    })
}

/// Every link in every diagram resolves to two ports (owner 0 = the diagram's owner box).
fn unresolved(p: &Project) -> usize {
    let mut bad = 0;
    for b in &p.behaviors {
        let d = &b.doc;
        for n in 0..d.nodes.len() {
            if d.nodes[n].parent.is_some() && matches!(d.nodes[n].kind, choren_core::xml::Kind::Element(_)) && d.is_el(n, "Link") {
                let diagram = d.nodes[n].parent.unwrap();
                let g = |k| d.attr_num(n, k, -1.0) as i64;
                if rules::find_port(d, diagram, g("outputowner"), g("indexofoutput")).is_none() || rules::find_port(d, diagram, g("inputowner"), g("indexofinput")).is_none() {
                    bad += 1;
                }
            }
        }
    }
    bad
}

#[test]
fn corpus_projects_open_and_links_resolve() {
    let mut files = vec![];
    for ext in ["xar", "crg"] { walk(&share(), ext, &mut files) }
    if files.is_empty() { return }
    let (mut xar, mut crg) = (0, 0);
    for f in &files {
        let p = Project::open(f).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unresolved(&p), 0, "{}", f.display());
        serde_json::to_string(&view::project(&p)).unwrap();
        if f.extension().unwrap() == "crg" { crg += 1; assert_eq!(p.behaviors.len(), 1) } else { xar += 1 }
    }
    assert_eq!((xar, crg), (358, 5));
    let happy = Project::open(&share().join("libraries/box/Animation/Moods/Positive/NAO/Happy/box.xar")).unwrap();
    let t = view::project(&happy).behaviors[0].root.timeline.clone().unwrap();
    assert!(t.enabled && t.fps == 25.0 && t.size == 53.0 && !t.curves.is_empty());
}

#[test]
fn reference_pml_opens() {
    let path = ref_project();
    if !path.exists() { eprintln!("reference project absent, skipped"); return }
    let p = Project::open(&path).unwrap();
    assert_eq!(p.behaviors.len(), 7);
    assert_eq!(unresolved(&p), 0);
    for b in &p.behaviors {
        assert_eq!(b.doc.write(), std::fs::read_to_string(path.parent().unwrap().join(&b.source)).unwrap());
    }
}

fn wait_box() -> Option<Project> {
    let f = share().join("projects/Aldebaran/display3d/people_perception/behavior.xar");
    f.exists().then(|| Project::open(&f).unwrap())
}

fn root_diagram(p: &Project) -> usize {
    let d = &p.behaviors[0].doc;
    let r = view::root_box(d);
    let t = d.kid(r, "Timeline").unwrap();
    d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap()
}

#[test]
fn edits_validate_and_undo_restores_bytes() {
    let Some(mut p) = wait_box() else { return };
    let original = p.behaviors[0].doc.write();
    let bid = p.behaviors[0].id.clone();
    let diagram = root_diagram(&p);
    let mut h = History::fresh();
    let s = edit::apply(&mut p, &mut h, &bid, "Add a new box", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "My box".into(), x: 10.0, y: 20.0, dialog_file: None }]).unwrap();
    assert!(s.dirty);
    let new_box = s.created[0];
    let d = &p.behaviors[0].doc;
    let new_id = d.attr_num(new_box, "id", 0.0) as i64;
    assert!(d.write().contains("name=\"My box\""));
    // the written file is still a valid XAR with the box in place
    let reparsed = choren_core::xml::Doc::parse(&d.write()).unwrap();
    assert_eq!(reparsed.write(), d.write());

    // add a port and link parent onStart (id from rail) → new box onStart (id 2)
    let rail_start = view::box_view(d, view::root_box(d)).inputs.iter().find(|i| i.nature == 2).unwrap().id;
    edit::apply(&mut p, &mut h, &bid, "Add input", vec![Command::AddPort { r#box: new_box, kind: PortKind::Input, name: "go".into(), values: BTreeMap::new() }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Link", vec![Command::AddLink { diagram, out_owner: 0, out_port: rail_start, in_owner: new_id, in_port: 2 }]).unwrap();
    // duplicates, wrong direction and bad names are rejected without changing anything
    let before = p.behaviors[0].doc.write();
    assert!(edit::apply(&mut p, &mut h, &bid, "Link", vec![Command::AddLink { diagram, out_owner: 0, out_port: rail_start, in_owner: new_id, in_port: 2 }]).unwrap_err().contains("already exists"));
    assert!(edit::apply(&mut p, &mut h, &bid, "Link", vec![Command::AddLink { diagram, out_owner: new_id, out_port: 2, in_owner: new_id, in_port: 3 }]).is_err());
    let bad = BTreeMap::from([("name".to_string(), Some("bad name".to_string()))]);
    let port = rules::ports(&p.behaviors[0].doc, new_box).last().unwrap();
    assert!(edit::apply(&mut p, &mut h, &bid, "Rename", vec![Command::SetScript { r#box: new_box, text: "x".into() }, Command::SetAttributes { node: port, values: bad }]).is_err());
    assert_eq!(p.behaviors[0].doc.write(), before);

    // moves merge into one step; script edit; paste a copy; remove the box (drops its links)
    for x in [30.0, 40.0] { edit::apply(&mut p, &mut h, &bid, "Drag", vec![Command::MoveBoxes { moves: vec![(new_box, x, 5.0)] }]).unwrap(); }
    edit::apply(&mut p, &mut h, &bid, "Script", vec![Command::SetScript { r#box: new_box, text: "class MyClass(GeneratedClass): pass".into() }]).unwrap();
    let clip = edit::copy(&p.behaviors[0].doc, &[new_box]);
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::Paste { diagram, xml: clip, dx: 20.0, dy: 20.0 }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Delete", vec![Command::Remove { nodes: vec![new_box] }]).unwrap();
    assert_eq!(unresolved(&p), 0);

    while edit::undo(&mut p, &mut h).is_some() {}
    assert_eq!(p.behaviors[0].doc.write(), original);
    assert!(!h.state(vec![]).dirty);
    while edit::redo(&mut p, &mut h).is_some() {}
    assert_eq!(unresolved(&p), 0);
    assert_eq!(p.behaviors[0].doc.write().matches("class MyClass(GeneratedClass): pass").count(), 1, "only the pasted copy remains");
}

#[test]
fn timeline_keyframes_insert_sorted_and_unique() {
    let f = share().join("libraries/box/Animation/Moods/Positive/NAO/Happy/box.xar");
    if !f.exists() { return }
    let mut p = Project::open(&f).unwrap();
    let bid = p.behaviors[0].id.clone();
    let d = &p.behaviors[0].doc;
    let t = d.kid(view::root_box(d), "Timeline").unwrap();
    let mut h = History::fresh();
    let layer = edit::apply(&mut p, &mut h, &bid, "Add new layer", vec![Command::InsertXml { parent: t, xml: "<BehaviorLayer name=\"extra\"><BehaviorKeyframe name=\"k20\" index=\"20\"><Diagram /></BehaviorKeyframe></BehaviorLayer>".into() }]).unwrap().created[0];
    for (name, idx) in [("k5", 5), ("k40", 40), ("k10", 10)] {
        let xml = format!("<BehaviorKeyframe name=\"{name}\" index=\"{idx}\"><Diagram /></BehaviorKeyframe>");
        edit::apply(&mut p, &mut h, &bid, "Add keyframe", vec![Command::InsertXml { parent: layer, xml }]).unwrap();
    }
    let d = &p.behaviors[0].doc;
    let order: Vec<_> = d.kids(layer, "BehaviorKeyframe").map(|k| d.attr_or(k, "index")).collect();
    assert_eq!(order, ["5", "10", "20", "40"]);
    assert!(edit::apply(&mut p, &mut h, &bid, "dup", vec![Command::InsertXml { parent: layer, xml: "<BehaviorKeyframe name=\"x\" index=\"10\" />".into() }]).unwrap_err().contains("Index not available"));
    assert!(choren_core::xml::Doc::parse(&p.behaviors[0].doc.write()).is_ok());
}
