use choren_core::edit::{self, Command, History};
use choren_core::project::Project;
use choren_core::view;
use std::path::Path;

#[test]
fn box_category_survives_rename_save_and_undo_without_changing_behavior() {
    let dir = std::env::temp_dir().join(format!("choren-appearance-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut project = Project::create(&dir, "Appearance").unwrap();
    let mut history = History::fresh();
    let behavior = project.behaviors[0].id.clone();
    let root = view::root_box(&project.behaviors[0].doc);
    let original = project.behaviors[0].doc.write();
    let set = |category: Option<&str>| Command::SetAttributes {
        node: root,
        values: [("choren_category".into(), category.map(str::to_string))].into(),
    };
    assert!(edit::apply(&mut project, &mut history, &behavior, "Invalid", vec![set(Some("unknown"))]).is_err());
    assert_eq!(project.behaviors[0].doc.write(), original);
    edit::apply(&mut project, &mut history, &behavior, "Category", vec![set(Some("terminal"))]).unwrap();
    assert_eq!(view::box_view(&project.behaviors[0].doc, root).category.as_deref(), Some("terminal"));
    edit::undo(&mut project, &mut history).unwrap();
    assert_eq!(project.behaviors[0].doc.write(), original);
    edit::redo(&mut project, &mut history).unwrap();
    edit::apply(&mut project, &mut history, &behavior, "Rename", vec![Command::SetAttributes {
        node: root, values: [("name".into(), Some("Renamed".into()))].into(),
    }]).unwrap();
    project.save().unwrap();
    let reopened = Project::open(Path::new(&project.source)).unwrap();
    let doc = &reopened.behaviors[0].doc;
    assert_eq!(view::box_view(doc, view::root_box(doc)).category.as_deref(), Some("terminal"));
    edit::apply(&mut project, &mut history, &behavior, "Automatic", vec![set(None)]).unwrap();
    assert_eq!(view::box_view(&project.behaviors[0].doc, root).category, None);
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn comments_load_their_own_text_without_formatting_whitespace_and_survive_round_trip() {
    use choren_core::xml::Doc;
    let xml = include_str!("../../../app/test/fixtures/comments.xar");
    let doc = Doc::parse(xml).unwrap();
    for doc in [&doc, &Doc::parse(&doc.write()).unwrap()] {
        let root = view::box_view(doc, view::root_box(doc));
        assert!(root.plugin_text.is_none());
        let gesture = &root.timeline.as_ref().unwrap().layers[0].keyframes[0].diagram.as_ref().unwrap().boxes[0];
        let comments = &gesture.timeline.as_ref().unwrap().layers[0].keyframes[0].diagram.as_ref().unwrap().boxes;
        assert_eq!(comments[0].plugin_text.as_deref(), Some("Introduction\nAsk the child to copy the gesture."));
        assert_eq!(comments[0].script, "generated_script = True");
        assert!(comments[1].plugin_text.as_ref().unwrap().contains("Keep <b>this</b> & every line as plain text.\nLine 3"));
        assert_eq!(comments[2].plugin_text.as_deref(), Some(""));
    }
    let doc = Doc::parse(r#"<ChoregrapheProject><Box plugin="textedit_plugin"><pluginContent><text>A &amp; B&#10;C</text></pluginContent></Box></ChoregrapheProject>"#).unwrap();
    assert_eq!(view::box_view(&doc, view::root_box(&doc)).plugin_text.as_deref(), Some("A & B\nC"));
}
