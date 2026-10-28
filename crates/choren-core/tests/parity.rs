use choren_core::{edit::{self, Command, History}, project::{self, Project}, library, view};
use std::{fs, path::PathBuf};

fn setup(name: &str) -> (PathBuf, Project, History) {
    let root = std::env::temp_dir().join(format!("choren-parity-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("project")).unwrap();
    let p = Project::create(&root.join("project"), "demo").unwrap();
    (root, p, History::fresh())
}

#[test]
fn project_copy_and_package_preserve_current_documents_and_assets() {
    let (root, mut p, mut h) = setup("copy");
    fs::write(root.join("sound.wav"), [0, 255, 12]).unwrap();
    p.import_files(&mut h, &[root.join("sound.wav")], "audio", false, None).unwrap();
    p.add_behavior("unsaved", choren_core::edit::NewBoxKind::Timeline).unwrap();
    let mut metadata = p.manifest_fields().unwrap();
    metadata.descriptions = vec![("en_US".into(), "A & B <story>".into()), ("fr_FR".into(), "Bonjour".into())];
    metadata.supported_languages = vec!["en_US".into(), "fr_FR".into()];
    p.set_manifest_fields(&metadata).unwrap();
    let (manifest_path, m) = p.manifest.as_mut().unwrap();
    let mr = m.root().unwrap();
    edit::insert_raw(m, mr, "<requirements><naoqiRequirement minVersion=\"2.8\" /></requirements>").unwrap();
    let manifest_path = manifest_path.clone();
    let expected = m.write();
    fs::create_dir(root.join("copy")).unwrap();
    let dest = p.save_as_folder(&root.join("copy"), "renamed").unwrap();
    let copy = Project::open(&dest).unwrap();
    assert_eq!(copy.name, "renamed");
    assert_eq!(copy.behaviors.len(), 2);
    assert!(copy.behaviors[1].doc.write().contains("Timeline"));
    assert_eq!(copy.read_file("audio/sound.wav").unwrap(), [0, 255, 12]);
    assert_eq!(copy.manifest.as_ref().unwrap().1.write(), expected);
    assert_eq!(copy.manifest_fields().unwrap().descriptions, metadata.descriptions);
    assert!(p.save_as_folder(&root.join("copy"), "renamed").is_err());
    assert!(!root.join("project/audio/sound.wav").exists());
    let package = p.package_files().unwrap();
    assert_eq!(package["manifest.xml"], expected.as_bytes());
    assert!(package.contains_key(&p.behaviors[0].source));
    assert!(!package.contains_key("demo.pml"));
    assert_eq!(package["audio/sound.wav"], [0, 255, 12]);
    assert!(p.pending.contains_key("audio/sound.wav"));
    assert_eq!(manifest_path, "manifest.xml");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn structural_undo_keeps_file_inverses_after_save_and_renames_resources() {
    let (root, mut p, mut h) = setup("undo");
    fs::write(root.join("note.txt"), "before").unwrap();
    p.import_files(&mut h, &[root.join("note.txt")], "", false, None).unwrap();
    p.save().unwrap(); h.mark_saved();
    edit::change_project(&mut p, &mut h, "Metadata", |p| {
        let mut m = p.manifest_fields().unwrap(); m.version = "2.0.0".into(); p.set_manifest_fields(&m)
    }).unwrap();
    edit::set_files(&mut p, &mut h, "Text", vec![("note.txt".into(), b"after".to_vec())]).unwrap();
    p.save().unwrap(); h.mark_saved();
    edit::undo(&mut p, &mut h).unwrap();
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.read_file("note.txt").unwrap(), b"before");
    p.save().unwrap();
    assert_eq!(fs::read(root.join("project/note.txt")).unwrap(), b"before");
    let d = &mut p.behaviors[0].doc;
    let b = view::root_box(d);
    edit::insert_raw(d, b, "<Parameter id=\"12\" name=\"File\" content_type=\"4\" value=\"/note.txt\" default_value=\"note.txt\" />").unwrap();
    edit::insert_raw(d, b, "<bitmap><![CDATA[note.txt]]></bitmap>").unwrap();
    edit::change_project(&mut p, &mut h, "Rename", |p| p.rename_resource("note.txt", "nested/new.txt")).unwrap();
    assert!(p.files.iter().any(|f| f.path == "nested/new.txt"));
    let renamed = view::box_view(&p.behaviors[0].doc, b);
    assert_eq!(renamed.params[0].value, "/nested/new.txt");
    assert_eq!(renamed.params[0].default_value, "nested/new.txt");
    assert!(renamed.bitmaps.contains(&"nested/new.txt".to_string()));
    p.save().unwrap();
    edit::undo(&mut p, &mut h).unwrap();
    assert!(p.files.iter().any(|f| f.path == "note.txt"));
    edit::redo(&mut p, &mut h).unwrap();
    p.save().unwrap();
    assert_eq!(fs::read(root.join("project/nested/new.txt")).unwrap(), b"before");
    for path in ["../outside", "./../outside", "/root", "C:/outside", "x//bad", "x/../bad"] { assert!(project::relative(path).is_err(), "{path}"); }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn comment_and_bitmap_edits_roundtrip_and_undo_atomically() {
    let (root, mut p, mut h) = setup("box");
    let id = p.behaviors[0].id.clone();
    let b = view::root_box(&p.behaviors[0].doc);
    edit::set_raw(&mut p.behaviors[0].doc, b, "plugin", "textedit_plugin");
    let original = p.behaviors[0].doc.write();
    edit::apply(&mut p, &mut h, &id, "Content", vec![Command::SetBoxContent {r#box: b, bitmaps: Some(vec!["icons/a&b.png".into()]), plugin_text: Some("A < B & C\nSecond line".into())}]).unwrap();
    let v = view::box_view(&p.behaviors[0].doc, b);
    assert_eq!(v.plugin_text.as_deref(), Some("A < B & C\nSecond line"));
    assert_eq!(v.bitmaps, ["icons/a&b.png"]);
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.behaviors[0].doc.write(), original);
    edit::redo(&mut p, &mut h).unwrap(); p.save().unwrap();
    let p = Project::open(std::path::Path::new(&p.source)).unwrap();
    assert_eq!(view::box_view(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc)).plugin_text, v.plugin_text);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn library_folders_sort_and_rename_without_overwrites_or_escape() {
    let (root, _, _) = setup("library");
    let lib = root.join("library"); library::create(&lib, "User").unwrap();
    library::manage(&lib, "", "create", "zeta").unwrap();
    library::manage(&lib, "", "create", "alpha").unwrap();
    library::manage(&lib, "", "sort", "").unwrap();
    let xml = fs::read_to_string(lib.join("xalinfo")).unwrap();
    assert!(xml.find("alpha").unwrap() < xml.find("zeta").unwrap());
    assert!(library::manage(&lib, "zeta", "rename", "alpha").is_err());
    assert!(library::manage(&lib, "", "create", "../bad").is_err());
    library::manage(&lib, "zeta", "rename", "beta").unwrap();
    assert!(lib.join("beta").is_dir());
    assert!(!lib.join("zeta").exists());
    assert!(fs::read_to_string(lib.join("xalinfo")).unwrap().contains("beta"));
    fs::write(lib.join("beta/sound.wav"), [0, 255, 42]).unwrap();
    fs::create_dir(lib.join("beta/empty")).unwrap();
    library::copy_folder(&lib.join("beta"), &lib, "alpha", "Copied").unwrap();
    assert_eq!(fs::read(lib.join("alpha/Copied/sound.wav")).unwrap(), [0, 255, 42]);
    assert!(lib.join("alpha/Copied/empty").is_dir());
    assert!(library::copy_folder(&lib.join("beta"), &lib, "alpha", "Copied").is_err());
    assert!(library::copy_folder(&lib.join("beta"), &lib, "beta", "recursive").is_err());
    #[cfg(unix)] {
        std::os::unix::fs::symlink(lib.join("alpha"), lib.join("linked")).unwrap();
        assert!(library::manage(&lib, "linked", "rename", "bad").is_err());
        assert!(library::copy_folder(&lib.join("beta"), &lib, "linked", "bad").is_err());
        std::os::unix::fs::symlink(root.join("project"), lib.join("beta/link")).unwrap();
        assert!(library::copy_folder(&lib.join("beta"), &lib, "alpha", "Unsafe").is_err());
        assert!(!lib.join("alpha/Unsafe").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tangent_coupling_and_overflow_are_atomic() {
    let (root, mut p, mut h) = setup("tangent");
    let id = p.behaviors[0].id.clone();
    let d = &mut p.behaviors[0].doc;
    let b = view::root_box(d);
    let t = d.kid(b, "Timeline").unwrap();
    let a = edit::insert_raw(d, t, "<ActuatorList><ActuatorCurve actuator=\"HeadYaw\" unit=\"0\"><Key frame=\"10\" value=\"3\" smooth=\"1\" symmetrical=\"1\"><Tangent side=\"left\" interpType=\"bezier\" abscissaParam=\"-2\" ordinateParam=\"-1\"/><Tangent side=\"right\" interpType=\"bezier\" abscissaParam=\"2\" ordinateParam=\"1\"/></Key></ActuatorCurve></ActuatorList>").unwrap();
    let c = d.kid(a, "ActuatorCurve").unwrap(); let k = d.kid(c, "Key").unwrap();
    let tangents: Vec<_> = d.kids(k, "Tangent").collect();
    edit::apply(&mut p, &mut h, &id, "Tangent", vec![Command::MoveTangent {tangent: tangents[0], abscissa: -4.0, ordinate: -2.0}]).unwrap();
    let d = &p.behaviors[0].doc;
    assert_eq!(d.attr_num(tangents[1], "abscissaParam", 0.0), 4.0);
    assert_eq!(d.attr_num(tangents[1], "ordinateParam", 0.0), 2.0);
    edit::set_raw(&mut p.behaviors[0].doc, k, "symmetrical", "0");
    edit::apply(&mut p, &mut h, &id, "Smooth tangent", vec![Command::MoveTangent {tangent: tangents[0], abscissa: -2.0, ordinate: -3.0}]).unwrap();
    let d = &p.behaviors[0].doc;
    assert_eq!(d.attr_num(tangents[1], "abscissaParam", 0.0), 4.0);
    assert_eq!(d.attr_num(tangents[1], "ordinateParam", 0.0), 6.0);
    let saved = d.write();
    assert!(edit::apply(&mut p, &mut h, &id, "Overflow", vec![Command::SetCurveValues {keys: vec![k], scale: f64::MAX, offset: 0.0}]).is_err());
    assert_eq!(p.behaviors[0].doc.write(), saved);
    assert!(edit::apply(&mut p, &mut h, &id, "Bad direction", vec![Command::MoveTangent {tangent: tangents[0], abscissa: 2.0, ordinate: 1.0}]).is_err());
    assert_eq!(p.behaviors[0].doc.write(), saved);
    fs::remove_dir_all(root).unwrap();
}

/// Behavior properties and requirements are written in the package manifest's layout.
#[test]
fn behavior_properties_and_requirements_follow_manifest_layout() {
    let (root, mut p, _) = setup("props");
    let mut m = p.manifest_fields().unwrap();
    let b = &mut m.behaviors.as_mut().unwrap()[0];
    assert_eq!((b.path.as_str(), b.nature.as_str()), ("behavior_1", "interactive"));
    b.user_requestable = true;
    b.names = vec![("fr_FR".into(), "Salut".into()), ("en_US".into(), "Hello".into())];
    b.tags = vec![("en_US".into(), "greeting".into()), ("en_US".into(), "fun".into())];
    b.trigger_sentences = vec![("en_US".into(), "say hello".into())];
    b.conditions = vec![("launchTrigger".into(), "('Launchpad/Hour' > 8) && ('Launchpad/Hour' < 20)".into())];
    b.permissions = vec!["canStandUp".into()];
    m.requirements = Some(project::Requirements { naoqi: vec![("2.8".into(), "".into())], robots: vec![project::RobotRequirement { model: "NAO".into(), ..Default::default() }] });
    p.set_manifest_fields(&m).unwrap();
    let xml = p.manifest.as_ref().unwrap().1.write();
    let flat: String = xml.split('\n').map(str::trim).collect();
    assert!(flat.contains(concat!(
        r#"<behaviorContent path="behavior_1"><userRequestable/><nature>interactive</nature>"#,
        r#"<names><name lang="en_US">Hello</name><name lang="fr_FR">Salut</name></names>"#,
        r#"<tags><tag lang="en_US">greeting</tag><tag lang="en_US">fun</tag></tags>"#,
        r#"<triggerSentences><sentence lang="en_US">say hello</sentence></triggerSentences>"#,
        r#"<autonomous><condition purpose="launchTrigger"><![CDATA[('Launchpad/Hour' > 8) && ('Launchpad/Hour' < 20)]]></condition></autonomous>"#,
        r#"<permissions><canStandUp/></permissions></behaviorContent>"#,
    )), "{xml}");
    assert!(flat.contains(r#"</contents><requirements><naoqiRequirement minVersion="2.8"/><robotRequirement model="NAO"/></requirements>"#), "{xml}");
    let back = p.manifest_fields().unwrap();
    assert_eq!(back.behaviors.as_ref().unwrap()[0].names[0], ("en_US".to_string(), "Hello".to_string()), "names sorted by language");
    assert_eq!(back.behaviors.as_ref().unwrap()[0].conditions, m.behaviors.as_ref().unwrap()[0].conditions);
    assert_eq!(back.requirements, m.requirements);
    // Saving without behaviors or requirements leaves them as they are.
    p.set_manifest_fields(&project::Manifest { behaviors: None, requirements: None, ..back.clone() }).unwrap();
    assert_eq!(p.manifest.as_ref().unwrap().1.write(), xml);
    for bad in ["flying", ""] {
        let mut m2 = back.clone();
        m2.behaviors.as_mut().unwrap()[0].nature = bad.into();
        assert!(p.set_manifest_fields(&m2).is_err());
    }
    let mut m2 = back.clone();
    m2.behaviors.as_mut().unwrap()[0].permissions = vec!["canFly".into()];
    assert!(p.set_manifest_fields(&m2).unwrap_err().contains("canRunInSleep"));
    fs::remove_dir_all(root).unwrap();
}

/// Saving the Properties dialog sends every behavior back: untouched entries keep their XML even
/// when they would not validate (no `<nature>`), and folders with `'` or `&` are found.
#[test]
fn manifest_round_trip_keeps_untouched_entries_and_finds_escaped_paths() {
    let (root, mut p, _) = setup("props-escaped");
    p.add_behavior("Tom's & dance", choren_core::edit::NewBoxKind::Diagram).unwrap();
    {
        let (_, m) = p.manifest.as_mut().unwrap();
        let contents = m.kid(m.root().unwrap(), "contents").unwrap();
        edit::insert_raw(m, contents, r#"<behaviorContent path="handmade"><permissions/></behaviorContent>"#).unwrap();
    }
    let contents = |p: &project::Project| { let m = &p.manifest.as_ref().unwrap().1; m.subtree_string(m.kid(m.root().unwrap(), "contents").unwrap()) };
    let before = contents(&p);
    let m = p.manifest_fields().unwrap();
    p.set_manifest_fields(&m).unwrap();
    assert_eq!(contents(&p), before, "unchanged behaviors keep their XML");
    let mut m2 = m.clone();
    let tom = m2.behaviors.as_mut().unwrap().iter_mut().find(|b| b.path.starts_with("Tom")).expect("the escaped path reads back");
    tom.user_requestable = true;
    p.set_manifest_fields(&m2).unwrap();
    let after = p.manifest_fields().unwrap();
    assert!(after.behaviors.unwrap().iter().any(|b| b.path.starts_with("Tom") && b.user_requestable));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rename_sets_the_package_name_and_undoes() {
    let (root, mut p, mut h) = setup("rename");
    edit::change_project(&mut p, &mut h, "Rename project", |p| p.rename(" Show & Tell ")).unwrap();
    assert_eq!(p.name, "Show & Tell");
    assert!(p.rename("  ").is_err());
    p.save().unwrap();
    let reopened = Project::open(&root.join("project/demo.pml")).unwrap();
    assert_eq!(reopened.name, "Show & Tell");
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.name, "demo");
    fs::remove_dir_all(root).unwrap();
}
