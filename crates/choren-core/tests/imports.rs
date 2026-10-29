use choren_core::{crg, edit::{self, History, NewBoxKind}, project::{FilePolicy, Project}, templates};
use std::{fs, path::PathBuf};

fn setup(name: &str) -> (PathBuf, Project, History) {
    let root = std::env::temp_dir().join(format!("choren-import-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("source")).unwrap();
    fs::create_dir_all(root.join("project")).unwrap();
    let p = Project::create(&root.join("project"), "demo").unwrap();
    (root, p, History::fresh())
}

#[test]
fn multiple_files_and_nested_folder_stage_undo_save_and_reopen() {
    let (root, mut p, mut h) = setup("resources");
    let sources = [root.join("source/声音.wav"), root.join("source/image.png"), root.join("source/image.choren-tmp")];
    for (i, source) in sources.iter().enumerate() { fs::write(source, [i as u8, 255]).unwrap(); }
    p.import_files(&mut h, &sources, "assets", false, None).unwrap();
    assert!(!root.join("project/assets/声音.wav").exists());
    assert_eq!(p.read_file("assets/声音.wav"), Some(vec![0, 255]));
    assert_eq!(h.state(vec![]).undo.as_deref(), Some("Import files"));
    edit::undo(&mut p, &mut h).unwrap();
    assert!(!p.files.iter().any(|f| f.path.starts_with("assets/")));
    assert!(p.pending.is_empty());
    edit::redo(&mut p, &mut h).unwrap();
    fs::create_dir_all(root.join("source/media/nested")).unwrap();
    fs::write(root.join("source/media/nested/note.txt"), "hello").unwrap();
    p.import_files(&mut h, &[root.join("source/media")], "assets", true, None).unwrap();
    fs::create_dir_all(root.join("project/assets")).unwrap();
    let reserved = root.join(format!("project/assets/.choren-save-{}-0", std::process::id()));
    fs::write(&reserved, "keep existing file").unwrap();
    p.save().unwrap();
    assert_eq!(fs::read_to_string(reserved).unwrap(), "keep existing file");
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(reopened.read_file("assets/media/nested/note.txt").unwrap(), b"hello");
    assert_eq!(reopened.read_file("assets/声音.wav").unwrap(), [0, 255]);
    assert_eq!(reopened.read_file("assets/image.choren-tmp").unwrap(), [2, 255]);
    assert_eq!(reopened.files.iter().filter(|f| f.kind == "resource").count(), 4);
    assert_eq!(fs::read(&sources[0]).unwrap(), [0, 255]);
}

#[test]
fn conflicts_bad_paths_and_read_failures_leave_project_unchanged() {
    let (root, mut p, mut h) = setup("failures");
    let file = root.join("source/note.txt");
    fs::write(&file, "new").unwrap();
    fs::write(root.join("project/note.txt"), "keep").unwrap();
    let original = p.pml.as_ref().unwrap().write();
    // Same-named files are reported, not changed.
    assert_eq!(p.import_files(&mut h, &[file.clone()], "", false, None).unwrap(), ["note.txt"]);
    for destination in ["../outside", "/absolute", "x/../../bad", "C:/outside", "x//bad", "./bad", "x\\bad"] {
        assert!(p.import_files(&mut h, &[file.clone()], destination, false, None).is_err(), "{destination}");
        assert_eq!(p.pml.as_ref().unwrap().write(), original);
        assert!(p.pending.is_empty());
    }
    assert!(p.import_files(&mut h, &[file.clone(), root.join("source/missing")], "assets", false, None).is_err());
    assert!(p.import_files(&mut h, &[file.clone(), file], "assets", false, None).is_err());
    assert!(p.pending.is_empty());
    assert_eq!(h.revision, 0);
    assert_eq!(fs::read_to_string(root.join("project/note.txt")).unwrap(), "keep");
}

#[test]
fn xar_and_crg_behaviors_preserve_documents_resources_and_manifest() {
    let (root, mut p, _) = setup("behavior");
    let xml = templates::behavior(NewBoxKind::Timeline);
    let xar = root.join("source/dance.xar");
    fs::write(&xar, &xml).unwrap();
    p.import_behavior(&xar, "Dance 中文", None).unwrap();
    assert_eq!(p.behaviors.last().unwrap().doc.write(), xml);
    assert!(!root.join("project/Dance 中文").exists());
    let archive = root.join("source/wave.crg");
    fs::write(&archive, crg::write(&[("behavior.xar".into(), xml.as_bytes().to_vec()), ("audio/hello.wav".into(), vec![3, 2, 1])], "wave").unwrap()).unwrap();
    p.import_behavior(&archive, "Wave", None).unwrap();
    p.save().unwrap();
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(reopened.behaviors.len(), 3);
    assert_eq!(reopened.behaviors[1].doc.write(), xml);
    assert_eq!(reopened.read_file("audio/hello.wav").unwrap(), [3, 2, 1]);
    assert!(reopened.manifest.unwrap().1.write().contains("path=\"Wave\""));
}

#[test]
fn malformed_or_conflicting_behavior_is_atomic() {
    let (root, mut p, mut h) = setup("bad-behavior");
    let xar = root.join("source/bad.xar");
    fs::write(&xar, "<not-a-behavior/>").unwrap();
    let original = p.pml.as_ref().unwrap().write();
    assert!(p.import_behavior(&xar, "Broken", None).is_err());
    assert_eq!(p.pml.as_ref().unwrap().write(), original);
    let pml = root.join("source/wrong.pml");
    fs::write(&pml, "<Package/>").unwrap();
    assert!(p.import_behavior(&pml, "Wrong", None).unwrap_err().contains("Choose a XAR"));
    fs::write(&xar, templates::behavior(NewBoxKind::Diagram)).unwrap();
    p.import_files(&mut h, &[xar.clone()], "Taken", false, None).unwrap();
    assert_eq!(p.import_behavior(&xar, "behavior_1", None).unwrap(), ["behavior_1/behavior.xar"]);
    for name in ["Taken", "taken", "Behavior_1", "../outside", ".", "bad/folder"] {
        assert!(p.import_behavior(&xar, name, None).is_err(), "{name}");
    }
    let archive = root.join("source/bad.crg");
    fs::write(&archive, crg::write(&[("behavior.xar".into(), fs::read(&xar).unwrap()), ("manifest.xml/escape".into(), vec![1])], "bad").unwrap()).unwrap();
    assert!(p.import_behavior(&archive, "Broken", None).is_err());
    assert_eq!(p.behaviors.len(), 1);
    assert!(!p.structure_dirty || !p.pml.as_ref().unwrap().write().contains("src=\"Broken\""));
}

#[cfg(unix)]
#[test]
fn folder_import_refuses_source_and_destination_symlinks() {
    use std::os::unix::fs::symlink;
    let (root, mut p, mut h) = setup("symlinks");
    fs::write(root.join("secret"), "outside").unwrap();
    symlink(root.join("secret"), root.join("source/link")).unwrap();
    assert!(p.import_files(&mut h, &[root.join("source")], "", true, None).unwrap_err().contains("Symbolic"));
    fs::remove_file(root.join("source/link")).unwrap();
    fs::write(root.join("source/note.txt"), "content").unwrap();
    symlink(root.join("source"), root.join("project/escape")).unwrap();
    assert!(p.import_files(&mut h, &[root.join("source/note.txt")], "escape", false, None).is_err());
    assert!(p.pending.is_empty());
    assert_eq!(h.revision, 0);
}

#[test]
fn replace_overwrites_and_folders_merge() {
    let (root, mut p, mut h) = setup("replace");
    fs::write(root.join("source/note.txt"), "new").unwrap();
    fs::write(root.join("project/note.txt"), "old").unwrap();
    assert_eq!(p.import_files(&mut h, &[root.join("source/note.txt")], "", false, None).unwrap(), ["note.txt"]);
    assert!(p.pending.is_empty());
    assert_eq!(h.revision, 0);
    p.import_files(&mut h, &[root.join("source/note.txt")], "", false, Some(FilePolicy::Replace)).unwrap();
    assert_eq!(p.read_file("note.txt").unwrap(), b"new");
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.read_file("note.txt").unwrap(), b"old");
    assert!(!p.files.iter().any(|f| f.path == "note.txt"));
    // Behaviors and the PML are never overwritten by a file import.
    fs::write(root.join("source/demo.pml"), "x").unwrap();
    assert!(p.import_files(&mut h, &[root.join("source/demo.pml")], "", false, Some(FilePolicy::Replace)).is_err());

    fs::create_dir_all(root.join("source/media")).unwrap();
    fs::write(root.join("source/media/a.txt"), "a1").unwrap();
    p.import_files(&mut h, &[root.join("source/media")], "", true, None).unwrap();
    fs::write(root.join("source/media/a.txt"), "a2").unwrap();
    fs::write(root.join("source/media/b.txt"), "b").unwrap();
    assert_eq!(p.import_files(&mut h, &[root.join("source/media")], "", true, None).unwrap(), ["media/a.txt"]);
    p.import_files(&mut h, &[root.join("source/media")], "", true, Some(FilePolicy::UseExisting)).unwrap();
    assert_eq!(p.read_file("media/a.txt").unwrap(), b"a1");
    assert_eq!(p.read_file("media/b.txt").unwrap(), b"b");
    p.import_files(&mut h, &[root.join("source/media")], "", true, Some(FilePolicy::Replace)).unwrap();
    assert_eq!(p.read_file("media/a.txt").unwrap(), b"a2");
    assert_eq!(p.files.iter().filter(|f| f.path.starts_with("media/")).count(), 2);

    let xar = root.join("source/dance.xar");
    fs::write(&xar, templates::behavior(NewBoxKind::Diagram)).unwrap();
    assert!(p.import_behavior(&xar, "Dance", None).unwrap().is_empty());
    assert_eq!(p.import_behavior(&xar, "Dance", None).unwrap(), ["Dance/behavior.xar"]);
    let timeline = templates::behavior(NewBoxKind::Timeline);
    fs::write(&xar, &timeline).unwrap();
    p.import_behavior(&xar, "Dance", Some(FilePolicy::Replace)).unwrap();
    assert_eq!(p.behaviors.len(), 2);
    assert_eq!(p.behaviors[1].doc.write(), timeline);
}

#[test]
fn folder_import_lists_behaviors_dialogs_and_topics() {
    let (root, mut p, mut h) = setup("folder-content");
    let show = root.join("source/show");
    fs::create_dir_all(show.join("behavior_1")).unwrap();
    fs::create_dir_all(show.join("greet")).unwrap();
    let xml = templates::behavior(NewBoxKind::Timeline);
    fs::write(show.join("behavior_1/behavior.xar"), &xml).unwrap();
    fs::write(show.join("behavior_1/hello.wav"), [1, 2]).unwrap();
    fs::write(show.join("greet/greet.dlg"), "multilanguage:greet\n  enu:greet_enu.top\n").unwrap();
    fs::write(show.join("greet/greet_enu.top"), "topic: ~greet()\nlanguage: enu\n").unwrap();
    p.import_files(&mut h, &[show.clone()], "", true, None).unwrap();
    let kind = |p: &Project, path: &str| p.files.iter().find(|f| f.path == path).map(|f| f.kind);
    assert_eq!(p.behaviors.len(), 2);
    assert_eq!(p.behaviors[1].source, "show/behavior_1/behavior.xar");
    assert_eq!(p.behaviors[1].doc.write(), xml);
    assert_eq!(choren_core::view::project(&p).behaviors.len(), 2, "the Outline sees the imported behavior");
    assert_eq!(kind(&p, "show/behavior_1/behavior.xar"), Some("behavior"));
    assert_eq!(kind(&p, "show/behavior_1/hello.wav"), Some("resource"));
    assert_eq!(kind(&p, "show/greet/greet.dlg"), Some("dialog"));
    assert_eq!(kind(&p, "show/greet/greet_enu.top"), Some("topic"));
    assert!(!p.pending.contains_key("show/behavior_1/behavior.xar"), "the behavior document is saved from memory");

    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.behaviors.len(), 1);
    assert!(!p.files.iter().any(|f| f.path.starts_with("show/")) && p.pending.is_empty());
    edit::redo(&mut p, &mut h).unwrap();
    p.save().unwrap();
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(reopened.behaviors.len(), 2);
    assert_eq!(kind(&reopened, "show/greet/greet_enu.top"), Some("topic"));
    assert!(reopened.manifest.as_ref().unwrap().1.write().contains("path=\"show/behavior_1\""));
    assert!(reopened.pml.as_ref().unwrap().write().contains("language=\"en_US\""));

    // A behavior.xar needs its own folder.
    assert!(p.import_files(&mut h, &[show.join("behavior_1/behavior.xar")], "", false, None).unwrap_err().contains("own folder"));
    fs::write(root.join("source/behavior.xar"), "<broken/>").unwrap();
    assert!(p.import_files(&mut h, &[root.join("source/behavior.xar")], "bad", false, None).unwrap_err().contains("not a valid behavior"));
    assert!(!p.files.iter().any(|f| f.path.starts_with("bad/")));
}

#[test]
fn reimporting_a_behavior_listed_as_a_resource_lists_it_as_a_behavior() {
    let (root, mut p, mut h) = setup("relist");
    fs::create_dir_all(root.join("source/old")).unwrap();
    fs::write(root.join("source/old/behavior.xar"), templates::behavior(NewBoxKind::Diagram)).unwrap();
    // How earlier versions imported it: a plain resource.
    p.list(&project_listing("old/behavior.xar"), true).unwrap();
    fs::create_dir_all(root.join("project/old")).unwrap();
    fs::copy(root.join("source/old/behavior.xar"), root.join("project/old/behavior.xar")).unwrap();
    assert_eq!(p.import_files(&mut h, &[root.join("source/old")], "", true, None).unwrap(), ["old/behavior.xar"]);
    p.import_files(&mut h, &[root.join("source/old")], "", true, Some(FilePolicy::Replace)).unwrap();
    assert_eq!(p.files.iter().filter(|f| f.path == "old/behavior.xar").map(|f| f.kind).collect::<Vec<_>>(), ["behavior"]);
    assert_eq!(p.behaviors.len(), 2);
    p.save().unwrap();
    let pml = p.pml.as_ref().unwrap().write();
    assert!(!pml.contains("<File name=\"behavior.xar\""), "{pml}");
}

fn project_listing(path: &str) -> choren_core::project::Listing {
    choren_core::project::Listing::new("Resources", format!("<File name=\"behavior.xar\" src=\"{path}\" />"), path, "resource")
}

#[test]
fn moving_a_behavior_folder_retargets_paths_and_moves_files_on_save() {
    let (root, mut p, mut h) = setup("move");
    let show = root.join("source/show");
    fs::create_dir_all(show.join("Dance")).unwrap();
    fs::create_dir_all(show.join("greet")).unwrap();
    fs::write(show.join("Dance/behavior.xar"), templates::behavior(NewBoxKind::Diagram)).unwrap();
    fs::write(show.join("Dance/hello.wav"), [1, 2]).unwrap();
    fs::write(show.join("greet/greet.dlg"), "multilanguage:greet\n").unwrap();
    p.import_files(&mut h, &[show.clone()], "", true, None).unwrap();
    p.save().unwrap();
    fs::write(root.join("project/show/Dance/notes.txt"), "unlisted").unwrap();
    let id = p.behaviors[1].id.clone();
    // A dialog box in Dance points at ../greet/greet.dlg (relative to its behavior file).
    {
        let d = &mut p.behaviors[1].doc;
        let b = choren_core::view::root_box(d);
        edit::insert_raw(d, b, "<dialogFile>../greet/greet.dlg</dialogFile>").unwrap();
    }
    edit::change_project(&mut p, &mut h, "Move", |p| p.move_path("show/Dance", "Dance")).unwrap();
    let b = p.behaviors.iter().find(|b| b.id == id).expect("the behavior keeps its id");
    assert_eq!((b.source.as_str(), b.name.as_str()), ("Dance/behavior.xar", "Dance"));
    assert!(b.doc.write().contains("<dialogFile>../show/greet/greet.dlg</dialogFile>"), "still relative to the moved behavior");
    assert!(p.files.iter().any(|f| f.path == "Dance/hello.wav" && f.kind == "resource"));
    assert!(p.pml.as_ref().unwrap().write().contains(r#"src="Dance""#));
    assert!(p.manifest.as_ref().unwrap().1.write().contains(r#"path="Dance""#));
    assert!(root.join("project/show/Dance/hello.wav").exists(), "nothing moves before saving");
    p.save().unwrap();
    assert!(!root.join("project/show/Dance").exists());
    assert_eq!(fs::read(root.join("project/Dance/hello.wav")).unwrap(), [1, 2]);
    assert_eq!(fs::read_to_string(root.join("project/Dance/notes.txt")).unwrap(), "unlisted");
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert!(reopened.behaviors.iter().any(|b| b.source == "Dance/behavior.xar"));
    edit::undo(&mut p, &mut h).unwrap();
    p.save().unwrap();
    assert_eq!(fs::read(root.join("project/show/Dance/hello.wav")).unwrap(), [1, 2], "undo after saving moves it back");
    assert!(p.behaviors.iter().any(|b| b.source == "show/Dance/behavior.xar"));

    for (from, to) in [("show", "show/inner"), ("demo.pml", "x.pml"), ("show/Dance/behavior.xar", "x.xar"), ("show/Dance", "behavior_1"), ("show/Dance", "show/dance"), ("missing", "x")] {
        assert!(p.move_path(from, to).is_err(), "{from} → {to}");
    }
    // A new behavior at the old path gets its own id.
    let moved = p.behaviors.iter().position(|b| b.id == id).unwrap();
    p.move_path("show/Dance", "Moved").unwrap();
    let again = p.add_behavior("show", NewBoxKind::Diagram);
    assert!(again.is_err() || p.behaviors.iter().filter(|b| b.id == id).count() == 1);
    assert_eq!(p.behaviors[moved].id, id);
}

#[test]
fn duplicating_and_reordering_behaviors() {
    let (root, mut p, mut h) = setup("duplicate");
    fs::write(root.join("project/behavior_1/sound.ogg"), [7]).unwrap();
    p.list(&choren_core::project::Listing::new("Resources", r#"<File name="sound" src="behavior_1/sound.ogg" />"#.into(), "behavior_1/sound.ogg", "resource"), true).unwrap();
    let mut m = p.manifest_fields().unwrap();
    m.behaviors.as_mut().unwrap()[0].names = vec![("en_US".into(), "Greeter".into())];
    p.set_manifest_fields(&m).unwrap();
    let id = p.behaviors[0].id.clone();
    assert_eq!(p.copy_name("behavior_1"), "behavior_1 copy");
    let mut copy = String::new();
    edit::change_project(&mut p, &mut h, "Duplicate", |p| { copy = p.duplicate_behavior(&id, "behavior_1 copy")?; Ok(()) }).unwrap();
    assert_eq!(p.copy_name("behavior_1"), "behavior_1 copy 2");
    assert!(p.files.iter().any(|f| f.path == "behavior_1 copy/sound.ogg" && f.kind == "resource"));
    assert!(p.manifest.as_ref().unwrap().1.write().matches("Greeter").count() == 2, "the copy keeps its properties");
    p.save().unwrap();
    assert_eq!(fs::read(root.join("project/behavior_1 copy/sound.ogg")).unwrap(), [7]);
    assert_eq!(Project::open(&root.join("project/demo.pml")).unwrap().behaviors.len(), 2);
    assert!(p.duplicate_behavior(&id, "elsewhere/behavior_1").is_err());

    p.add_behavior("third", NewBoxKind::Diagram).unwrap();
    let names = |p: &Project| p.behaviors.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let third = p.behaviors[2].id.clone();
    edit::change_project(&mut p, &mut h, "Reorder", |p| p.reorder_behavior(&third, &id, false)).unwrap();
    assert_eq!(names(&p), ["third", "behavior_1", "behavior_1 copy"]);
    let pml = p.pml.as_ref().unwrap().write();
    assert!(pml.find(r#"src="third""#).unwrap() < pml.find(r#"src="behavior_1""#).unwrap());
    let manifest = p.manifest.as_ref().unwrap().1.write();
    assert!(manifest.find(r#"path="third""#).unwrap() < manifest.find(r#"path="behavior_1""#).unwrap());
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(names(&p), ["behavior_1", "behavior_1 copy", "third"]);
    edit::change_project(&mut p, &mut h, "Reorder", |p| p.reorder_behavior(&id, &third, true)).unwrap();
    assert_eq!(names(&p), ["behavior_1 copy", "third", "behavior_1"]);
    let resources: Vec<_> = p.files.iter().filter(|f| f.kind == "resource").map(|f| f.path.as_str()).collect();
    assert_eq!(resources, ["behavior_1/sound.ogg", "behavior_1 copy/sound.ogg"], "resources keep their PML order");
    assert!(p.reorder_behavior(&id, &id, false).is_err());
    p.save().unwrap();
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(names(&reopened), ["behavior_1 copy", "third", "behavior_1"]);
}

#[test]
fn chorenproj_keeps_the_tree_order_beside_the_pml() {
    let (root, mut p, mut h) = setup("chorenproj");
    let file = root.join("project/demo.chorenproj");
    assert_eq!(p.choren_file().as_deref(), Some("demo.chorenproj"));
    let order = serde_json::json!({"": ["behavior_1", "demo.pml", "manifest.xml"]});
    edit::change_project(&mut p, &mut h, "Reorder", |p| p.set_choren("treeOrder", Some(order.clone()))).unwrap();
    assert!(!file.exists(), "written on save");
    assert!(p.state(&h, vec![]).dirty);
    edit::undo(&mut p, &mut h).unwrap();
    assert!(p.choren.is_empty() && p.pending.is_empty());
    edit::redo(&mut p, &mut h).unwrap();
    p.save().unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains(r#""format": "chorenproj""#) && text.contains(r#""version": 1"#));
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(reopened.choren["treeOrder"], order);
    assert!(!reopened.files.iter().any(|f| f.path.ends_with(".chorenproj")), "not a PML entry");
    // A rename keeps its place in the order.
    edit::change_project(&mut p, &mut h, "Rename", |p| p.move_path("behavior_1", "Dance")).unwrap();
    assert_eq!(p.choren["treeOrder"][""][0], "Dance");
    assert!(p.set_choren("version", Some(2.into())).is_err());
    fs::write(&file, "not json").unwrap();
    assert!(Project::open(&root.join("project/demo.pml")).unwrap().diagnostics.iter().any(|d| d.message.contains("Choren project data")));
}
