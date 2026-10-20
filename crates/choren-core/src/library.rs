//! Box libraries: a folder tree ordered by `xalinfo`, one `box.xar` or `behavior.xar`
//! per box folder. The install library is read-only; Choren's own and user libraries share the format.

use crate::xml::{escape_attr, Doc};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// Absolute path of the box's XAR file.
    pub path: String,
    /// Folder path inside the library, joined with " / " (the UI groups by it).
    pub category: String,
    pub source: &'static str,
}

const BOX_FILES: [&str; 2] = ["box.xar", "behavior.xar"];
const META_FILES: [&str; 4] = ["box.xar", "behavior.xar", ".metadata", "xalinfo"];

/// Choregraphe percent-encodes some characters in folder names (`Set Reco. Lang%2e`).
pub fn decode_name(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len()).then(|| u8::from_str_radix(&s[i + 1..i + 3], 16).ok()).flatten();
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Child folder names in `xalinfo` order, then any unlisted ones by name.
fn ordered_children(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.is_symlink() && p.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.') && n != "translations"))
        .collect();
    dirs.sort();
    let listed: Vec<String> = std::fs::read_to_string(dir.join("xalinfo"))
        .ok()
        .and_then(|s| Doc::parse(&s).ok())
        .map(|d| {
            let root = d.root().unwrap();
            d.kids(root, "Node").map(|n| d.attr_or(n, "name")).collect()
        })
        .unwrap_or_default();
    let rank = |p: &PathBuf| {
        let n = p.file_name().unwrap().to_string_lossy();
        listed.iter().position(|l| *l == n || *l == decode_name(&n)).unwrap_or(usize::MAX)
    };
    dirs.sort_by_key(rank); // stable: unlisted folders keep their name order
    dirs
}

/// Indexes a library root such as `share/choregraphe/libraries/box`.
pub fn index(root: &Path, source: &'static str) -> Vec<Entry> {
    let mut out = vec![];
    walk(root, &mut vec![], source, &mut out);
    out
}

fn walk(dir: &Path, trail: &mut Vec<String>, source: &'static str, out: &mut Vec<Entry>) {
    for child in ordered_children(dir) {
        let name = decode_name(&child.file_name().unwrap().to_string_lossy());
        match BOX_FILES.iter().map(|f| child.join(f)).find(|f| f.is_file()) {
            Some(file) => out.push(Entry { name, path: crate::project::normalize(&file.to_string_lossy()), category: trail.join(" / "), source }),
            None => {
                trail.push(name);
                walk(&child, trail, source, out);
                trail.pop();
            }
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub relative_path: String,
    pub kind: &'static str,
    pub box_file: Option<String>,
    pub has_script: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TreeRoot {
    pub path: String,
    pub name: String,
    pub source: &'static str,
    pub writable: bool,
    pub children: Vec<TreeNode>,
}

/// Keeps raw filesystem identities separate from decoded display names, including empty folders.
pub fn tree(root: &Path, source: &'static str, writable: bool) -> Result<TreeRoot, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    fn children(root: &Path, dir: &Path) -> Vec<TreeNode> {
        ordered_children(dir).into_iter().map(|child| {
            let file = BOX_FILES.iter().map(|f| child.join(f)).find(|f| f.is_file() && !f.is_symlink());
            TreeNode {
                name: decode_name(&child.file_name().unwrap().to_string_lossy()),
                relative_path: child.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"),
                kind: if file.is_some() { "box" } else { "folder" },
                children: if file.is_some() { vec![] } else { children(root, &child) },
                has_script: file.as_ref().and_then(|f| box_xml(f).ok()).is_some_and(|(doc, _)| !crate::view::box_view(&doc, crate::view::root_box(&doc)).script.is_empty()),
                box_file: file.map(|f| crate::project::normalize(&f.to_string_lossy())),
            }
        }).collect()
    }
    Ok(TreeRoot {
        name: if source == "choren" { "Choren".into() } else { decode_name(&root.file_name().unwrap_or_default().to_string_lossy()) },
        path: crate::project::normalize(&root.to_string_lossy()),
        source,
        writable: writable && source == "user" && !is_install(&root) && !root.metadata().map_err(|e| e.to_string())?.permissions().readonly(),
        children: children(&root, &root),
    })
}

/// The library box's root `<Box>` element as XML, ready for `Command::AddBox`.
pub fn box_xml(path: &Path) -> Result<(Doc, String), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let doc = Doc::parse(&src)?;
    let b = doc.kid(doc.root().unwrap(), "Box").ok_or("Not a Choregraphe box (no root Box)")?;
    let xml = doc.subtree_string(b);
    Ok((doc, xml))
}

/// Files that travel with a library box (sounds, dialogs…), relative to its folder.
pub fn attached_files(box_file: &Path) -> Vec<String> {
    let dir = box_file.parent().unwrap_or(Path::new("."));
    let mut out = vec![];
    fn rec(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                if !p.is_symlink() && rel != "translations" {
                    rec(base, &p, out)
                }
            } else if !META_FILES.contains(&rel.as_str()) {
                out.push(rel);
            }
        }
    }
    rec(dir, dir, &mut out);
    out.sort();
    out
}

/// Creates an empty user library folder (`.metadata` + `xalinfo`).
pub fn create(dir: &Path, title: &str) -> Result<(), String> {
    if dir.exists() && std::fs::read_dir(dir).map_err(|e| e.to_string())?.next().is_some() {
        return Err("The selected directory is not empty.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write(&dir.join(".metadata"), &format!("Type: ChoregrapheBoxLibrary\nVersion: 3.0\ntitle: {title}\n"))?;
    write(&dir.join("xalinfo"), "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<Folder expanded=\"no\" />\n")
}

/// Adds a box (a `<Box>` element) to a user library folder, listing it in that folder's `xalinfo`.
pub fn add_box(library: &Path, folder: &str, name: &str, box_xml: &str) -> Result<PathBuf, String> {
    if is_install(library) {
        return Err("Selected box library is read only.".into());
    }
    let folder = folder.trim_matches('/');
    if !folder.is_empty() {
        crate::project::relative(folder)?;
    }
    if name.is_empty() || name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*']) {
        return Err("Can not contain any of these characters: \\ / < > : \" | ? *".into());
    }
    let mut checked = library.to_path_buf();
    for part in folder.split('/').filter(|p| !p.is_empty()).chain(std::iter::once(name)) {
        checked.push(part);
        if checked.is_symlink() { return Err("Library destinations cannot contain symbolic links.".into()); }
    }
    let mut parent = library.to_path_buf();
    for part in folder.split('/').filter(|p| !p.is_empty()) {
        list_in_xalinfo(&parent, part)?;
        parent.push(part);
        std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    }
    let target = parent.join(name);
    if target.exists() {
        return Err(format!("\"{name}\" already exists in this folder."));
    }
    let mut doc = Doc::default();
    let b = doc.parse_fragment(box_xml)?;
    doc.reformat(b, 1, 4);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let xar = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<ChoregrapheBox xar_version=\"3\">\n    {}\n</ChoregrapheBox>\n", doc.subtree_string(b));
    write(&target.join("box.xar"), &xar)?;
    list_in_xalinfo(&parent, name)?;
    Ok(target.join("box.xar"))
}

/// Removes a box or folder from a user library.
pub fn remove(library: &Path, rel: &str) -> Result<(), String> {
    if is_install(library) {
        return Err("Selected box library is read only.".into());
    }
    let rel = crate::project::relative(rel)?;
    let root = library.canonicalize().map_err(|e| e.to_string())?;
    let mut checked = library.to_path_buf();
    for part in rel.split('/') {
        checked.push(part);
        if checked.is_symlink() { return Err("Library folders cannot contain symbolic links.".into()); }
    }
    let target = library.join(&rel).canonicalize().map_err(|e| e.to_string())?;
    if target == root || !target.starts_with(&root) { return Err("Select a folder inside the library.".into()); }
    let parent = target.parent().unwrap().to_path_buf();
    let name = target.file_name().ok_or("Invalid library path")?.to_string_lossy().to_string();
    std::fs::remove_dir_all(&target).map_err(|e| format!("Cannot delete {rel}: {e}"))?;
    let path = parent.join("xalinfo");
    if let Some(mut d) = std::fs::read_to_string(&path).ok().and_then(|s| Doc::parse(&s).ok()) {
        let root = d.root().unwrap();
        let kids: Vec<_> = d.kids(root, "Node").filter(|&n| d.attr(n, "name").as_deref() == Some(&name)).collect();
        d.el_mut(root).children.retain(|c| !kids.contains(c));
        write(&path, &d.write())?;
    }
    Ok(())
}

fn list_in_xalinfo(dir: &Path, name: &str) -> Result<(), String> {
    let path = dir.join("xalinfo");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|_| "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<Folder expanded=\"no\" />\n".into());
    let mut d = Doc::parse(&src)?;
    let root = d.root().unwrap();
    if d.kids(root, "Node").any(|n| d.attr(n, "name").as_deref() == Some(name)) {
        return Ok(());
    }
    let node = d.parse_fragment(&format!("<Node name=\"{}\" />", escape_attr(name)))?;
    let pad = d.new_text("\n    ".into());
    let el = d.el_mut(root);
    if el.children.is_empty() {
        let close = d.new_text("\n".into());
        d.el_mut(root).children.extend([pad, node, close]);
    } else {
        let at = el.children.len() - 1;
        el.children.splice(at..at, [pad, node]);
    }
    d.nodes[node].parent = Some(root);
    write(&path, &d.write())
}

/// The install library (`…/share/choregraphe/libraries/box`) is never written.
fn is_install(dir: &Path) -> bool {
    dir.components().any(|c| c.as_os_str() == "share") && dir.ends_with("libraries/box")
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

/// User-library folder operations preserve the existing XAL ordering and refuse overwrites.
pub fn manage(library: &Path, folder: &str, action: &str, name: &str) -> Result<(), String> {
    if is_install(library) { return Err("Selected box library is read only.".into()); }
    let root = library.canonicalize().map_err(|e| e.to_string())?;
    let target = if folder.is_empty() { root.clone() } else { root.join(crate::project::relative(folder)?) };
    let mut checked = root.clone();
    for part in target.strip_prefix(&root).map_err(|e| e.to_string())?.components() {
        checked.push(part);
        if checked.is_symlink() { return Err("Library folders cannot contain symbolic links.".into()); }
    }
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&root) { return Err("Folder is outside the selected library.".into()); }
    let valid_name = || -> Result<(), String> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*', '\n', '\r']) { Err("Choose a valid folder name.".into()) } else { Ok(()) }
    };
    match action {
        "create" => {
            valid_name()?;
            let child = target.join(name);
            if child.exists() { return Err("This folder already exists.".into()); }
            std::fs::create_dir(&child).map_err(|e| e.to_string())?;
            write(&child.join("xalinfo"), "<Folder expanded=\"no\" />")?;
            list_in_xalinfo(&target, name)
        }
        "rename" => {
            valid_name()?;
            if target == root { return Err("Select a folder inside the library.".into()); }
            let parent = target.parent().unwrap();
            let dest = parent.join(name);
            if dest.exists() { return Err("The destination already exists.".into()); }
            let meta = parent.join("xalinfo");
            let mut d = Doc::parse(&std::fs::read_to_string(&meta).unwrap_or_else(|_| "<Folder/>".into()))?;
            let r = d.root().unwrap();
            let old = target.file_name().unwrap().to_string_lossy();
            for n in d.kids(r, "Node").filter(|&n| d.attr_or(n, "name") == old || d.attr_or(n, "name") == decode_name(&old)).collect::<Vec<_>>() { crate::edit::set_raw(&mut d, n, "name", name); }
            std::fs::rename(&target, &dest).map_err(|e| e.to_string())?;
            if let Err(e) = write(&meta, &d.write()) { let _ = std::fs::rename(&dest, &target); return Err(e); }
            list_in_xalinfo(parent, name)
        }
        "sort" => {
            let mut children = ordered_children(&target);
            children.sort_by_key(|p| decode_name(&p.file_name().unwrap().to_string_lossy()).to_lowercase());
            let body: String = children.iter().map(|p| format!("\n    <Node name=\"{}\" />", escape_attr(&p.file_name().unwrap().to_string_lossy()))).collect();
            write(&target.join("xalinfo"), &format!("<Folder expanded=\"no\">{body}\n</Folder>"))
        }
        _ => Err("Unknown library operation.".into()),
    }
}

/// Reads a library tree: every regular file (relative path, bytes) and folder; symbolic links and
/// special files are refused.
fn collect(dir: &Path, base: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>, dirs: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() { return Err("Symbolic links cannot be copied into libraries.".into()); }
        let rel = path.strip_prefix(base).map_err(|e| e.to_string())?.to_path_buf();
        if kind.is_dir() {dirs.push(rel); collect(&path, base, files, dirs)?;}
        else if kind.is_file() {files.push((rel, std::fs::read(path).map_err(|e| e.to_string())?));}
        else {return Err("Only regular library files can be copied.".into());}
    }
    Ok(())
}

/// Imports a legacy box library file (`.cbl`) into a new library folder `dir`. A `.cbl` is the
/// `.crg` container: the library tree, and its `.metadata` as the control file (library format 1
/// → archive format 7).
pub fn import_cbl(cbl: &[u8], dir: &Path) -> Result<(), String> {
    let fail = |e: String| format!("Could not import CBL file: {e}");
    let files = crate::crg::files(cbl).map_err(fail)?;
    let mut files: Vec<(String, Vec<u8>)> = files.into_iter().map(|(name, data)| crate::project::relative(&name).map(|n| (n, data))).collect::<Result<_, _>>().map_err(fail)?;
    // importDebianPackage writes `.metadata` from the control file, then extracts the data over it.
    if !files.iter().any(|f| f.0 == ".metadata") {
        files.push((".metadata".into(), crate::crg::metadata(cbl).map_err(fail)?.into_bytes()));
    }
    let meta = String::from_utf8_lossy(&files.iter().find(|f| f.0 == ".metadata").unwrap().1).into_owned();
    if !meta.lines().any(|l| l.trim() == "Type: ChoregrapheBoxLibrary") {
        return Err(fail("it is not a box library.".into()));
    }
    if dir.exists() && std::fs::read_dir(dir).map_err(|e| e.to_string())?.next().is_some() {
        return Err("The selected directory is not empty.".into());
    }
    let result = files.iter().try_for_each(|(name, data)| {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, data)).map_err(|e| format!("Cannot write {name}: {e}"))
    });
    if result.is_err() {
        // The folder was empty: remove what the import wrote, not the folder the user picked.
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
        }
    }
    result
}

/// A library folder as a `.cbl` ("Save library as… › Box library file").
pub fn export_cbl(dir: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::read_to_string(dir.join(".metadata")).map_err(|_| "This folder is not a box library (no .metadata).".to_string())?;
    let (mut files, mut dirs) = (vec![], vec![]);
    collect(dir, dir, &mut files, &mut dirs)?;
    // Like Choregraphe 2.8.8, `.metadata` also travels in the data; older readers take the control copy.
    let mut files: Vec<(String, Vec<u8>)> = files.into_iter().map(|(p, b)| (p.to_string_lossy().replace('\\', "/"), b)).collect();
    files.sort();
    crate::crg::archive(&files, &meta)
}

/// Copies a folder or definition with its attachments, without overwriting an existing entry.
pub fn copy_folder(source: &Path, library: &Path, folder: &str, name: &str) -> Result<(), String> {
    if is_install(library) { return Err("Selected box library is read only.".into()); }
    if name.is_empty() || name.starts_with('.') || name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*', '\n', '\r', '\0']) { return Err("Choose a valid entry name.".into()); }
    let root = library.canonicalize().map_err(|e| e.to_string())?;
    let parent = if folder.is_empty() {root.clone()} else {root.join(crate::project::relative(folder)?)};
    let mut checked = root.clone();
    for part in parent.strip_prefix(&root).map_err(|e| e.to_string())?.components() {
        checked.push(part);
        if checked.is_symlink() { return Err("Library destinations cannot contain symbolic links.".into()); }
    }
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if !parent.starts_with(&root) { return Err("Destination is outside the selected library.".into()); }
    if source.is_symlink() { return Err("Symbolic links cannot be copied into libraries.".into()); }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&source) { return Err("Cannot copy a folder into itself.".into()); }
    let dest = parent.join(name);
    // Read first: a failed or unsafe source cannot leave a partially copied entry.
    let mut files = Vec::new(); let mut dirs = Vec::new();
    collect(&source, &source, &mut files, &mut dirs)?;
    std::fs::create_dir(&dest).map_err(|e| format!("Cannot create {name} (existing entries are not overwritten): {e}"))?;
    let result = (|| {
        for dir in dirs {std::fs::create_dir_all(dest.join(dir)).map_err(|e| e.to_string())?;}
        for (path, bytes) in files {std::fs::write(dest.join(path), bytes).map_err(|e| e.to_string())?;}
        list_in_xalinfo(&parent, name)
    })();
    if result.is_err() {let _ = std::fs::remove_dir_all(&dest);}
    result
}
