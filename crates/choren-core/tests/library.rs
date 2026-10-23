use choren_core::edit::{History, NewBoxKind};
use choren_core::library;
use choren_core::project::{FilePolicy, Inserted, Manifest, Project};
use choren_core::{poses, view};
use std::path::{Path, PathBuf};

fn share() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
}

fn choren_lib() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/choren")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("choren-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn main_diagram(d: &choren_core::xml::Doc) -> usize {
    let r = view::root_box(d);
    let t = d.kid(r, "Timeline").unwrap();
    d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap()
}

#[test]
fn install_library_follows_xalinfo_and_includes_behavior_boxes() {
    let root = share().join("libraries/box");
    if !root.exists() { return }
    let entries = library::index(&root, "install");
    assert_eq!(entries.len(), 357);
    let tops: Vec<_> = entries.iter().map(|e| e.category.split(" / ").next().unwrap().to_string()).fold(vec![], |mut v, c| { if v.last() != Some(&c) { v.push(c) } v });
    assert_eq!(&tops[..4], ["Animation", "LEDs", "Movement", "Multimedia"].map(String::from));
    assert!(entries.iter().any(|e| e.path.ends_with("behavior.xar")));
    assert!(entries.iter().any(|e| e.name == "Set Reco. Lang."), "percent-decoded names");
    // every box's root <Box> is a valid fragment
    for e in &entries {
        let (_, xml) = library::box_xml(Path::new(&e.path)).unwrap();
        choren_core::xml::Doc::default().parse_fragment(&xml).unwrap();
    }
}

#[test]
fn choren_library_is_complete_and_parses() {
    let entries = library::index(&choren_lib(), "choren");
    let cats: std::collections::BTreeSet<_> = entries.iter().map(|e| e.category.split(" / ").next().unwrap()).collect();
    assert_eq!(cats.len(), 7, "{cats:?}");
    for e in &entries {
        let p = Project::open(Path::new(&e.path)).unwrap();
        let b = view::project(&p).behaviors.remove(0).root;
        assert!(b.script.contains("class MyClass(GeneratedClass)"), "{}", e.name);
    }
}

#[test]
fn new_project_add_behavior_insert_library_box_and_save() {
    let root = share().join("libraries/box/Animation/Moods/Negative/NAO/Fear/box.xar");
    if !root.exists() { return }
    let dir = tmp("project");
    let mut p = Project::create(&dir, "Demo").unwrap();
    assert_eq!(p.behaviors.len(), 1);
    let mut h = History::fresh();

    // insert a library box with an attached sound; second insert conflicts, then renames
    let bid = p.behaviors[0].id.clone();
    let diagram = main_diagram(&p.behaviors[0].doc);
    assert!(matches!(p.insert_library_box(&mut h, &bid, diagram, &root, 10.0, 10.0, None).unwrap(), Inserted::Done(_)));
    let Inserted::Conflicts(c) = p.insert_library_box(&mut h, &bid, diagram, &root, 50.0, 10.0, None).unwrap() else { panic!() };
    assert_eq!(c, ["behavior_1/surprise3.ogg"], "attachments go next to the behavior");
    // The UI reads `{result, value}` (app/src/workspace/authoring.ts).
    assert_eq!(serde_json::to_string(&Inserted::Conflicts(c.clone())).unwrap(), r#"{"result":"conflicts","value":["behavior_1/surprise3.ogg"]}"#);
    p.insert_library_box(&mut h, &bid, diagram, &root, 50.0, 10.0, Some(FilePolicy::Rename)).unwrap();
    assert!(p.behaviors[0].doc.write().contains("value=\"/surprise3_1.ogg\""));

    let second = p.add_behavior("", NewBoxKind::Timeline).unwrap();
    assert!(second.ends_with("behavior_2/behavior.xar"));
    let mut m: Manifest = p.manifest_fields().unwrap();
    m.version = "1.2.3".into();
    m.names = vec![("en_US".into(), "Demo & co".into())];
    p.set_manifest_fields(&m).unwrap();
    assert!(p.set_manifest_fields(&Manifest { version: "1".into(), ..m.clone() }).is_err());
    assert!(p.state(&h, vec![]).dirty);
    p.save().unwrap();

    let q = Project::open(&dir.join("Demo.pml")).unwrap();
    assert_eq!(q.behaviors.len(), 2);
    assert!(dir.join("behavior_1/surprise3.ogg").exists() && dir.join("behavior_1/surprise3_1.ogg").exists());
    assert_eq!(q.manifest_fields().unwrap().version, "1.2.3");
    assert_eq!(q.metadata["Title"], "Demo & co");
    assert_eq!(view::project(&q).behaviors[0].root.timeline.as_ref().unwrap().layers[0].keyframes[0].diagram.as_ref().unwrap().boxes.len(), 2);
    assert!(q.files.iter().any(|f| f.path == "behavior_1/surprise3_1.ogg" && f.kind == "resource"));

    let mut q = q;
    q.remove_behavior(&mut History::fresh(), &q.behaviors[1].id.clone()).unwrap();
    q.save().unwrap();
    assert_eq!(Project::open(&dir.join("Demo.pml")).unwrap().behaviors.len(), 1);
    assert!(dir.join("behavior_2/behavior.xar").exists(), "files are kept");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn user_library_add_and_remove() {
    let dir = tmp("lib");
    library::create(&dir, "Mine").unwrap();
    let (_, xml) = library::box_xml(&choren_lib().join("Programming/Time/Wait/box.xar")).unwrap();
    let file = library::add_box(&dir, "Tools/Timing", "My Wait", &xml).unwrap();
    assert!(Project::open(&file).is_ok());
    let e = library::index(&dir, "user");
    assert_eq!((e[0].name.as_str(), e[0].category.as_str()), ("My Wait", "Tools / Timing"));
    assert!(library::add_box(&dir, "../x", "y", &xml).is_err());
    assert!(library::add_box(&share().join("libraries/box"), "", "y", &xml).is_err());
    library::remove(&dir, "Tools/Timing/My Wait").unwrap();
    assert!(library::index(&dir, "user").is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn install_pose_library_parses() {
    let f = share().join("libraries/positions/standard.xap");
    if !f.exists() { return }
    let (d, poses) = poses::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(poses.len(), 3);
    assert!(poses.iter().all(|p| p.motors.len() == 26 && p.folder == "Basics"));
    assert_eq!(d.write(), std::fs::read_to_string(&f).unwrap());
}

#[test]
fn undo_after_removing_a_behavior_never_touches_another_one() {
    let dir = tmp("undo-removed");
    let mut p = Project::create(&dir, "Two").unwrap();
    p.add_behavior("second", NewBoxKind::Diagram).unwrap();
    let (a, b) = (p.behaviors[0].id.clone(), p.behaviors[1].id.clone());
    let b_before = p.behaviors[1].doc.write();
    let mut h = History::fresh();
    let root = view::root_box(&p.behaviors[0].doc);
    let values = std::collections::BTreeMap::from([("tooltip".to_string(), Some("edited".to_string()))]);
    choren_core::edit::apply(&mut p, &mut h, &a, "Edit", vec![choren_core::edit::Command::SetAttributes { node: root, values }]).unwrap();
    p.remove_behavior(&mut h, &a).unwrap();
    assert!(choren_core::edit::undo(&mut p, &mut h).is_none(), "the removed behavior's step is discarded");
    assert_eq!(p.behaviors[0].id, b);
    assert_eq!(p.behaviors[0].doc.write(), b_before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn undoing_a_replacing_library_insert_keeps_the_original_attachment() {
    let lib = share().join("libraries/box/Animation/Moods/Negative/NAO/Fear/box.xar");
    if !lib.exists() { return }
    let dir = tmp("undo-attach");
    let mut p = Project::create(&dir, "Attach").unwrap();
    std::fs::write(dir.join("behavior_1/surprise3.ogg"), b"mine").unwrap();
    let mut h = History::fresh();
    let bid = p.behaviors[0].id.clone();
    let diagram = main_diagram(&p.behaviors[0].doc);
    p.insert_library_box(&mut h, &bid, diagram, &lib, 0.0, 0.0, Some(FilePolicy::Replace)).unwrap();
    assert!(p.pending.contains_key("behavior_1/surprise3.ogg"));
    choren_core::edit::undo(&mut p, &mut h).unwrap();
    assert!(!p.pending.contains_key("behavior_1/surprise3.ogg"));
    p.save().unwrap();
    assert_eq!(std::fs::read(dir.join("behavior_1/surprise3.ogg")).unwrap(), b"mine");
    choren_core::edit::redo(&mut p, &mut h).unwrap();
    assert!(p.pending.contains_key("behavior_1/surprise3.ogg"), "redo restages the library file");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recreating_a_removed_behavior_never_replays_its_old_steps() {
    let dir = tmp("undo-reused-id");
    let mut p = Project::create(&dir, "Reuse").unwrap();
    let mut h = History::fresh();
    let id = p.add_behavior("", NewBoxKind::Diagram).unwrap();
    let diagram = main_diagram(&p.behaviors[1].doc);
    choren_core::edit::apply(&mut p, &mut h, &id, "Add", vec![choren_core::edit::Command::NewBox { diagram, kind: NewBoxKind::Python, name: "B".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap();
    p.remove_behavior(&mut h, &id).unwrap();
    let again = p.add_behavior("", NewBoxKind::Diagram).unwrap();
    assert_eq!(again, id, "the automatic name, and so the id, is reused");
    let fresh = p.behaviors[1].doc.write();
    assert!(choren_core::edit::undo(&mut p, &mut h).is_none());
    assert!(choren_core::edit::redo(&mut p, &mut h).is_none());
    assert_eq!(p.behaviors[1].doc.write(), fresh);
    std::fs::remove_dir_all(dir).unwrap();
}

/// `.cbl` box library files: Choren's export reads back as the same tree, and projects or
/// escaping paths are refused.
#[test]
fn box_library_files_import_and_export() {
    let dir = tmp("cbl");
    let cbl = library::export_cbl(&choren_lib()).unwrap();
    library::import_cbl(&cbl, &dir).unwrap();
    let names = |d: &Path| library::index(d, "user").into_iter().map(|e| (e.category, e.name)).collect::<Vec<_>>();
    assert_eq!(names(&dir), names(&choren_lib()));
    assert_eq!(std::fs::read(dir.join(".metadata")).unwrap(), std::fs::read(choren_lib().join(".metadata")).unwrap());
    assert!(library::import_cbl(&cbl, &dir).unwrap_err().contains("not empty"));
    std::fs::remove_dir_all(&dir).unwrap();

    // Metadata only in the control file (older archives).
    let old = choren_core::crg::archive(&[("xalinfo".into(), b"<Folder />".to_vec())], "Type: ChoregrapheBoxLibrary\nVersion: 3.0\ntitle: Old\n").unwrap();
    library::import_cbl(&old, &dir).unwrap();
    assert!(std::fs::read_to_string(dir.join(".metadata")).unwrap().contains("title: Old"));
    std::fs::remove_dir_all(&dir).unwrap();

    let project = choren_core::crg::write(&[("behavior.xar".into(), b"<ChoregrapheProject />".to_vec())], "p").unwrap();
    assert!(library::import_cbl(&project, &dir).unwrap_err().contains("not a box library"));
    let escape = choren_core::crg::archive(&[("../x".into(), vec![1])], "Type: ChoregrapheBoxLibrary\n").unwrap();
    assert!(library::import_cbl(&escape, &dir).is_err());
    assert!(!dir.exists() && !dir.parent().unwrap().join("x").exists());
}

#[test]
fn tree_keeps_empty_folders_raw_paths_and_root_identity() {
    let root = tmp("tree-raw");
    let a = root.join("a/Same");
    let b = root.join("b/Same");
    library::create(&a, "Same").unwrap();
    library::create(&b, "Same").unwrap();
    std::fs::create_dir_all(a.join("Empty%20folder")).unwrap();
    std::fs::create_dir_all(a.join("HiddenSymlinkTarget")).unwrap();
    std::fs::write(a.join("xalinfo"), "<Folder><Node name=\"Empty%20folder\"/></Folder>").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&b, a.join("Linked")).unwrap();
    let tree = library::tree(&a, "user", true).unwrap();
    assert_eq!(tree.children[0].name, "Empty folder");
    assert_eq!(tree.children[0].relative_path, "Empty%20folder");
    assert!(tree.children[0].children.is_empty());
    assert_eq!(tree.children[0].kind, "folder");
    assert!(!tree.children.iter().any(|n| n.name == "Linked"));
    assert!(tree.writable);
    let other = library::tree(&b, "user", true).unwrap();
    assert_eq!(tree.name, other.name);
    assert_ne!(tree.path, other.path);
    assert!(!library::tree(&a, "install", true).unwrap().writable);
    assert!(!library::tree(&a, "user", false).unwrap().writable);
    assert!(library::tree(&root.join("missing"), "user", true).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
