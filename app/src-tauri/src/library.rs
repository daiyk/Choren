//! Box and pose libraries. The install library is read-only and needs a picker grant; Choren's
//! clean-room library ships as an app resource. The Libraries panel also manages user folders.

use choren_core::library::{self, Entry};
use choren_core::{poses, view};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_fs::FsExt;

pub fn choren_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().resolve("library/choren", tauri::path::BaseDirectory::Resource).map_err(|e| e.to_string())
}

/// Reads are limited to picker-granted paths and the bundled Choren library.
pub fn readable(app: &AppHandle, path: &Path) -> Result<(), String> {
    let bundled = choren_root(app).ok().and_then(|r| r.canonicalize().ok());
    let inside = |root: &Option<PathBuf>| root.as_ref().is_some_and(|r| path.canonicalize().is_ok_and(|p| p.starts_with(r)));
    if app.fs_scope().is_allowed(path) || inside(&bundled) { Ok(()) } else { Err("This location has not been granted to Choren.".into()) }
}

#[tauri::command]
pub fn library_index(app: AppHandle, share_dir: Option<String>, user_dirs: Option<Vec<String>>) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = library::index(&choren_root(&app)?, "choren")
        .into_iter()
        .map(|e| Entry { category: format!("Choren / {}", e.category), ..e })
        .collect();
    if let Some(share) = share_dir {
        let root = Path::new(&share).join("libraries/box");
        readable(&app, &root)?;
        out.extend(library::index(&root, "install"));
    }
    for dir in user_dirs.unwrap_or_default() {
        readable(&app, Path::new(&dir))?;
        out.extend(library::index(Path::new(&dir), "user"));
    }
    Ok(out)
}

#[tauri::command]
pub fn library_tree(app: AppHandle, share_dir: Option<String>, user_dirs: Option<Vec<String>>) -> Result<Vec<library::TreeRoot>, String> {
    let mut roots = vec![library::tree(&choren_root(&app)?, "choren", false)?];
    if let Some(share) = share_dir {
        let root = Path::new(&share).join("libraries/box");
        readable(&app, &root)?;
        roots.push(library::tree(&root, "install", false)?);
    }
    for dir in user_dirs.unwrap_or_default() {
        readable(&app, Path::new(&dir))?;
        let root = library::tree(Path::new(&dir), "user", user_library(&app, &dir).is_ok())?;
        if !roots.iter().any(|existing| existing.path == root.path) { roots.push(root); }
    }
    Ok(roots)
}

#[tauri::command]
pub fn library_box(app: AppHandle, path: String) -> Result<view::BoxView, String> {
    let path = PathBuf::from(path);
    readable(&app, &path)?;
    let (doc, _) = library::box_xml(&path)?;
    Ok(view::box_view(&doc, view::root_box(&doc)))
}

#[tauri::command]
pub fn library_create(app: AppHandle, dir: String, title: String) -> Result<(), String> {
    let dir = PathBuf::from(dir);
    readable(&app, dir.parent().unwrap_or(&dir))?;
    library::create(&dir, &title)
}

/// Settings › Choregraphe libraries: the installation's `share/choregraphe` folder from what the
/// user picked (that folder, `share`, the installation folder or, on macOS, `Choregraphe.app` or
/// the folder holding it).
/// Its sibling `alrobotmodel` holds the robot meshes, so anything else is refused.
#[tauri::command]
pub fn library_share_dir(app: AppHandle, path: String) -> Result<String, String> {
    let picked = PathBuf::from(path);
    readable(&app, &picked)?;
    share_dir(&picked)
        .map(|d| d.to_string_lossy().into_owned())
        .ok_or_else(|| "This folder is not a Choregraphe installation. Choose Choregraphe.app, the installation folder or its share/choregraphe folder.".into())
}

fn share_dir(picked: &Path) -> Option<PathBuf> {
    let children = |d: &Path| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    // A macOS bundle keeps the installation in Contents/Resources/choregraphe-<version>-mac; the
    // picked folder may be the bundle or the folder holding it.
    let apps = std::iter::once(picked.to_path_buf()).chain(children(picked).into_iter().filter(|c| c.extension().is_some_and(|x| x == "app")));
    let bundled = apps.flat_map(|app| children(&app.join("Contents/Resources"))).map(|r| r.join("share/choregraphe"));
    [picked.to_path_buf(), picked.join("choregraphe"), picked.join("share/choregraphe")]
        .into_iter()
        .chain(bundled)
        .find(|d| d.join("libraries").is_dir() && d.parent().is_some_and(|share| share.join("alrobotmodel").is_dir()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn share_dir_is_found_from_any_install_level_and_projects_are_refused() {
        let root = std::env::temp_dir().join(format!("choren-share-{}", std::process::id()));
        let share = root.join("Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share");
        std::fs::create_dir_all(share.join("choregraphe/libraries")).unwrap();
        std::fs::create_dir_all(share.join("alrobotmodel/meshes")).unwrap();
        std::fs::create_dir_all(root.join("project/behavior_1")).unwrap();
        for picked in [root.clone(), root.join("Choregraphe.app"), share.parent().unwrap().to_path_buf(), share.clone(), share.join("choregraphe")] {
            assert_eq!(super::share_dir(&picked), Some(share.join("choregraphe")), "{picked:?}");
        }
        assert_eq!(super::share_dir(&root.join("project")), None);
        assert_eq!(super::share_dir(&root.join("missing")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

/// Library › Import a `.cbl` box library file into a new (empty) library folder.
#[tauri::command]
pub fn library_import_cbl(app: AppHandle, path: String, dir: String) -> Result<(), String> {
    let (path, dir) = (PathBuf::from(path), PathBuf::from(dir));
    readable(&app, &path)?;
    readable(&app, dir.parent().unwrap_or(&dir))?;
    library::import_cbl(&std::fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?, &dir)
}

/// Library › Save library as a `.cbl` box library file (`path` from the save picker).
#[tauri::command]
pub fn library_export_cbl(app: AppHandle, library: String, path: String) -> Result<(), String> {
    let (library, path) = (PathBuf::from(library), PathBuf::from(path));
    readable(&app, &library)?;
    if !app.fs_scope().is_allowed(&path) {
        return Err("Select the file through the file picker first.".into());
    }
    std::fs::write(&path, library::export_cbl(&library)?).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

#[tauri::command]
pub fn library_remove(app: AppHandle, library: String, path: String) -> Result<(), String> {
    library::remove(&user_library(&app, &library)?, &path)
}

#[tauri::command]
pub fn poses_read(app: AppHandle, path: String) -> Result<Vec<poses::Pose>, String> {
    readable(&app, Path::new(&path))?;
    let src = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(poses::parse(&src)?.1)
}

/// Adds or replaces (`pose`) or removes (`remove`) a pose and writes the library file.
#[tauri::command]
pub fn poses_write(app: AppHandle, path: String, pose: Option<poses::Pose>, remove: Option<(String, String)>, previous: Option<(String, String)>) -> Result<Vec<poses::Pose>, String> {
    let p = Path::new(&path);
    // A save dialog grants the file itself; an open folder grants its parent.
    readable(&app, p).or_else(|_| readable(&app, p.parent().unwrap_or(p)))?;
    let mut doc = match std::fs::read_to_string(p) {
        Ok(src) => poses::parse(&src)?.0,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => poses::empty(),
        Err(e) => return Err(format!("Cannot read {path}: {e}")),
    };
    if let Some(pose) = pose {
        if let Some((folder, name)) = &previous {
            let existing = poses::parse(&doc.write())?.1;
            if !existing.iter().any(|p| &p.folder == folder && &p.name == name) { return Err("The original pose no longer exists. Reopen the library.".into()); }
            if (folder, name) != (&pose.folder, &pose.name) && existing.iter().any(|p| p.folder == pose.folder && p.name == pose.name) { return Err("A pose with that name already exists in this folder.".into()); }
            if (folder, name) != (&pose.folder, &pose.name) { poses::remove(&mut doc, folder, name); }
        }
        poses::add(&mut doc, &pose)?;
    }
    if let Some((folder, name)) = remove {
        poses::remove(&mut doc, &folder, &name);
    }
    let text = doc.write();
    std::fs::write(p, &text).map_err(|e| format!("Cannot write {path}: {e}"))?;
    Ok(poses::parse(&text)?.1)
}

pub fn user_library(app: &AppHandle, library: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(library).canonicalize().map_err(|e| e.to_string())?;
    if !app.fs_scope().is_allowed(&root) || root.starts_with(choren_root(app)?) || !root.join(".metadata").is_file() {
        return Err("Select an editable user library through the folder picker.".into());
    }
    Ok(root)
}

#[tauri::command]
pub fn library_manage(app: AppHandle, library: String, folder: String, action: String, name: String) -> Result<(), String> {
    library::manage(&user_library(&app, &library)?, &folder, &action, &name)
}

#[tauri::command]
pub fn library_copy(app: AppHandle, source: String, library: String, folder: String, name: String) -> Result<(), String> {
    let path = PathBuf::from(&source).canonicalize().map_err(|e| e.to_string())?;
    readable(&app, &path)?;
    library::copy_folder(Path::new(&source), &user_library(&app, &library)?, &folder, &name)
}

#[tauri::command]
pub fn library_edit(app: AppHandle, library: String, path: String, commands: Vec<choren_core::edit::Command>) -> Result<view::BoxView, String> {
    let root = user_library(&app, &library)?;
    let path = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(&root) { return Err("Select a box inside this library.".into()); }
    let mut p = choren_core::project::Project::open(&path)?;
    let id = p.behaviors.first().ok_or("Missing library box")?.id.clone();
    choren_core::edit::apply(&mut p, &mut choren_core::edit::History::fresh(), &id, "Edit library box", commands)?;
    p.save()?;
    Ok(view::box_view(&p.behaviors[0].doc, view::root_box(&p.behaviors[0].doc)))
}
