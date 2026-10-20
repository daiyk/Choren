//! One scenario per authoring feature: ConvertToBox, motion curves, port editing, translations,
//! dialogs, and CRG export / save as folder. Each ends by undoing back to the original bytes.

use choren_core::edit::{self, Command, History, Interp, MirrorMode, NewBoxKind, PortKind, SimplifyMode};
use choren_core::project::Project;
use choren_core::xml::Doc;
use choren_core::{motion, rules, translations, view};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("choren-authoring-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn share() -> PathBuf {
    std::env::var_os("CHOREGRAPHE_SHARE").map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
    })
}

fn inner(d: &Doc, b: usize) -> usize {
    let t = d.kid(b, "Timeline").unwrap();
    d.kid(d.kid(d.kid(t, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap()
}

fn new_project(name: &str) -> (Project, History, String) {
    let p = Project::create(&tmp(name), name).unwrap();
    let id = p.behaviors[0].id.clone();
    (p, History::fresh(), id)
}

fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, Option<String>> {
    pairs.iter().map(|(k, v)| (k.to_string(), Some(v.to_string()))).collect()
}

/// Every link in every diagram resolves to two ports.
fn links_resolve(d: &Doc, diagram: usize) -> bool {
    d.kids(diagram, "Link").all(|l| {
        let g = |k| d.attr_num(l, k, -1.0) as i64;
        rules::find_port(d, diagram, g("outputowner"), g("indexofoutput")).is_some() && rules::find_port(d, diagram, g("inputowner"), g("indexofinput")).is_some()
    }) && d.kids(diagram, "Box").all(|b| d.kid(b, "Timeline").is_none_or(|_| links_resolve(d, inner(d, b))))
}

#[test]
fn convert_to_box_wraps_selection_and_rewires_links() {
    let (mut p, mut h, bid) = new_project("convert");
    let original = p.behaviors[0].doc.write();
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let mut ids = vec![];
    for (i, name) in ["Say hi", "Wave", "End"].iter().enumerate() {
        let s = edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: name.to_string(), x: 100.0 * i as f64, y: 50.0, dialog_file: None }]).unwrap();
        ids.push(s.created[0]);
    }
    let link = |o: (i64, i64), i: (i64, i64)| Command::AddLink { diagram, out_owner: o.0, out_port: o.1, in_owner: i.0, in_port: i.1 };
    // rail onStart → Say hi → Wave → End → rail onStopped
    edit::apply(&mut p, &mut h, &bid, "Links", vec![link((0, 2), (1, 2)), link((1, 4), (2, 2)), link((2, 4), (3, 2)), link((3, 4), (0, 4))]).unwrap();
    let before = p.behaviors[0].doc.write();
    assert!(edit::apply(&mut p, &mut h, &bid, "Convert", vec![Command::ConvertToBox { diagram, boxes: vec![], name: "x".into() }]).is_err());
    assert_eq!(p.behaviors[0].doc.write(), before);

    edit::apply(&mut p, &mut h, &bid, "Convert selection to box", vec![Command::ConvertToBox { diagram, boxes: ids[..2].to_vec(), name: "Greeting".into() }]).unwrap();
    let d = &p.behaviors[0].doc;
    let outer = view::diagram(d, diagram);
    assert_eq!(outer.boxes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["End", "Greeting"]);
    let g = outer.boxes.iter().find(|b| b.name == "Greeting").unwrap();
    assert_eq!((g.x, g.y), (0.0, 50.0));
    let inputs: Vec<_> = g.inputs.iter().map(|p| (p.name.as_str(), p.id, p.nature)).collect();
    assert_eq!(inputs, [("onLoad", 1, 0), ("onStart", 2, 2), ("onStop", 3, 3), ("Say_hi_onStart", 5, 2)]);
    assert_eq!(g.outputs.iter().map(|p| (p.name.as_str(), p.id)).collect::<Vec<_>>(), [("onStopped", 4), ("Wave_onStopped", 6)]);
    assert!(g.inputs[3].tooltip.starts_with("This input has been automatically generated\nby converting"));
    let mut links: Vec<_> = outer.links.iter().map(|l| (l.out_owner, l.out_port, l.in_owner, l.in_port)).collect();
    links.sort();
    let mut want = [(0, 2, g.id, 5), (3, 4, 0, 4), (g.id, 6, 3, 2)];
    want.sort();
    assert_eq!(links, want);
    let inside = &g.timeline.as_ref().unwrap().layers[0].keyframes[0].diagram.as_ref().unwrap();
    assert_eq!(inside.boxes.iter().map(|b| (b.name.as_str(), b.id)).collect::<Vec<_>>(), [("Say hi", 1), ("Wave", 2)]);
    let mut links: Vec<_> = inside.links.iter().map(|l| (l.out_owner, l.out_port, l.in_owner, l.in_port)).collect();
    links.sort();
    assert_eq!(links, [(0, 5, 1, 2), (1, 4, 2, 2), (2, 4, 0, 6)]);
    assert!(links_resolve(d, diagram));
    assert_eq!(Doc::parse(&d.write()).unwrap().write(), d.write());

    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.behaviors[0].doc.write(), before);
    while edit::undo(&mut p, &mut h).is_some() {}
    assert_eq!(p.behaviors[0].doc.write(), original);
}

#[test]
fn motion_keys_interpolation_mirror_and_qianim() {
    let (mut p, mut h, bid) = new_project("motion");
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let b = edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind: NewBoxKind::Timeline, name: "Dance".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let original = p.behaviors[0].doc.write();
    let t = p.behaviors[0].doc.kid(b, "Timeline").unwrap();
    let robot = choren_robot::Robot::load("nao6").unwrap();
    // robot angles in radians, descriptor order; the hands are a ratio
    let pose = |pairs: &[(&str, f64)]| {
        let mut a = vec![0.0; robot.joints.len()];
        for (j, v) in pairs {
            a[robot.joint_index(j).unwrap()] = if j.ends_with("Hand") { *v } else { v.to_radians() };
        }
        a
    };
    let store = |frame, group: &str, pairs: &[(&str, f64)]| Command::StoreKeys { timeline: t, frame, group: group.into(), angles: pose(pairs) };
    edit::apply(&mut p, &mut h, &bid, "Store joints in keyframe", vec![store(30, "Head", &[("HeadYaw", 10.0), ("HeadPitch", 10.0)]), store(10, "HeadYaw", &[("HeadYaw", -5.5)]), store(10, "LArm", &[("LShoulderRoll", 20.0), ("LHand", 0.6)])]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Store", vec![store(20, "HeadYaw", &[("HeadYaw", 3.0)]), store(30, "HeadYaw", &[("HeadYaw", 12.25)])]).unwrap();
    assert!(edit::apply(&mut p, &mut h, &bid, "Store", vec![store(5, "Tail", &[])]).unwrap_err().contains("Unknown joint"));
    assert!(edit::apply(&mut p, &mut h, &bid, "Store", vec![Command::StoreKeys { timeline: t, frame: 5, group: "Head".into(), angles: vec![] }]).is_err());

    let curve = |p: &Project, name: &str| view::box_view(&p.behaviors[0].doc, b).timeline.unwrap().curves.into_iter().find(|c| c.actuator == name).unwrap();
    let yaw = curve(&p, "HeadYaw");
    assert_eq!((yaw.frames.clone(), yaw.values.clone(), yaw.unit), (vec![10.0, 20.0, 30.0], vec![-5.5, 3.0, 12.25], 0));
    assert_eq!((curve(&p, "LHand").unit, curve(&p, "LHand").values.clone()), (1, vec![0.6]));
    assert_eq!(curve(&p, "LElbowRoll").values, [0.0], "the whole arm is stored");
    assert_eq!(curve(&p, "HeadPitch").frames, [30.0]);

    // move, collide, interpolation and tangent rules
    let k20 = yaw.keys[1].node;
    assert!(edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::MoveKeys { keys: vec![k20], delta: 10 }]).unwrap_err().contains("already a key"));
    edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::MoveKeys { keys: vec![k20], delta: -5 }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").frames, [10.0, 15.0, 30.0]);
    edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::MoveKeys { keys: vec![k20], delta: -10 }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").frames, [5.0, 10.0, 30.0], "keys stay sorted");
    edit::apply(&mut p, &mut h, &bid, "Interp", vec![Command::SetInterpolation { keys: vec![k20], interp: Interp::Linear }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").keys[0].right.as_ref().unwrap().interp, "linear");
    let k10 = curve(&p, "HeadYaw").keys[1].node;
    edit::apply(&mut p, &mut h, &bid, "Interp", vec![Command::SetInterpolation { keys: vec![k10], interp: Interp::Bezier }]).unwrap();
    let key = &curve(&p, "HeadYaw").keys[1];
    let (l, r) = (key.left.clone().unwrap(), key.right.clone().unwrap());
    assert_eq!((l.interp.as_str(), l.abscissa, r.abscissa), ("bezier", -1.66667, 6.66667));
    assert!(edit::apply(&mut p, &mut h, &bid, "Tangent", vec![Command::SetAttributes { node: l.node, values: attrs(&[("abscissaParam", "2")]) }]).unwrap_err().contains("non-positive"));
    edit::apply(&mut p, &mut h, &bid, "Tangent", vec![Command::SetAttributes { node: r.node, values: attrs(&[("abscissaParam", "4"), ("ordinateParam", "1.5")]) }]).unwrap();
    assert!(edit::apply(&mut p, &mut h, &bid, "Frame", vec![Command::SetAttributes { node: k10, values: attrs(&[("frame", "11")]) }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Smooth", vec![Command::SetAttributes { node: k10, values: attrs(&[("smooth", "1"), ("symmetrical", "1")]) }]).unwrap();

    // .qianim round trip keeps frames, values, interpolation kinds and tangents
    let d = &p.behaviors[0].doc;
    let qianim = motion::to_qianim(d, t).unwrap();
    assert!(qianim.contains("<Animation typeVersion=\"2.0\" xmlns:editor=\"http://www.aldebaran.com/animation/editor\">"));
    assert!(qianim.contains("unit=\"dimensionless\"") && qianim.contains("editor:interpType=\"bezier_auto\"") && qianim.contains("editor:smooth=\"true\""));
    let list = motion::from_qianim(&qianim, 25.0).unwrap();
    let before_import = view::box_view(d, b).timeline.unwrap().curves;
    let old_list = d.kid(t, "ActuatorList").unwrap();
    edit::apply(&mut p, &mut h, &bid, "Import animation", vec![Command::Remove { nodes: vec![old_list] }, Command::InsertXml { parent: t, xml: list }]).unwrap();
    let after_import = view::box_view(&p.behaviors[0].doc, b).timeline.unwrap().curves;
    assert_eq!(after_import.len(), before_import.len());
    for (a, c) in before_import.iter().zip(&after_import) {
        assert_eq!((&a.actuator, &a.frames, &a.values, a.unit), (&c.actuator, &c.frames, &c.values, c.unit));
        for (x, y) in a.keys.iter().zip(&c.keys) {
            assert_eq!((x.smooth, x.left.as_ref().map(|t| &t.interp), x.right.as_ref().map(|t| (&t.interp, t.abscissa, t.ordinate))), (y.smooth, y.left.as_ref().map(|t| &t.interp), y.right.as_ref().map(|t| (&t.interp, t.abscissa, t.ordinate))));
        }
    }
    edit::undo(&mut p, &mut h).unwrap();
    assert!(motion::from_qianim("<Animation typeVersion=\"1.0\" />", 25.0).is_err());

    // Mirror left → right negates roll/yaw; Flip twice restores the bytes
    edit::apply(&mut p, &mut h, &bid, "Mirror", vec![Command::Mirror { timeline: t, mode: MirrorMode::LeftToRight, frames: None }]).unwrap();
    assert_eq!(curve(&p, "RShoulderRoll").values, [-20.0]);
    assert_eq!(curve(&p, "RHand").values, [0.6]);
    assert_eq!(curve(&p, "LShoulderRoll").values, [20.0]);
    let mirrored = p.behaviors[0].doc.write();
    edit::apply(&mut p, &mut h, &bid, "Flip", vec![Command::Mirror { timeline: t, mode: MirrorMode::Flip, frames: None }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").values, [-3.0, 5.5, -12.25]);
    assert_eq!(curve(&p, "HeadPitch").values, [10.0]);
    edit::apply(&mut p, &mut h, &bid, "Flip", vec![Command::Mirror { timeline: t, mode: MirrorMode::Flip, frames: None }]).unwrap();
    assert_eq!(p.behaviors[0].doc.write(), mirrored);

    edit::apply(&mut p, &mut h, &bid, "Remove joints", vec![Command::RemoveKeys { timeline: t, frame: 30, group: "Head".into(), to: None }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").frames, [5.0, 10.0]);
    assert!(curve(&p, "HeadPitch").frames.is_empty());

    while edit::undo(&mut p, &mut h).is_some() {}
    edit::redo(&mut p, &mut h).unwrap();
    assert_eq!(p.behaviors[0].doc.write(), original);
}

#[test]
fn port_editing_nature_memory_key_and_parameter_ranges() {
    let (mut p, mut h, bid) = new_project("ports");
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let b = edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Box".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let original = p.behaviors[0].doc.write();
    let port = |p: &Project, name: &str| rules::ports(&p.behaviors[0].doc, b).find(|&n| p.behaviors[0].doc.attr(n, "name").as_deref() == Some(name)).unwrap();
    let values = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();

    // an ALMemory event input is named after its key, which may contain '/'
    edit::apply(&mut p, &mut h, &bid, "Add event from ALMemory", vec![Command::AddPort { r#box: b, kind: PortKind::Input, name: "Device/Touch".into(), values: values(&[("type", "0"), ("nature", "4"), ("stm_value_name", "Device/Touch"), ("inner", "1")]) }]).unwrap();
    assert!(p.behaviors[0].doc.write().contains("<Input name=\"Device/Touch\" type=\"0\" type_size=\"1\" nature=\"4\" stm_value_name=\"Device/Touch\" inner=\"1\""));
    assert!(edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::AddPort { r#box: b, kind: PortKind::Input, name: "a/b".into(), values: BTreeMap::new() }]).is_err());

    // nature selector: an input becomes an ALMemory event only with a key; the key lands after nature
    let start = port(&p, "onStart");
    assert!(edit::apply(&mut p, &mut h, &bid, "Edit input", vec![Command::SetAttributes { node: start, values: attrs(&[("nature", "4")]) }]).unwrap_err().contains("memory key"));
    edit::apply(&mut p, &mut h, &bid, "Edit input", vec![Command::SetAttributes { node: start, values: attrs(&[("nature", "4"), ("stm_value_name", "MiddleTactilTouched")]) }]).unwrap();
    assert!(p.behaviors[0].doc.write().contains("<Input name=\"onStart\" type=\"1\" type_size=\"1\" nature=\"4\" stm_value_name=\"MiddleTactilTouched\" inner=\"0\""));
    assert_eq!(view::box_view(&p.behaviors[0].doc, b).inputs[1].stm.as_deref(), Some("MiddleTactilTouched"));
    let stopped = port(&p, "onStopped");
    edit::apply(&mut p, &mut h, &bid, "Edit output", vec![Command::SetAttributes { node: stopped, values: attrs(&[("nature", "2")]) }]).unwrap();

    // parameter range and choices
    edit::apply(&mut p, &mut h, &bid, "Add parameter", vec![Command::AddPort { r#box: b, kind: PortKind::Parameter, name: "Speed".into(), values: values(&[("content_type", "1"), ("value", "5"), ("default_value", "5")]) }]).unwrap();
    let speed = port(&p, "Speed");
    assert!(edit::apply(&mut p, &mut h, &bid, "Edit parameter", vec![Command::SetAttributes { node: speed, values: attrs(&[("min", "10"), ("max", "1")]) }]).unwrap_err().contains("greater than minimum"));
    edit::apply(&mut p, &mut h, &bid, "Edit parameter", vec![Command::SetAttributes { node: speed, values: attrs(&[("min", "0"), ("max", "100")]) }]).unwrap();
    assert!(p.behaviors[0].doc.write().contains("value=\"5\" default_value=\"5\" min=\"0\" max=\"100\" tooltip=\"\""));
    edit::apply(&mut p, &mut h, &bid, "Add parameter", vec![Command::AddPort { r#box: b, kind: PortKind::Parameter, name: "Side".into(), values: values(&[("content_type", "3"), ("value", "Left")]) }]).unwrap();
    let side = port(&p, "Side");
    edit::apply(&mut p, &mut h, &bid, "Edit parameter", vec![
        Command::SetAttributes { node: side, values: attrs(&[("custom_choice", "1")]) },
        Command::InsertXml { parent: side, xml: "<Choice value=\"Left\" />".into() },
        Command::InsertXml { parent: side, xml: "<Choice value=\"Right\" />".into() },
    ]).unwrap();
    let pv = view::box_view(&p.behaviors[0].doc, b).params.into_iter().find(|x| x.name == "Side").unwrap();
    assert!(pv.custom_choice);
    assert_eq!(pv.choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["Left", "Right"]);
    edit::apply(&mut p, &mut h, &bid, "Edit parameter", vec![Command::Remove { nodes: vec![pv.choices[0].node] }]).unwrap();
    assert!(Doc::parse(&p.behaviors[0].doc.write()).is_ok());

    while edit::undo(&mut p, &mut h).is_some() {}
    edit::redo(&mut p, &mut h).unwrap();
    assert_eq!(p.behaviors[0].doc.write(), original);
}

#[test]
fn translations_compile_like_lrelease() {
    // lrelease.qm is Choregraphe 2.8.8's `bin/lrelease lrelease.ts`.
    let qm = translations::compile(include_bytes!("../../../app/test/fixtures/lrelease.ts")).unwrap();
    assert!(qm == include_bytes!("../../../app/test/fixtures/lrelease.qm"));
}

#[test]
fn translations_gather_localized_strings_and_merge() {
    let (mut p, mut h, bid) = new_project("tr");
    let d = &p.behaviors[0].doc;
    let diagram = inner(d, view::root_box(d));
    let b = edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Say".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let text = BTreeMap::from([("content_type".to_string(), "5".to_string()), ("value".to_string(), "Hello & \"welcome\"".to_string())]);
    edit::apply(&mut p, &mut h, &bid, "Add parameter", vec![Command::AddPort { r#box: b, kind: PortKind::Parameter, name: "Text".into(), values: text }]).unwrap();
    let (_, md) = p.manifest.as_mut().unwrap();
    let langs = md.kid(md.root().unwrap(), "supportedLanguages").unwrap();
    edit::insert_raw(md, langs, "<language>fr_FR</language>").unwrap();

    translations::update(&mut p, &mut h).unwrap();
    edit::undo(&mut p, &mut h).unwrap();
    assert!(!p.pml.as_ref().unwrap().write().contains("<Translation "), "undo drops the entries");
    edit::redo(&mut p, &mut h).unwrap();
    let en = String::from_utf8(p.read_file("translations/translation_en_US.ts").unwrap()).unwrap();
    assert_eq!(en, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE TS>\n<TS version=\"2.1\" language=\"en_US\">\n    <context>\n        <name>behavior_1/behavior.xar:/Say</name>\n        <message>\n            <location filename=\"behavior_1/behavior.xar\" line=\"0\"/>\n            <source>Hello &amp; &quot;welcome&quot;</source>\n            <comment>Text</comment>\n            <translation type=\"unfinished\">Hello &amp; &quot;welcome&quot;</translation>\n        </message>\n    </context>\n</TS>\n");
    let fr = String::from_utf8(p.read_file("translations/translation_fr_FR.ts").unwrap()).unwrap();
    assert!(fr.contains("<translation type=\"unfinished\"></translation>"));
    assert!(p.pml.as_ref().unwrap().write().contains("<Translations auto-fill=\"en_US\">"));

    // a translated string survives; a removed one becomes obsolete; undo unstages the file
    let fr = fr.replace("<translation type=\"unfinished\"></translation>", "<translation>Bonjour</translation>");
    edit::set_files(&mut p, &mut h, "Edit", vec![("translations/translation_fr_FR.ts".into(), fr.into_bytes())]).unwrap();
    let param = rules::ports(&p.behaviors[0].doc, b).last().unwrap();
    edit::apply(&mut p, &mut h, &bid, "Set", vec![Command::SetAttributes { node: param, values: attrs(&[("value", "Bye")]) }]).unwrap();
    translations::update(&mut p, &mut h).unwrap();
    let fr = String::from_utf8(p.read_file("translations/translation_fr_FR.ts").unwrap()).unwrap();
    assert!(fr.contains("<source>Hello &amp; &quot;welcome&quot;</source>\n            <comment>Text</comment>\n            <translation type=\"obsolete\">Bonjour</translation>"));
    assert!(fr.contains("<source>Bye</source>"));
    p.save().unwrap();
    h.mark_saved();
    let saved = std::fs::read(Path::new(&p.source).parent().unwrap().join("translations/translation_fr_FR.ts")).unwrap();
    edit::undo(&mut p, &mut h).unwrap();
    assert_ne!(p.read_file("translations/translation_fr_FR.ts").unwrap(), saved, "undo after save stages the previous content");
    edit::redo(&mut p, &mut h).unwrap();
    assert!(!p.state(&h, vec![]).dirty);

    // packages carry the supported languages' compiled `.qm` instead of the `.ts`
    let package = p.package_files().unwrap();
    assert_eq!(package["translations/translation_fr_FR.qm"], translations::compile(&p.read_file("translations/translation_fr_FR.ts").unwrap()).unwrap());
    assert!(package.contains_key("translations/translation_en_US.qm"));
    assert!(!package.keys().any(|k| k.ends_with(".ts")));

    // the reference project's file is reproduced byte for byte
    let reference = std::env::var_os("REF_PROJECT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads/Robot_activities_demos/Robot_activities_demos.pml"));
    if reference.exists() {
        let r = Project::open(&reference).unwrap();
        let old = r.read_file("translations/translation_en_US.ts").unwrap();
        let again = translations::render("en_US", true, &translations::gather(&r), Some(&old)).unwrap();
        if again.as_bytes() != old {
            std::fs::write(std::env::temp_dir().join("choren-ts-mismatch.ts"), &again).unwrap();
        }
        assert!(again.as_bytes() == old, "see choren-ts-mismatch.ts in the temp dir");
    }
}

#[test]
fn dialogs_create_topics_and_edit_them_undoably() {
    let (mut p, mut h, _) = new_project("dlg");
    assert!(p.add_dialog(&mut h, "bad name", &["en_US".into()]).is_err());
    let (pml, files) = (p.pml.as_ref().unwrap().write(), p.files.len());
    p.add_dialog(&mut h, "greet", &["en_US".into(), "fr_FR".into()]).unwrap();
    // undoing the creation drops the PML entries and file rows too; redo brings them back
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!((p.pml.as_ref().unwrap().write(), p.files.len()), (pml, files));
    assert!(p.read_file("greet/greet.dlg").is_none());
    edit::redo(&mut p, &mut h).unwrap();
    assert_eq!(p.files.len(), files + 3);
    assert_eq!(p.read_file("greet/greet.dlg").unwrap(), b"multilanguage:greet\n  enu:greet_enu.top\n  frf:greet_frf.top\n");
    assert_eq!(p.read_file("greet/greet_frf.top").unwrap(), b"topic: ~greet()\nlanguage: frf\n\n");
    assert!(p.add_dialog(&mut h, "greet", &["en_US".into()]).unwrap_err().contains("already exists"));
    let topic = "topic: ~greet()\nlanguage: enu\n\nu:(hello) hi there\n";
    for i in 1..=topic.len() {
        edit::set_files(&mut p, &mut h, "Edit topic", vec![("greet/greet_enu.top".into(), topic[..i].as_bytes().to_vec())]).unwrap();
    }
    assert_eq!(h.state(vec![]).undo.as_deref(), Some("Edit topic"));
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(p.read_file("greet/greet_enu.top").unwrap(), b"topic: ~greet()\nlanguage: enu\n\n", "typing merges into one step");
    edit::redo(&mut p, &mut h).unwrap();
    p.save().unwrap();
    let pml = std::fs::read_to_string(&p.source).unwrap();
    assert!(pml.contains("<Dialog name=\"greet\" src=\"greet/greet.dlg\" />"));
    assert!(pml.contains("<Topic name=\"greet_enu\" src=\"greet/greet_enu.top\" topicName=\"greet\" language=\"en_US\" />"));
    let again = Project::open(Path::new(&p.source)).unwrap();
    assert_eq!(again.files.iter().filter(|f| f.kind == "topic" && f.missing == Some(false)).count(), 2);
    assert_eq!(again.read_file("greet/greet_enu.top").unwrap(), topic.as_bytes());
}

#[test]
fn crg_export_and_save_as_project_folder() {
    let (mut p, mut h, bid) = new_project("crgexp");
    p.stage("sounds/beep.ogg", Some(vec![7; 1000]));
    let crg = p.export_crg(&bid).unwrap();
    let files = choren_core::crg::files(&crg).unwrap();
    assert_eq!(files[0], ("behavior.xar".to_string(), p.behaviors[0].doc.write().into_bytes()));
    assert!(files.iter().any(|(n, b)| n == "sounds/beep.ogg" && b.len() == 1000));

    // the exported archive opens as a read-only .crg and saves as a project folder
    let out = tmp("crgout");
    std::fs::write(out.join("demo.crg"), &crg).unwrap();
    let mut imported = Project::open(&out.join("demo.crg")).unwrap();
    assert!(imported.save().unwrap_err().contains("Save as project folder"));
    let folder = out.join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    let pml = imported.save_as_folder(&folder, "demo").unwrap();
    let reopened = Project::open(&pml).unwrap();
    assert_eq!(reopened.behaviors[0].doc.write(), p.behaviors[0].doc.write());
    assert_eq!(reopened.behaviors[0].source, "behavior_1/behavior.xar");
    assert_eq!(reopened.read_file("sounds/beep.ogg").unwrap().len(), 1000);
    assert!(reopened.diagnostics.is_empty());
    assert!(imported.save_as_folder(&folder, "demo").is_err(), "never overwrites");
    let _ = &mut h;

    // a shipped archive round-trips through save-as-folder unchanged
    let shipped = share().join("projects/Aldebaran/display3d/fsr.crg");
    if shipped.exists() {
        let c = Project::open(&shipped).unwrap();
        let dir = tmp("fsr");
        let pml = c.save_as_folder(&dir, "fsr").unwrap();
        let again = Project::open(&pml).unwrap();
        assert_eq!(again.behaviors[0].doc.write(), c.behaviors[0].doc.write());
    }
}

#[cfg(unix)]
#[test]
fn save_permission_error_identifies_the_folder_and_keeps_edits_for_retry() {
    use std::os::unix::fs::PermissionsExt;
    let (mut p, mut h, bid) = new_project("save-permission");
    let root = p.root_dir().unwrap();
    let file = root.join(&p.behaviors[0].source);
    let folder = file.parent().unwrap();
    let original = std::fs::read(&file).unwrap();
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    edit::apply(&mut p, &mut h, &bid, "new box", vec![Command::NewBox { diagram, kind: NewBoxKind::Python, name: "Unsaved".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap();
    let edited = p.behaviors[0].doc.write();
    let permissions = std::fs::metadata(folder).unwrap().permissions();
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = p.save();
    // Restore before asserting so a failed test leaves no read-only temporary directory.
    std::fs::set_permissions(folder, permissions).unwrap();
    let error = result.expect_err("an unprivileged process cannot atomically save in a read-only folder");
    assert!(error.contains(file.to_str().unwrap()), "{error}");
    assert!(error.contains("containing folder"), "{error}");
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert_eq!(p.behaviors[0].doc.write(), edited);
    p.save().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dialog_box_port_sort_keyframes_range_and_curve_editing() {
    let (mut p, mut h, bid) = new_project("p1");
    let diagram = inner(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc));
    let original = p.behaviors[0].doc.write();
    let doc = |p: &Project| p.behaviors[0].doc.write();

    // Dialog box: dialogFile first, relative to the behavior folder (`behavior_1/`), then a new
    // file; paths outside the project or not .dlg are refused
    let dlg = Command::NewBox { diagram, kind: NewBoxKind::Dialog, name: "Chat".into(), x: 0.0, y: 0.0, dialog_file: Some("../greet/greet.dlg".into()) };
    let b = edit::apply(&mut p, &mut h, &bid, "Add", vec![dlg]).unwrap().created[0];
    let xml = doc(&p);
    let at = |s: &str| xml.find(s).unwrap();
    assert!(at("<Box name=\"Chat\" id=\"1\"") < at("<dialogFile>../greet/greet.dlg</dialogFile>") && at("<dialogFile>") < at("box-dialog.png"));
    assert_eq!(view::box_view(&p.behaviors[0].doc, b).dialog_file.as_deref(), Some("../greet/greet.dlg"));
    assert!(edit::apply(&mut p, &mut h, &bid, "Edit", vec![Command::SetDialogFile { r#box: b, file: "../../x.dlg".into() }]).is_err());
    assert!(edit::apply(&mut p, &mut h, &bid, "Edit", vec![Command::SetDialogFile { r#box: b, file: "a/b.top".into() }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Edit", vec![Command::SetDialogFile { r#box: b, file: "../bye/bye.dlg".into() }]).unwrap();
    assert_eq!(view::box_view(&p.behaviors[0].doc, b).dialog_file.as_deref(), Some("../bye/bye.dlg"));

    // SortIO: inputs reversed, ids kept; partial lists are refused
    let inputs: Vec<usize> = p.behaviors[0].doc.kids(b, "Input").collect();
    assert!(edit::apply(&mut p, &mut h, &bid, "Sort", vec![Command::SortPorts { r#box: b, ports: inputs[..2].to_vec() }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Sort", vec![Command::SortPorts { r#box: b, ports: inputs.iter().rev().copied().collect() }]).unwrap();
    let v = view::box_view(&p.behaviors[0].doc, b);
    assert_eq!(v.inputs.iter().map(|x| (x.name.as_str(), x.id)).collect::<Vec<_>>(), [("onStop", 3), ("onStart", 2), ("onLoad", 1)]);
    assert_eq!(v.outputs[0].name, "onStopped");
    assert!(Doc::parse(&doc(&p)).is_ok());

    // timeline: keyframes translate, copy/paste (names kept, taken indices shift), range and stretch
    let tb = edit::apply(&mut p, &mut h, &bid, "Add", vec![Command::NewBox { diagram, kind: NewBoxKind::Timeline, name: "Dance".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
    let t = p.behaviors[0].doc.kid(tb, "Timeline").unwrap();
    edit::apply(&mut p, &mut h, &bid, "Add layer", vec![Command::InsertXml { parent: t, xml: "<BehaviorLayer name=\"behavior_layer1\"><BehaviorKeyframe name=\"keyframe1\" index=\"1\"><Diagram /></BehaviorKeyframe></BehaviorLayer>".into() }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Set size", vec![Command::SetAttributes { node: t, values: attrs(&[("size", "300")]) }]).unwrap();
    let layer = p.behaviors[0].doc.kid(t, "BehaviorLayer").unwrap();
    edit::apply(&mut p, &mut h, &bid, "Add keyframe", vec![Command::InsertXml { parent: layer, xml: "<BehaviorKeyframe name=\"kf2\" index=\"50\"><Diagram /></BehaviorKeyframe>".into() }]).unwrap();
    let layers = |p: &Project| view::box_view(&p.behaviors[0].doc, tb).timeline.unwrap().layers;
    let kf = |p: &Project| layers(p)[0].keyframes.iter().map(|k| (k.name.clone(), k.index)).collect::<Vec<_>>();
    let k1 = layers(&p)[0].keyframes[0].node;
    assert!(edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::TranslateKeyframes { keyframes: vec![k1], delta: 49 }]).unwrap_err().contains("Index not available"));
    assert!(edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::TranslateKeyframes { keyframes: vec![k1], delta: -1 }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Move", vec![Command::TranslateKeyframes { keyframes: vec![k1], delta: 59 }]).unwrap();
    assert_eq!(kf(&p), [("kf2".into(), 50), ("keyframe1".into(), 60)]);
    let clip = edit::copy(&p.behaviors[0].doc, &[k1]);
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::PasteKeyframes { layer, xml: clip.clone(), index: 10 }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::PasteKeyframes { layer, xml: clip.clone(), index: 20 }]).unwrap();
    assert_eq!(kf(&p), [("keyframe1".into(), 10), ("keyframe1".into(), 20), ("kf2".into(), 50), ("keyframe1".into(), 60)]);
    edit::apply(&mut p, &mut h, &bid, "Paste", vec![Command::PasteKeyframes { layer, xml: clip, index: 50 }]).unwrap();
    assert_eq!(kf(&p)[2..4], [("kf2".into(), 50), ("keyframe1".into(), 51)], "a taken index moves to the next free one");
    edit::undo(&mut p, &mut h).unwrap();

    let range = |p: &Project| { let v = view::box_view(&p.behaviors[0].doc, tb).timeline.unwrap(); (v.start_frame, v.end_frame, v.size) };
    assert!(edit::apply(&mut p, &mut h, &bid, "Range", vec![Command::SetTimelineRange { timeline: t, start: 10, end: 5 }]).is_err());
    assert!(edit::apply(&mut p, &mut h, &bid, "FPS", vec![Command::SetAttributes { node: t, values: attrs(&[("fps", "0")]) }]).is_err());
    assert!(edit::apply(&mut p, &mut h, &bid, "Mode", vec![Command::SetAttributes { node: t, values: attrs(&[("resources_acquisition", "3")]) }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Timeline", vec![Command::SetAttributes { node: t, values: attrs(&[("fps", "30"), ("resources_acquisition", "1")]) }]).unwrap();

    // curves: keys at 10, 20, 30 on HeadYaw and LShoulderRoll
    let robot = choren_robot::Robot::load("nao6").unwrap();
    let pose = |pairs: &[(&str, f64)]| {
        let mut a = vec![0.0; robot.joints.len()];
        pairs.iter().for_each(|(j, v)| a[robot.joint_index(j).unwrap()] = v.to_radians());
        a
    };
    for (f, v) in [(10, 10.0), (20, 20.0), (30, 40.0)] {
        edit::apply(&mut p, &mut h, &bid, "Store", vec![Command::StoreKeys { timeline: t, frame: f, group: "HeadYaw".into(), angles: pose(&[("HeadYaw", v)]) }, Command::StoreKeys { timeline: t, frame: f, group: "LShoulderRoll".into(), angles: pose(&[("LShoulderRoll", v)]) }]).unwrap();
    }
    let curve = |p: &Project, name: &str| view::box_view(&p.behaviors[0].doc, tb).timeline.unwrap().curves.into_iter().find(|c| c.actuator == name);
    edit::apply(&mut p, &mut h, &bid, "Range", vec![Command::SetTimelineRange { timeline: t, start: 10, end: 30 }]).unwrap();
    assert_eq!(range(&p), (10, 30, 300.0));
    edit::apply(&mut p, &mut h, &bid, "Stretch", vec![Command::StretchTimelineRange { timeline: t, start: 10, end: 50 }]).unwrap();
    assert_eq!(range(&p), (10, 50, 300.0));
    assert_eq!(curve(&p, "HeadYaw").unwrap().frames, [10.0, 30.0, 50.0]);
    assert_eq!(kf(&p)[..2], [("keyframe1".into(), 10), ("keyframe1".into(), 30)]);
    assert_eq!(kf(&p)[2..], [("kf2".into(), 50), ("keyframe1".into(), 60)], "keyframes after the range stay");
    edit::undo(&mut p, &mut h).unwrap();

    // Mirror in a frame range copies only those keys; Flip in a range swaps them
    edit::apply(&mut p, &mut h, &bid, "Mirror", vec![Command::Mirror { timeline: t, mode: MirrorMode::LeftToRight, frames: Some((15, 30)) }]).unwrap();
    assert_eq!((curve(&p, "RShoulderRoll").unwrap().frames, curve(&p, "RShoulderRoll").unwrap().values), (vec![20.0, 30.0], vec![-20.0, -40.0]));
    edit::undo(&mut p, &mut h).unwrap();
    let before_flip = doc(&p);
    edit::apply(&mut p, &mut h, &bid, "Flip", vec![Command::Mirror { timeline: t, mode: MirrorMode::Flip, frames: Some((20, 20)) }]).unwrap();
    assert_eq!(curve(&p, "LShoulderRoll").unwrap().frames, [10.0, 30.0]);
    assert_eq!(curve(&p, "RShoulderRoll").unwrap().values, [-20.0]);
    assert_eq!(curve(&p, "HeadYaw").unwrap().values, [10.0, -20.0, 40.0]);
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(doc(&p), before_flip);

    // values and tangents merge while dragging; copy/paste replaces keys at the same frames
    let yaw = curve(&p, "HeadYaw").unwrap();
    let keys: Vec<usize> = yaw.keys.iter().map(|k| k.node).collect();
    edit::apply(&mut p, &mut h, &bid, "Values", vec![Command::SetCurveValues { keys: keys[..2].to_vec(), scale: 2.0, offset: 0.0 }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Values", vec![Command::SetCurveValues { keys: keys[..2].to_vec(), scale: 1.0, offset: 1.0 }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").unwrap().values, [21.0, 41.0, 40.0]);
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(curve(&p, "HeadYaw").unwrap().values, [10.0, 20.0, 40.0], "one merged step");
    edit::apply(&mut p, &mut h, &bid, "Interp", vec![Command::SetInterpolation { keys: vec![keys[1]], interp: Interp::Bezier }]).unwrap();
    let right = curve(&p, "HeadYaw").unwrap().keys[1].right.clone().unwrap().node;
    assert!(edit::apply(&mut p, &mut h, &bid, "Tangent", vec![Command::MoveTangent { tangent: right, abscissa: -1.0, ordinate: 0.0 }]).is_err());
    edit::apply(&mut p, &mut h, &bid, "Tangent", vec![Command::MoveTangent { tangent: right, abscissa: 2.0, ordinate: 1.0 }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Tangent", vec![Command::MoveTangent { tangent: right, abscissa: 3.0, ordinate: 1.5 }]).unwrap();
    assert_eq!(h.state(vec![]).undo.as_deref(), Some("Tangent"));
    edit::undo(&mut p, &mut h).unwrap();
    assert_eq!(h.state(vec![]).undo.as_deref(), Some("Interp"), "tangent moves merge");
    let clip = edit::copy(&p.behaviors[0].doc, &keys[..2]);
    edit::apply(&mut p, &mut h, &bid, "Paste keys", vec![Command::PasteCurveKeys { timeline: t, xml: clip, frame: 30 }]).unwrap();
    let yaw = curve(&p, "HeadYaw").unwrap();
    assert_eq!((yaw.frames, yaw.values), (vec![10.0, 20.0, 30.0, 40.0], vec![10.0, 20.0, 10.0, 20.0]));

    // Choren's simplifier (the default) drops keys that a straight line between the kept ones
    // reproduces within the tolerance (key 20 is 5 off the line 10..30) and leaves the handles
    let last = curve(&p, "LShoulderRoll").unwrap().keys[2].node;
    edit::apply(&mut p, &mut h, &bid, "Interp", vec![Command::SetInterpolation { keys: vec![last], interp: Interp::Bezier }]).unwrap();
    let simplify = |json: &str| serde_json::from_str::<Command>(&format!(r#"{{"type": "simplifyCurves", "timeline": {t}, "curves": [], "from": 0, "to": 30, {json}}}"#)).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Simplify", vec![simplify(r#""tolerance": 5.1"#)]).unwrap();
    let roll = curve(&p, "LShoulderRoll").unwrap();
    assert_eq!(roll.frames, [10.0, 30.0]);
    assert_eq!(roll.keys[1].left.clone().unwrap().abscissa, -3.33333, "the handle stays");
    edit::undo(&mut p, &mut h).unwrap();
    assert!(edit::apply(&mut p, &mut h, &bid, "Simplify", vec![simplify(r#""tolerance": 0.5, "advanced": [10, 5]"#)]).is_err());

    // the Choregraphe-compatible one drops keys whose removal stays within the error margin and
    // re-aims the neighbours' handles at their new neighbours; RemoveKeys over a range
    edit::apply(&mut p, &mut h, &bid, "Simplify", vec![simplify(r#""tolerance": 0.5, "mode": "Choregraphe""#)]).unwrap();
    assert_eq!(curve(&p, "LShoulderRoll").unwrap().frames, [10.0, 20.0, 30.0], "a corner stays");
    edit::apply(&mut p, &mut h, &bid, "Simplify", vec![Command::SimplifyCurves { timeline: t, curves: vec![], from: 0, to: 30, tolerance: 50.0, mode: SimplifyMode::Choregraphe, advanced: None }]).unwrap();
    let roll = curve(&p, "LShoulderRoll").unwrap();
    assert_eq!(roll.frames, [10.0, 30.0]);
    let handle = roll.keys[1].left.clone().unwrap();
    // the saved -3.33333 loads as a fraction 0.333333 of the 10 frames to key 20, now of 20 frames
    assert_eq!((handle.abscissa, handle.ordinate), (-6.66666, 0.0), "a third of the way back to frame 10");
    // advanced mode (25 fps): one
    // segment from 10 to 40, its right handle moved a step to 0.553333 of the way; the keys are
    // made anew, so the sides between them become Bezier and the smooth flag goes
    let yaw = curve(&p, "HeadYaw").unwrap();
    assert_eq!(yaw.frames, [10.0, 30.0, 40.0]);
    edit::apply(&mut p, &mut h, &bid, "Smooth", vec![Command::SetAttributes { node: yaw.keys[2].node, values: attrs(&[("smooth", "1")]) }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "FPS", vec![Command::SetAttributes { node: t, values: attrs(&[("fps", "25")]) }]).unwrap();
    edit::apply(&mut p, &mut h, &bid, "Simplify", vec![Command::SimplifyCurves { timeline: t, curves: vec![yaw.node], from: 0, to: 40, tolerance: 50.0, mode: SimplifyMode::Choregraphe, advanced: Some((10.0, 5.0)) }]).unwrap();
    let yaw = curve(&p, "HeadYaw").unwrap();
    assert_eq!(yaw.frames, [10.0, 40.0]);
    let side = |t: Option<view::Tangent>| t.map(|t| (t.interp, t.abscissa, t.ordinate));
    assert_eq!((side(yaw.keys[0].left.clone()), side(yaw.keys[0].right.clone())), (None, Some(("bezier".into(), 16.6, 0.0))), "the outer side stays automatic");
    assert_eq!(side(yaw.keys[1].left.clone()), Some(("bezier".into(), -9.99999, -15.0)), "a third of the way back to frame 10, slope 1.5");
    assert!(!yaw.keys[1].smooth);
    edit::apply(&mut p, &mut h, &bid, "Remove", vec![Command::RemoveKeys { timeline: t, frame: 0, group: "Head".into(), to: Some(35) }]).unwrap();
    assert_eq!(curve(&p, "HeadYaw").unwrap().frames, [40.0]);

    while edit::undo(&mut p, &mut h).is_some() {}
    assert_eq!(doc(&p), original);
    while edit::redo(&mut p, &mut h).is_some() {}
    while edit::undo(&mut p, &mut h).is_some() {}
    assert_eq!(doc(&p), original);
}
