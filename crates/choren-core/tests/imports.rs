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
