//! Project sessions held by the backend. The UI opens a project here and receives the same
//! `ProjectDocument` shape it used to parse itself; edit/undo/redo/save are ready for the
//! authoring UI.

use choren_core::edit::{self, Command, History, HistoryState, NewBoxKind};
use choren_core::project::{FilePolicy, Inserted, Manifest, Project};
use choren_core::view::{self, ProjectDocument};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;
use tauri_plugin_fs::FsExt;

#[derive(Default)]
pub struct Sessions {
    next: Mutex<u32>,
    open: Mutex<HashMap<u32, (Project, History)>>,
}

#[derive(Serialize)]
pub struct Opened {
    session: u32,
    #[serde(flatten)]
    document: ProjectDocument,
}

#[derive(Serialize)]
pub struct Changed {
    history: HistoryState,
    document: ProjectDocument,
}

impl Sessions {
    /// The application package of the current project, the name (`<uuid>/<folder>`) the robot
    /// starts `behavior` by, as Choregraphe's Play uploads it, its boxes' debugger paths and its
    /// box-to-box links.
    #[allow(clippy::type_complexity)]
    pub fn package(&self, session: u32, behavior: &str) -> Result<(Vec<u8>, String, std::collections::HashMap<String, Vec<i64>>, std::collections::HashMap<String, Vec<(String, String)>>), String> {
        with(self, session, |p, _| {
            if p.format != choren_core::project::Format::Pml {
                return Err("Play needs a .pml project. Create one with File › New project…".into());
            }
            let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
            let uuid = p.manifest_fields().ok_or("The project has no manifest.")?.uuid;
            Ok((p.package()?, format!("{uuid}/{}", b.source.trim_end_matches("/behavior.xar")), choren_naoqi::runtime::debugger_index(&b.doc), choren_naoqi::runtime::output_links(&b.doc)))
        })
    }
}

impl Sessions {
    /// The debugger path of the box at `path` (editor address) and the name of its port `port`.
    pub fn box_port(&self, session: u32, behavior: &str, path: &[[i64; 3]], port: Option<i64>) -> Result<(String, String), String> {
        with(self, session, |p, _| {
            let d = &p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?.doc;
            let b = choren_naoqi::runtime::box_at(d, path)?;
            let name = match port {
                Some(id) => d.attr_or(d.kids(b, "Input").find(|&x| d.attr_num(x, "id", -1.0) as i64 == id).ok_or("Input not found")?, "name"),
                None => String::new(),
            };
            Ok((choren_naoqi::runtime::box_uri(d, b), name))
        })
    }
}

fn with<T>(s: &Sessions, id: u32, f: impl FnOnce(&mut Project, &mut History) -> Result<T, String>) -> Result<T, String> {
    let mut open = s.open.lock().unwrap();
    let (p, h) = open.get_mut(&id).ok_or("This project is no longer open.")?;
    f(p, h)
}

fn register(sessions: &Sessions, project: Project) -> Opened {
    let document = view::project(&project);
    let mut next = sessions.next.lock().unwrap();
    *next += 1;
    sessions.open.lock().unwrap().insert(*next, (project, History::fresh()));
    Opened { session: *next, document }
}

#[tauri::command]
pub fn project_open(app: tauri::AppHandle, sessions: State<Sessions>, path: String) -> Result<Opened, String> {
    let file = std::path::PathBuf::from(&path);
    if !app.fs_scope().is_allowed(&file) {
        return Err("Select the project through the file picker first.".into());
    }
    Ok(register(&sessions, Project::open(&file)?))
}

#[tauri::command]
pub fn project_close(sessions: State<Sessions>, session: u32) {
    sessions.open.lock().unwrap().remove(&session);
}

#[tauri::command]
pub fn project_edit(sessions: State<Sessions>, session: u32, behavior: String, label: String, commands: Vec<Command>) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        let created = edit::apply(p, h, &behavior, &label, commands)?.created;
        Ok(Changed { history: p.state(h, created), document: view::project(p) })
    })
}

#[tauri::command]
pub fn project_undo(sessions: State<Sessions>, session: u32) -> Result<Option<Changed>, String> {
    with(&sessions, session, |p, h| Ok(edit::undo(p, h).map(|_| Changed { history: p.state(h, vec![]), document: view::project(p) })))
}

#[tauri::command]
pub fn project_redo(sessions: State<Sessions>, session: u32) -> Result<Option<Changed>, String> {
    with(&sessions, session, |p, h| Ok(edit::redo(p, h).map(|_| Changed { history: p.state(h, vec![]), document: view::project(p) })))
}

#[tauri::command]
pub fn project_save(sessions: State<Sessions>, session: u32) -> Result<HistoryState, String> {
    with(&sessions, session, |p, h| {
        p.save()?;
        h.mark_saved();
        Ok(p.state(h, vec![]))
    })
}

#[tauri::command]
pub fn project_copy(sessions: State<Sessions>, session: u32, behavior: String, boxes: Vec<usize>) -> Result<String, String> {
    with(&sessions, session, |p, _| {
        let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        Ok(edit::copy(&b.doc, &boxes))
    })
}

/// Creates `<dir>/<name>.pml` with one diagram behavior and opens it.
#[tauri::command]
pub fn project_new(app: tauri::AppHandle, sessions: State<Sessions>, dir: String, name: String) -> Result<Opened, String> {
    let dir = std::path::PathBuf::from(dir);
    if !app.fs_scope().is_allowed(&dir) {
        return Err("Select the folder through the file picker first.".into());
    }
    Ok(register(&sessions, Project::create(&dir, &name)?))
}

#[derive(Serialize)]
pub struct InsertResult {
    insert: Inserted,
    document: ProjectDocument,
}

/// Inserts a library box into a diagram; on attached-file conflicts nothing changes and the
/// conflicting paths come back so the UI can retry with a `policy`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn project_insert_library_box(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, behavior: String, diagram: usize, path: String, x: f64, y: f64, policy: Option<FilePolicy>) -> Result<InsertResult, String> {
    let path = std::path::PathBuf::from(path);
    crate::library::readable(&app, &path)?;
    with(&sessions, session, |p, h| {
        let insert = p.insert_library_box(h, &behavior, diagram, &path, x, y, policy)?;
        Ok(InsertResult { insert, document: view::project(p) })
    })
}

#[tauri::command]
pub fn project_add_behavior(sessions: State<Sessions>, session: u32, name: String, root: NewBoxKind) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        edit::change_project(p, h, "Add behavior", |p| p.add_behavior(&name, root).map(|_| ()))?;
        Ok(Changed { history: p.state(h, vec![]), document: view::project(p) })
    })
}

/// Sources must be explicitly selected with the native picker; destinations stay in the project.
#[tauri::command]
pub fn project_import_files(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, paths: Vec<String>, destination: String, folder: bool, policy: Option<FilePolicy>) -> Result<Imported, String> {
    let sources = paths.iter().map(|p| granted(&app, p)).collect::<Result<Vec<_>, _>>()?;
    with(&sessions, session, |p, h| {
        let conflicts = p.import_files(h, &sources, &destination, folder, policy)?;
        Ok(if conflicts.is_empty() { Imported::Done(changed(p, h, vec![])) } else { Imported::Conflicts { conflicts } })
    })
}

/// An import's result; on same-named content nothing changes and the paths come back so the
/// UI can ask and retry with a `policy`.
#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum Imported {
    Done(Changed),
    Conflicts { conflicts: Vec<String> },
}

#[tauri::command]
pub fn project_import_behavior(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, path: String, name: String, policy: Option<FilePolicy>) -> Result<Imported, String> {
    let source = granted(&app, &path)?;
    with(&sessions, session, |p, h| {
        let mut conflicts = vec![];
        // Conflicts abort the change so no empty undo step is recorded.
        let result = edit::change_project(p, h, "Import behavior", |p| {
            conflicts = p.import_behavior(&source, &name, policy)?;
            if conflicts.is_empty() { Ok(()) } else { Err(String::new()) }
        });
        if !conflicts.is_empty() { return Ok(Imported::Conflicts { conflicts }); }
        result?;
        Ok(Imported::Done(changed(p, h, vec![])))
    })
}

/// Removes a behavior (or every entry under `folder`) from the project. With `delete_files`, the
/// behavior's folder (or `folder`) is deleted from disk on the next save, except files the
/// project still lists.
fn remove(p: &mut Project, behavior: Option<&str>, folder: Option<&str>, delete_files: bool) -> Result<(), String> {
    match (behavior, folder) {
        (Some(id), _) => {
            let source = p.behaviors.iter().find(|b| b.id == id).ok_or("Unknown behavior")?.source.clone();
            p.remove_behavior(&mut History::fresh(), id)?;
            if delete_files { p.delete_path(choren_core::project::behavior_files(&source))?; }
        }
        (None, Some(folder)) => {
            p.remove_folder(&mut History::fresh(), folder)?;
            if delete_files { p.delete_path(folder)?; }
        }
        (None, None) => return Err("Select a behavior or a folder.".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn project_remove_behavior(sessions: State<Sessions>, session: u32, behavior: String, delete_files: Option<bool>) -> Result<Changed, String> {
    let delete = delete_files.unwrap_or(false);
    with(&sessions, session, |p, h| {
        edit::change_project(p, h, if delete { "Delete behavior" } else { "Remove behavior" }, |p| remove(p, Some(&behavior), None, delete))?;
        Ok(Changed { history: p.state(h, vec![]), document: view::project(p) })
    })
}

#[tauri::command]
pub fn project_remove_folder(sessions: State<Sessions>, session: u32, folder: String, delete_files: bool) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        edit::change_project(p, h, if delete_files { "Delete folder" } else { "Remove folder" }, |p| remove(p, None, Some(&folder), delete_files))?;
        Ok(changed(p, h, vec![]))
    })
}

/// The files that removing a behavior or folder with `delete_files` would delete on save.
#[tauri::command]
pub fn project_removal_files(sessions: State<Sessions>, session: u32, behavior: Option<String>, folder: Option<String>) -> Result<Vec<String>, String> {
    with(&sessions, session, |p, _| p.deletion_preview(|p| remove(p, behavior.as_deref(), folder.as_deref(), true)))
}

#[tauri::command]
pub fn project_manifest(sessions: State<Sessions>, session: u32) -> Result<Option<Manifest>, String> {
    with(&sessions, session, |p, _| Ok(p.manifest_fields()))
}

#[tauri::command]
pub fn project_set_manifest(sessions: State<Sessions>, session: u32, manifest: Manifest) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        edit::change_project(p, h, "Edit project properties", |p| p.set_manifest_fields(&manifest))?;
        Ok(Changed { history: p.state(h, vec![]), document: view::project(p) })
    })
}

/// Adds a box from an open diagram to a user library ("Add to library").
#[tauri::command]
pub fn library_add_box(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, behavior: String, node: usize, library: String, folder: String, name: String) -> Result<String, String> {
    crate::library::user_library(&app, &library)?;
    let (xml, attachments) = with(&sessions, session, |p, _| {
        let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        if node >= b.doc.nodes.len() || !b.doc.is_el(node, "Box") {
            return Err("Only boxes can be added to a library".to_string());
        }
        let xml = b.doc.subtree_string(node);
        let mut d = choren_core::xml::Doc::default();
        d.parse_fragment(&xml)?;
        let mut paths = std::collections::BTreeSet::new();
        for n in 0..d.nodes.len() {
            if d.is_el(n, "Parameter") && d.attr_raw(n, "content_type") == Some("4") {
                for attr in ["value", "default_value"] {
                    let path = d.attr_or(n, attr);
                    if !path.is_empty() { paths.insert(choren_core::project::relative(path.trim_start_matches('/'))?); }
                }
            }
            if d.is_el(n, "bitmap") {
                let path = d.text(n);
                if p.files.iter().any(|f| f.path == path && f.kind == "resource") { paths.insert(path); }
            }
            if d.is_el(n, "dialogFile") {
                let path = choren_core::project::relative(&d.text(n))?;
                let parent = path.rsplit_once('/').map_or("", |(p,_)| p);
                paths.insert(path.clone());
                paths.extend(p.files.iter().filter(|f| f.kind == "topic" && f.path.rsplit_once('/').map_or("", |(p,_)| p) == parent).map(|f| f.path.clone()));
            }
        }
        let attachments = paths.into_iter().map(|path| {
            let bytes = p.read_file(&path).ok_or_else(|| format!("Missing attached file {path}"))?;
            Ok((path, bytes))
        }).collect::<Result<Vec<_>, String>>()?;
        Ok((xml, attachments))
    })?;
    if attachments.iter().any(|(rel,_)| matches!(rel.as_str(), "box.xar" | "behavior.xar" | ".metadata" | "xalinfo")) { return Err("An attached file conflicts with a reserved library file.".into()); }
    let file = choren_core::library::add_box(std::path::Path::new(&library), &folder, &name, &xml)?;
    let copy = || -> Result<(), String> {
        for (rel, bytes) in attachments {
            let path = file.parent().unwrap().join(rel);
            if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    if let Err(error) = copy() {
        let relative = file.parent().unwrap().strip_prefix(&library).map_err(|e| e.to_string())?.to_string_lossy();
        let cleanup = choren_core::library::remove(std::path::Path::new(&library), &relative);
        return Err(format!("Cannot copy attached files: {error}{}", cleanup.err().map(|e| format!(". The new library entry could not be removed: {e}")).unwrap_or_default()));
    }
    Ok(file.to_string_lossy().into_owned())
}

pub(crate) fn granted(app: &tauri::AppHandle, path: &str) -> Result<std::path::PathBuf, String> {
    let file = std::path::PathBuf::from(path);
    if !app.fs_scope().is_allowed(&file) {
        return Err("Select the file through the file picker first.".into());
    }
    Ok(file)
}

fn changed(p: &Project, h: &History, created: Vec<usize>) -> Changed {
    Changed { history: p.state(h, created), document: view::project(p) }
}

/// "Update Translations": regenerates `translations/translation_<lang>.ts` (one undo step).
#[tauri::command]
pub fn project_update_translations(sessions: State<Sessions>, session: u32) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        choren_core::translations::update(p, h)?;
        Ok(changed(p, h, vec![]))
    })
}

/// Adds a dialog topic (`.dlg` + one `.top` per language).
#[tauri::command]
pub fn project_add_dialog(sessions: State<Sessions>, session: u32, name: String, languages: Vec<String>) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        p.add_dialog(h, &name, &languages)?;
        Ok(changed(p, h, vec![]))
    })
}

/// Text of a project file (staged contents first), e.g. a `.top` topic.
#[tauri::command]
pub fn project_read_text(sessions: State<Sessions>, session: u32, path: String) -> Result<String, String> {
    with(&sessions, session, |p, _| {
        let rel = choren_core::project::relative(&path)?;
        if !p.files.iter().any(|f| f.path == rel) { return Err("Select a listed project file.".into()); }
        if let Some(b) = p.behaviors.iter().find(|b| b.source == rel) { return Ok(b.doc.write()); }
        if let Some((path, doc)) = &p.manifest { if path == &rel { return Ok(doc.write()); } }
        if p.files.iter().any(|f| f.path == rel && f.kind == "project") { return Ok(p.pml.as_ref().ok_or("Missing project document")?.write()); }
        let bytes = p.read_file(&rel).ok_or_else(|| format!("Cannot read {rel}"))?;
        String::from_utf8(bytes).map_err(|_| format!("{rel} is not UTF-8 text"))
    })
}

/// Stages new text for a project file as an undo step (written on save).
#[tauri::command]
pub fn project_write_text(sessions: State<Sessions>, session: u32, path: String, text: String) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        let path = choren_core::project::relative(&path)?;
        let kind = p.files.iter().find(|f| f.path == path).ok_or("Select a listed project file")?.kind;
        if kind == "project" || kind == "behavior" { return Err("Use the project or behavior editor for this file.".into()); }
        if kind == "manifest" {
            let doc = choren_core::xml::Doc::parse(&text)?;
            if doc.name(doc.root().ok_or("Empty manifest")?) != "package" { return Err("A manifest needs a package root element.".into()); }
            edit::change_project(p, h, "Edit manifest", |p| {
                p.manifest = Some((path, doc));
                if let Some(fields) = p.manifest_fields() {
                    p.metadata.insert("UUID".into(), fields.uuid);
                    p.metadata.insert("Version".into(), fields.version);
                    p.metadata.remove("Title");
                    if let Some((_, name)) = fields.names.first() { p.metadata.insert("Title".into(), name.clone()); }
                }
                Ok(())
            })?;
            return Ok(changed(p, h, vec![]));
        }
        if kind == "translation" {
            let doc = choren_core::xml::Doc::parse(&text)?;
            if doc.name(doc.root().ok_or("Empty translation")?) != "TS" { return Err("A translation needs a TS root element.".into()); }
        }
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        edit::set_files(p, h, &format!("Edit {name}"), vec![(path, text.into_bytes())])?;
        Ok(changed(p, h, vec![]))
    })
}

/// "Export to CRG": one behavior and the project's resources as a legacy archive.
#[tauri::command]
pub fn project_export_crg(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, behavior: String, path: String) -> Result<(), String> {
    let file = granted(&app, &path)?;
    let bytes = with(&sessions, session, |p, _| p.export_crg(&behavior))?;
    std::fs::write(&file, bytes).map_err(|e| format!("Cannot write {path}: {e}"))
}

/// Writes an imported `.crg` (or loose `.xar`) as `<dir>/<name>.pml` and reopens it in place.
#[tauri::command]
pub fn project_save_as_folder(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, dir: String, name: String) -> Result<Opened, String> {
    let dir = granted(&app, &dir)?;
    let pml = with(&sessions, session, |p, _| p.save_as_folder(&dir, &name))?;
    app.fs_scope().allow_directory(&dir, true).map_err(|e| e.to_string())?;
    let project = Project::open(&pml)?;
    let document = view::project(&project);
    sessions.open.lock().unwrap().insert(session, (project, History::fresh()));
    Ok(Opened { session, document })
}

/// "Export motion to clipboard › Python": the script text, Bezier or simplified.
#[tauri::command]
pub fn project_export_motion_python(sessions: State<Sessions>, session: u32, behavior: String, timeline: usize, bezier: bool) -> Result<String, String> {
    with(&sessions, session, |p, _| {
        let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        if timeline >= b.doc.nodes.len() || !b.doc.is_el(timeline, "Timeline") {
            return Err("No timeline selected".to_string());
        }
        Ok(choren_core::motion::to_python(&b.doc, timeline, bezier))
    })
}

/// Exports a timeline's motion curves as a `.qianim` file.
#[tauri::command]
pub fn project_export_qianim(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, behavior: String, timeline: usize, path: String) -> Result<(), String> {
    let file = granted(&app, &path)?;
    let text = with(&sessions, session, |p, _| {
        let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        if timeline >= b.doc.nodes.len() || !b.doc.is_el(timeline, "Timeline") {
            return Err("No timeline selected".to_string());
        }
        choren_core::motion::to_qianim(&b.doc, timeline)
    })?;
    std::fs::write(&file, text).map_err(|e| format!("Cannot write {path}: {e}"))
}

/// Replaces a timeline's motion curves with a `.qianim` file's (one undo step).
#[tauri::command]
pub fn project_import_qianim(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, behavior: String, timeline: usize, path: String) -> Result<Changed, String> {
    let file = granted(&app, &path)?;
    let xml = std::fs::read_to_string(&file).map_err(|e| format!("Cannot read {path}: {e}"))?;
    with(&sessions, session, |p, h| {
        let b = p.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        if timeline >= b.doc.nodes.len() || !b.doc.is_el(timeline, "Timeline") {
            return Err("No timeline selected".to_string());
        }
        let fps = b.doc.attr_num(timeline, "fps", 25.0);
        let old: Vec<usize> = b.doc.kids(timeline, "ActuatorList").collect();
        let list = choren_core::motion::from_qianim(&xml, fps)?;
        let s = edit::apply(p, h, &behavior, "Import animation", vec![Command::Remove { nodes: old }, Command::InsertXml { parent: timeline, xml: list }])?;
        Ok(changed(p, h, s.created))
    })
}

#[tauri::command]
pub fn project_resource(sessions: State<Sessions>, session: u32, path: String, target: Option<String>) -> Result<Changed, String> {
    with(&sessions, session, |p, h| {
        edit::change_project(p, h, if target.is_some() { "Rename resource" } else { "Remove resource reference" }, |p| {
            if let Some(target) = target { p.rename_resource(&path, &target) } else { p.remove_resource(&path) }
        })?;
        Ok(changed(p, h, vec![]))
    })
}

/// File › Build application package: the current project as a `.pkg` (the same bytes Play uploads).
#[tauri::command]
pub fn project_build_package(app: tauri::AppHandle, sessions: State<Sessions>, session: u32, path: String) -> Result<(), String> {
    let file = granted(&app, &path)?;
    let bytes = with(&sessions, session, |p, _| p.package())?;
    std::fs::write(&file, bytes).map_err(|e| format!("Cannot write {path}: {e}"))
}

#[tauri::command]
pub fn project_read_binary(sessions: State<Sessions>, session: u32, path: String) -> Result<Vec<u8>, String> {
    with(&sessions, session, |p, _| {
        let path = choren_core::project::relative(&path)?;
        if !p.files.iter().any(|f| f.path == path && f.kind == "resource") { return Err("Select a project resource.".into()); }
        let bytes = p.read_file(&path).ok_or("Resource is missing")?;
        if bytes.len() > 64 * 1024 * 1024 { return Err("Preview supports resources up to 64 MB.".into()); }
        Ok(bytes)
    })
}
