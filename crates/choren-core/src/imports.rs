//! Project content imports are staged in memory; source files are never moved or overwritten.
use crate::edit::{self, History, NewBoxKind};
use crate::project::{FilePolicy, Format, Listing, Project};
use crate::xml::escape_attr;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn relative(path: &str) -> Result<&str, String> {
    if path.is_empty() || path.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' ']) || p.contains(['\\', '<', '>', ':', '"', '|', '?', '*']) || p.chars().any(char::is_control)) {
        return Err(format!("Invalid project path: {path}. Use a relative path without .. or special filename characters."));
    }
    Ok(path)
}

fn name(path: &Path) -> Result<&str, String> {
    let name = path.file_name().and_then(|s| s.to_str()).ok_or("The source has no valid filename.")?;
    relative(name)
}

fn collect(path: &Path, target: String, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    relative(&target)?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if meta.is_symlink() {
        return Err(format!("Symbolic links cannot be imported: {}", path.display()));
    }
    if meta.is_dir() {
        let mut entries = std::fs::read_dir(path).map_err(|e| e.to_string())?.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        entries.sort();
        for child in entries {
            collect(&child, format!("{target}/{}", name(&child)?), files)?;
        }
    } else if meta.is_file() {
        files.push((target, std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?));
    } else {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    Ok(())
}

/// Checks every target before any project mutation, including undeclared files and symlinks.
/// With `existing`, a target that is already a replaceable file (exact path) is allowed and returned.
fn available(p: &Project, paths: &[String], existing: bool) -> Result<BTreeSet<String>, String> {
    if p.format != Format::Pml {
        return Err("Imports need a .pml project. Use Save as project folder first.".into());
    }
    let root = p.root_dir().unwrap();
    let mut taken: BTreeSet<String> = p.files.iter().map(|f| f.path.to_lowercase()).chain(p.pending.keys().map(|s| s.to_lowercase())).collect();
    let mut found = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for rel in paths {
        relative(rel)?;
        let key = rel.to_lowercase();
        if !seen.insert(key.clone()) {
            return Err(format!("{rel} is selected more than once."));
        }
        // Behaviors, the PML and the manifest are documents held in memory; never overwrite them.
        let replaceable = existing && !p.files.iter().any(|f| f.path.to_lowercase() == key && matches!(f.kind, "behavior" | "project" | "manifest"));
        if taken.iter().any(|t| (*t == key && !replaceable) || t.starts_with(&format!("{key}/")) || key.starts_with(&format!("{t}/"))) {
            return Err(format!("{rel} already exists in the project. Choose a different destination."));
        }
        if taken.contains(&key) {
            if !p.files.iter().any(|f| f.path == *rel) && !p.pending.contains_key(rel) {
                return Err(format!("Use the existing file's exact capitalization for {rel}."));
            }
            found.insert(rel.clone());
        }
        let mut parent = root.clone();
        let parts: Vec<_> = rel.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            // Match case-insensitively on every platform so imports remain portable.
            let entry = if parent.is_dir() {
                std::fs::read_dir(&parent).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?.into_iter().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))
            } else { None };
            if let Some(entry) = entry {
                let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                let last = i == parts.len() - 1;
                if meta.is_symlink() || (!last && !meta.is_dir()) || (last && !(replaceable && meta.is_file())) {
                    return Err(format!("{rel} conflicts with an existing file, folder or symbolic link. Choose a different destination."));
                }
                if entry.file_name().to_str() != Some(part) {
                    return Err(format!("Use the existing name's exact capitalization: {}", entry.path().display()));
                }
                if last { found.insert(rel.clone()); }
            }
            parent.push(part);
        }
        taken.insert(key);
    }
    Ok(found)
}

fn listing(path: &str) -> Listing {
    Listing::new("Resources", format!("<File name=\"{}\" src=\"{}\" />", escape_attr(path.rsplit('/').next().unwrap()), escape_attr(path)), path, "resource")
}

/// What an imported file becomes, by Choregraphe's naming: `<folder>/behavior.xar` is a
/// behavior, `.dlg` a dialog, `.top` a topic (from its `topic:`/`language:` header), and
/// anything else a resource.
enum Content {
    Behavior,
    Listed(Listing),
}

fn content(path: &str, bytes: &[u8]) -> Result<Content, String> {
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let extension = file.rsplit_once('.').map(|(_, ext)| ext.to_lowercase()).unwrap_or_default();
    Ok(Content::Listed(match extension.as_str() {
        _ if file == "behavior.xar" => {
            if folder.is_empty() {
                return Err("A behavior.xar must go into its own folder. Choose a destination folder, or use Import behavior.".into());
            }
            crate::project::parse_xar(path, bytes).map_err(|e| format!("{path} is not a valid behavior: {e}"))?;
            return Ok(Content::Behavior);
        }
        "dlg" => Listing::new("Dialogs", format!("<Dialog name=\"{}\" src=\"{}\" />", escape_attr(stem), escape_attr(path)), path, "dialog"),
        "top" => match topic_header(bytes) {
            Some((topic, language)) => Listing::new("Topics", format!("<Topic name=\"{}\" src=\"{}\" topicName=\"{}\" language=\"{}\" />", escape_attr(stem), escape_attr(path), escape_attr(&topic), escape_attr(language)), path, "topic"),
            None => listing(path),
        },
        _ => listing(path),
    }))
}

/// A QiChat topic's name and robot language (ISO code) from its `topic: ~name()` and
/// `language: enu` lines.
fn topic_header(bytes: &[u8]) -> Option<(String, &'static str)> {
    let (mut topic, mut language) = (None, None);
    for line in std::str::from_utf8(bytes).ok()?.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("topic:") {
            topic = rest.trim().strip_prefix('~').and_then(|r| r.split('(').next()).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        } else if let Some(rest) = line.strip_prefix("language:") {
            language = crate::project::LANGUAGES.iter().find(|l| l.1 == rest.trim()).map(|l| l.0);
        }
        if topic.is_some() && language.is_some() { break; }
    }
    Some((topic?, language?))
}

impl Project {
    /// Files import into `destination`; folder import keeps the selected folder's name/tree and
    /// merges into existing folders. Without a `policy`, same-named files change nothing and are
    /// returned for the UI to ask; `Replace` overwrites them, `UseExisting` keeps them.
    /// Behaviors, dialogs and topics are listed as such (see `content`); everything else is a
    /// resource. Empty directories are not represented by the PML resource list.
    pub fn import_files(&mut self, h: &mut History, sources: &[PathBuf], destination: &str, folder: bool, policy: Option<FilePolicy>) -> Result<Vec<String>, String> {
        if sources.is_empty() || (folder && sources.len() != 1) {
            return Err("Select files or one folder to import.".into());
        }
        if !destination.is_empty() { relative(destination)?; }
        let mut files = vec![];
        for source in sources {
            let meta = std::fs::symlink_metadata(source).map_err(|e| e.to_string())?;
            if meta.is_dir() != folder { return Err("Select the requested file or folder type.".into()); }
            let target = if destination.is_empty() { name(source)?.to_string() } else { format!("{destination}/{}", name(source)?) };
            collect(source, target, &mut files)?;
        }
        if files.is_empty() { return Err("The selected folder contains no files to import.".into()); }
        let existing = available(self, &files.iter().map(|f| f.0.clone()).collect::<Vec<_>>(), true)?;
        match policy {
            _ if existing.is_empty() => {}
            None => return Ok(existing.into_iter().collect()),
            Some(FilePolicy::Replace) => {}
            Some(FilePolicy::UseExisting) => files.retain(|f| !existing.contains(&f.0)),
            Some(FilePolicy::Rename) => return Err("Renaming imported files is not supported.".into()),
        }
        let contents = files.iter().map(|(path, bytes)| content(path, bytes)).collect::<Result<Vec<_>, _>>()?;
        edit::change_project(self, h, if folder { "Import folder" } else { "Import files" }, |p| {
            for ((path, bytes), content) in files.into_iter().zip(contents) {
                let listed = p.files.iter().find(|f| f.path == path).map(|f| f.kind);
                let kind = match &content { Content::Behavior => "behavior", Content::Listed(l) => l.kind };
                // A replaced file that was listed as another kind (e.g. a behavior.xar imported
                // as a plain resource before) is listed again as what it is.
                if let Some(old) = listed.filter(|&old| old != kind) {
                    let section = match old { "resource" => "Resources", "dialog" => "Dialogs", "topic" => "Topics", _ => return Err(format!("{path} already exists in the project.")) };
                    p.list(&Listing::new(section, String::new(), &path, old), false)?;
                }
                match content {
                    Content::Behavior => {
                        let (folder, xar) = path.rsplit_once('/').unwrap();
                        p.list_behavior(folder, xar, &bytes, None)?;
                        p.pending.remove(&path);
                    }
                    Content::Listed(l) => {
                        if listed != Some(kind) { p.list(&l, true)?; }
                        p.stage(&path, Some(bytes));
                    }
                }
            }
            Ok(())
        })?;
        Ok(vec![])
    }

    /// Like New behavior, this changes project structure outside diagram undo history.
    /// CRG resources retain their project-relative names, preserving resource references.
    /// An existing behavior in `folder` or same-named resources are returned without a change
    /// unless `policy` is `Replace`, which overwrites them.
    pub fn import_behavior(&mut self, source: &Path, folder: &str, policy: Option<FilePolicy>) -> Result<Vec<String>, String> {
        relative(folder)?;
        if folder.contains('/') { return Err("Use a single folder name for the behavior.".into()); }
        let xar = format!("{folder}/behavior.xar");
        let existing = self.behaviors.iter().position(|b| b.source == xar);
        if existing.is_none() { available(self, &[folder.to_string()], false)?; }
        if !std::fs::symlink_metadata(source).map_err(|e| e.to_string())?.is_file() {
            return Err("Choose a regular behavior file; symbolic links cannot be imported.".into());
        }
        let extension = source.extension().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
        if !matches!(extension.as_str(), "xar" | "crg") {
            return Err("Choose a XAR file or a CRG archive containing exactly one behavior.".into());
        }
        let imported = Project::open(source)?;
        if imported.behaviors.len() != 1 {
            return Err("Choose a XAR file or a CRG archive containing exactly one behavior.".into());
        }
        let resources: Vec<_> = imported.archive.into_iter().filter(|(path, _)| !path.ends_with("manifest.xml")).collect();
        let mut targets: Vec<_> = if existing.is_none() { vec![xar.clone()] } else { vec![] };
        targets.extend(resources.iter().map(|f| f.0.clone()));
        let mut conflicts: Vec<_> = available(self, &targets, true)?.into_iter().collect();
        if existing.is_some() { conflicts.insert(0, xar); }
        match policy {
            _ if conflicts.is_empty() => {}
            None => return Ok(conflicts),
            Some(FilePolicy::Replace) => {}
            Some(_) => return Err("An imported behavior can only replace existing content.".into()),
        }
        // Build the structure on a copy so a malformed document cannot leave a partial import.
        let mut next = self.clone();
        let doc = imported.behaviors.into_iter().next().unwrap().doc;
        match existing {
            Some(i) => next.behaviors[i].doc = doc,
            None => {
                next.add_behavior(folder, NewBoxKind::Diagram)?;
                next.behaviors.last_mut().unwrap().doc = doc;
            }
        }
        for (path, bytes) in resources {
            if !next.files.iter().any(|f| f.path == path) { next.list(&listing(&path), true)?; }
            next.pending.insert(path, bytes);
        }
        *self = next;
        Ok(vec![])
    }
}
