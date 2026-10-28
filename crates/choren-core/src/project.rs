//! Opening and saving projects: `.pml` project folders, legacy `.crg` archives and loose `.xar` files.

use crate::view::{Diagnostic, ProjectFile};
use crate::xml::{Doc, NodeId};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pml,
    Crg,
    Xar,
}

#[derive(Clone, Debug)]
pub struct Behavior {
    pub id: String,
    pub name: String,
    /// Path relative to the project root (or the archive member name).
    pub source: String,
    pub doc: Doc,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub source: String,
    pub format: Format,
    pub name: String,
    pub behaviors: Vec<Behavior>,
    pub files: Vec<ProjectFile>,
    pub metadata: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    pub pml: Option<Doc>,
    /// Manifest path and document (PML projects).
    pub manifest: Option<(String, Doc)>,
    /// Files staged for the next save (relative path → bytes), e.g. library attachments.
    pub pending: BTreeMap<String, Vec<u8>>,
    /// Files deleted from disk on the next save (relative paths), e.g. a removed behavior's folder.
    pub deleted: BTreeSet<String>,
    /// The PML/manifest or the behavior list changed outside the undo history.
    pub structure_dirty: bool,
    /// Non-behavior files of a `.crg` archive (relative path → bytes), kept for save-as-folder.
    pub archive: BTreeMap<String, Vec<u8>>,
}

/// Robot languages (`share/choregraphe/robotlanguages`: ISO code → ASR embedded code). Topic
/// files are named `<topic>_<code>.top`.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("ar_SA", "arw"), ("pt_BR", "ptb"), ("zh_CN", "mnc"), ("cs_CZ", "czc"), ("da_DK", "dad"), ("nl_NL", "dun"), ("en_US", "enu"), ("fi_FI", "fif"),
    ("fr_FR", "frf"), ("de_DE", "ged"), ("el_GR", "grg"), ("it_IT", "iti"), ("ja_JP", "jpj"), ("ko_KR", "kok"), ("zh_TW", "mnt"), ("nn_NO", "nor"),
    ("pl_PL", "plp"), ("pt_PT", "ptp"), ("ru_RU", "rur"), ("es_ES", "spe"), ("sv_SE", "sws"), ("tr_TR", "trt"),
];

pub fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// Imported references may only read within the folder the user opened.
pub fn relative(path: &str) -> Result<String, String> {
    let p = normalize(path);
    let mut p = p.as_str();
    while let Some(rest) = p.strip_prefix("./") { p = rest; }
    let windows_abs = p.as_bytes().get(1) == Some(&b':');
    if p.is_empty() || p.starts_with('/') || windows_abs || p.split('/').any(|s| s == ".." || s.is_empty() || s == "." || s.contains(':') || s.contains('\0')) {
        return Err(format!("Reference is outside the project folder: {path}"));
    }
    Ok(p.to_string())
}

/// `path`, relative to the folder of the behavior file `behavior`, as a project path. Choregraphe
/// stores a Dialog box's `dialogFile` this way.
pub fn from_behavior(behavior: &str, path: &str) -> Result<String, String> {
    let behavior = normalize(behavior);
    let mut dir: Vec<&str> = behavior.rsplit_once('/').map_or(vec![], |(d, _)| d.split('/').collect());
    let path = normalize(path);
    let mut rest = path.as_str();
    while let Some(r) = rest.strip_prefix("../") {
        dir.pop().ok_or_else(|| format!("Reference is outside the project folder: {path}"))?;
        rest = r;
    }
    dir.push(rest);
    relative(&dir.join("/"))
}

/// Parses a `.xar`, refusing one without the root `Box` the editor and the runtime rely on.
pub fn parse_xar(name: &str, bytes: &[u8]) -> Result<Doc, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{name} is not UTF-8"))?;
    let doc = Doc::parse(text).map_err(|e| format!("{name}: {e}"))?;
    let root = doc.root().unwrap();
    if !matches!(doc.name(root), "ChoregrapheProject" | "ChoregrapheBox") || doc.kid(root, "Box").is_none() {
        return Err(format!("{name}: not a Choregraphe XAR file (no root Box)"));
    }
    Ok(doc)
}

impl Project {
    pub fn open(path: &Path) -> Result<Project, String> {
        let source = normalize(&path.to_string_lossy());
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let format = match ext.as_str() {
            "pml" => Format::Pml,
            "crg" => Format::Crg,
            "xar" => Format::Xar,
            _ => return Err(format!("Unsupported file type: .{ext}")),
        };
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {source}: {e}"))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut p = Project { source: source.clone(), format, name: stem, behaviors: vec![], files: vec![], metadata: BTreeMap::new(), diagnostics: vec![], pml: None, manifest: None, pending: BTreeMap::new(), deleted: BTreeSet::new(), structure_dirty: false, archive: BTreeMap::new() };
        match format {
            Format::Xar => {
                let file = path.file_name().unwrap().to_string_lossy().to_string();
                p.push_behavior(p.name.clone(), file, &bytes)?;
            }
            Format::Crg => {
                for (raw, data) in crate::crg::files(&bytes)? {
                    let file = relative(&raw)?;
                    if file.ends_with(".xar") {
                        let name = file.trim_end_matches("/behavior.xar").to_string();
                        p.push_behavior(name, file, &data)?;
                    } else {
                        let kind = if file.ends_with("manifest.xml") { "manifest" } else { "resource" };
                        p.files.push(ProjectFile::new(file.clone(), kind));
                        p.archive.insert(file, data);
                    }
                }
            }
            Format::Pml => p.open_pml(path, &bytes)?,
        }
        Ok(p)
    }

    fn push_behavior(&mut self, name: String, file: String, bytes: &[u8]) -> Result<(), String> {
        let id = format!("{}::{file}", self.source);
        self.behaviors.push(Behavior { id: id.clone(), name, source: file.clone(), doc: parse_xar(&file, bytes)? });
        self.files.push(ProjectFile { behavior_id: Some(id), ..ProjectFile::new(file, "behavior") });
        Ok(())
    }

    fn open_pml(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new("."));
        let pml = Doc::parse(std::str::from_utf8(bytes).map_err(|_| "Project file is not UTF-8")?)?;
        let pkg = pml.root().unwrap();
        if pml.name(pkg) != "Package" {
            return Err("Not a Choregraphe project (no Package)".into());
        }
        if let Some(n) = pml.attr(pkg, "name").filter(|n| !n.is_empty()) {
            self.name = n;
        }
        self.metadata.insert("Format version".into(), pml.attr(pkg, "format_version").unwrap_or("Unknown".into()));
        self.files.push(ProjectFile::new(path.file_name().unwrap().to_string_lossy().to_string(), "project"));
        if let Some(sec) = pml.kid(pkg, "BehaviorDescriptions") {
            for d in pml.kids(sec, "BehaviorDescription") {
                let src = pml.attr_or(d, "src");
                let file = relative(&format!("{src}/{}", pml.attr_or(d, "xar")))?;
                let data = std::fs::read(dir.join(&file)).map_err(|e| format!("Cannot read {file}: {e}"))?;
                self.push_behavior(src, file, &data)?;
            }
        }
        for (section, tag, kind) in [("Resources", "File", "resource"), ("Dialogs", "Dialog", "dialog"), ("Topics", "Topic", "topic"), ("Translations", "Translation", "translation")] {
            if let Some(sec) = pml.kid(pkg, section) {
                for item in pml.kids(sec, tag) {
                    let file = relative(&pml.attr_or(item, "src"))?;
                    let missing = !dir.join(&file).exists();
                    if missing {
                        self.diagnostics.push(Diagnostic::warning(&file, "Referenced file is missing or inaccessible."));
                    }
                    self.files.push(ProjectFile { missing: Some(missing), ..ProjectFile::new(file, kind) });
                }
            }
        }
        if let Some(m) = pml.kid(pkg, "Manifest").and_then(|m| pml.attr(m, "src")) {
            let file = relative(&m)?;
            let manifest = std::fs::read_to_string(dir.join(&file)).ok().and_then(|s| Doc::parse(&s).ok());
            match manifest {
                Some(md) => {
                    let root = md.root().unwrap();
                    for (key, attr) in [("Version", "version"), ("UUID", "uuid")] {
                        if let Some(v) = md.attr(root, attr) {
                            self.metadata.insert(key.into(), v);
                        }
                    }
                    let names: Vec<_> = md.kid(root, "names").map(|n| md.kids(n, "name").collect()).unwrap_or_default();
                    let title = names.iter().find(|&&n| md.attr_or(n, "lang").starts_with("en")).or(names.first());
                    if let Some(&t) = title {
                        self.metadata.insert("Title".into(), md.text(t));
                    }
                    self.files.push(ProjectFile::new(file.clone(), "manifest"));
                    self.manifest = Some((file, md));
                }
                None => {
                    self.diagnostics.push(Diagnostic::warning(&file, "Project manifest is missing or unreadable."));
                    self.files.push(ProjectFile { missing: Some(true), ..ProjectFile::new(file, "manifest") });
                }
            }
        }
        self.pml = Some(pml);
        Ok(())
    }

    /// Folder that project-relative paths resolve against (none for read-only `.crg`).
    pub fn root_dir(&self) -> Option<PathBuf> {
        (self.format != Format::Crg).then(|| Path::new(&self.source).parent().unwrap_or(Path::new(".")).to_path_buf())
    }

    /// Writes behaviors, staged files, and the PML/manifest when they changed. Unmodified
    /// documents are written byte for byte; each file is replaced atomically.
    pub fn save(&mut self) -> Result<(), String> {
        // shortcut: CRG archives are import-only; saving one needs "save as project folder".
        let dir = self.root_dir().ok_or("Legacy .crg archives are read-only. Use File › Save as project folder…")?;
        let put = |rel: &str, bytes: &[u8]| -> Result<(), String> {
            let target = dir.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
            }
            // Imports can contain any filename, including the old fixed temporary suffix.
            // Reserve a fresh sibling so saving never clobbers a different project file.
            let write = || -> std::io::Result<()> {
                use std::io::Write;
                for index in 0u64.. {
                    let tmp = target.with_file_name(format!(".choren-save-{}-{index}", std::process::id()));
                    let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
                        Ok(file) => file,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => return Err(e),
                    };
                    let result = file.write_all(bytes);
                    drop(file);
                    let result = result.and_then(|_| std::fs::rename(&tmp, &target));
                    if result.is_err() { let _ = std::fs::remove_file(&tmp); }
                    return result;
                }
                unreachable!()
            };
            write().map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("Cannot save {}: {e}. Check write permissions for the file and its containing folder (saving creates and replaces a temporary file there). Your edits have not been discarded.", target.display())
                } else {
                    format!("Cannot save {rel}: {e}")
                }
            })
        };
        for b in &self.behaviors {
            put(&b.source, b.doc.write().as_bytes())?;
        }
        for (rel, bytes) in &self.pending {
            put(rel, bytes)?;
        }
        let mut folders = BTreeSet::new();
        for rel in self.deleted.clone() {
            if !self.listed(&rel) {
                match std::fs::remove_file(dir.join(&rel)) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("Cannot delete {rel}: {e}")),
                    _ => {}
                }
                folders.extend(Path::new(&rel).ancestors().skip(1).filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf));
            }
            self.deleted.remove(&rel);
        }
        // Deepest first; folders that still hold other files stay.
        for folder in folders.iter().rev() {
            let _ = std::fs::remove_dir(dir.join(folder));
        }
        if self.structure_dirty {
            if let Some(pml) = &self.pml {
                let name = Path::new(&self.source).file_name().unwrap().to_string_lossy().to_string();
                put(&name, pml.write().as_bytes())?;
            }
            if let Some((rel, m)) = &self.manifest {
                put(rel, m.write().as_bytes())?;
            }
        }
        self.pending.clear();
        self.structure_dirty = false;
        Ok(())
    }

    pub fn state(&self, h: &crate::edit::History, created: Vec<usize>) -> crate::edit::HistoryState {
        let mut s = h.state(created);
        s.dirty |= self.structure_dirty || !self.pending.is_empty() || !self.deleted.is_empty();
        s
    }

    fn pml_section(&mut self, tag: &str) -> Result<usize, String> {
        let pml = self.pml.as_mut().ok_or("This operation needs a .pml project")?;
        let pkg = pml.root().unwrap();
        match pml.kid(pkg, tag) {
            Some(n) => Ok(n),
            None => crate::edit::insert_raw(pml, pkg, &format!("<{tag} />")),
        }
    }

    /// Adds (or removes) a PML entry and its row in the file list.
    pub fn list(&mut self, l: &Listing, add: bool) -> Result<(), String> {
        if add {
            let sec = self.pml_section(l.section)?;
            crate::edit::insert_raw(self.pml.as_mut().unwrap(), sec, &l.xml)?;
            self.files.push(ProjectFile { missing: Some(false), ..ProjectFile::new(l.path.clone(), l.kind) });
        } else {
            let pml = self.pml.as_mut().ok_or("This operation needs a .pml project")?;
            if let Some(sec) = pml.kid(pml.root().unwrap(), l.section) {
                let doomed: Vec<_> = pml.el(sec).children.iter().copied().filter(|&n| matches!(pml.nodes[n].kind, crate::xml::Kind::Element(_)) && pml.attr(n, "src").as_deref() == Some(&l.path)).collect();
                doomed.into_iter().for_each(|n| crate::edit::detach_raw(pml, n));
            }
            self.files.retain(|f| !(f.path == l.path && f.kind == l.kind));
        }
        self.structure_dirty = true;
        Ok(())
    }

    /// Inserts a library box into a diagram (one undo step) and stages its attached files at the
    /// project root, where its resource parameters (`/file.ogg`) point. With conflicts and no
    /// policy, nothing changes and the conflicting paths are returned for the UI to resolve.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_library_box(&mut self, h: &mut crate::edit::History, behavior: &str, diagram: usize, box_file: &Path, x: f64, y: f64, policy: Option<FilePolicy>) -> Result<Inserted, String> {
        let (_, mut xml) = crate::library::box_xml(box_file)?;
        let files = crate::library::attached_files(box_file);
        let mut stage = vec![];
        if !files.is_empty() {
            let dir = self.root_dir().ok_or("Attached files need a project folder; this .crg archive is read-only.")?;
            // Each file is imported next to the behavior,
            // so the box's own relative paths (`dialogFile`, resource parameters) stay valid.
            let folder = self.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?.source.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default();
            let taken = |rel: &str, me: &Project| dir.join(format!("{folder}{rel}")).exists() || me.pending.contains_key(&format!("{folder}{rel}"));
            let conflicts: Vec<String> = files.iter().filter(|f| taken(f, self)).cloned().collect();
            if !conflicts.is_empty() && policy.is_none() {
                return Ok(Inserted::Conflicts(conflicts.iter().map(|f| format!("{folder}{f}")).collect()));
            }
            for f in &files {
                let mut target = f.clone();
                if conflicts.contains(f) {
                    match policy.unwrap() {
                        FilePolicy::UseExisting => continue,
                        FilePolicy::Replace => {}
                        FilePolicy::Rename => {
                            let (stem, ext) = f.rsplit_once('.').map_or((f.as_str(), String::new()), |(a, b)| (a, format!(".{b}")));
                            target = (1..).map(|i| format!("{stem}_{i}{ext}")).find(|t| !taken(t, self)).unwrap();
                            // point `dialogFile` and `/`-prefixed resource values at the renamed file
                            xml = xml.replace(&format!("=\"/{}\"", crate::xml::escape_attr(f)), &format!("=\"/{}\"", crate::xml::escape_attr(&target)));
                            xml = xml.replace(&format!("<dialogFile>{}</dialogFile>", crate::xml::escape_text(f)), &format!("<dialogFile>{}</dialogFile>", crate::xml::escape_text(&target)));
                        }
                    }
                }
                let bytes = std::fs::read(box_file.parent().unwrap().join(f)).map_err(|e| format!("Cannot read attached file {f}: {e}"))?;
                stage.push((format!("{folder}{target}"), bytes));
            }
        }
        let state = crate::edit::apply(self, h, behavior, "Add a new box", vec![crate::edit::Command::AddBox { diagram, xml, x, y }])?;
        // Attachments belong to the same undo step: undoing the box restores the previous files.
        let changes = stage.iter().map(|(rel, bytes)| (rel.clone(), self.read_file(rel), Some(bytes.clone()))).collect();
        let listings = stage
            .iter()
            .filter(|(rel, _)| self.pml.is_some() && !self.files.iter().any(|f| &f.path == rel))
            .map(|(rel, _)| {
                let name = Path::new(rel).file_stem().unwrap().to_string_lossy().to_string();
                Listing::new("Resources", format!("<File name=\"{}\" src=\"{}\" />", crate::xml::escape_attr(&name), crate::xml::escape_attr(rel)), rel, "resource")
            })
            .collect();
        crate::edit::add_files_to_last(self, h, changes, listings)?;
        Ok(Inserted::Done(self.state(h, state.created)))
    }

    /// Creates a new project folder (`<dir>/<name>.pml`, `manifest.xml`, `behavior_1/behavior.xar`) on disk.
    pub fn create(dir: &Path, name: &str) -> Result<Project, String> {
        let xar = crate::templates::behavior(crate::edit::NewBoxKind::Diagram);
        write_folder(dir, name, &[("behavior_1".into(), xar.into_bytes())], &[])?;
        Project::open(&dir.join(format!("{name}.pml")))
    }

    /// Writes a `.crg` or loose `.xar` project as a project folder and returns the new `.pml`
    /// path. Behaviors keep their current (possibly unsaved) contents; archive files keep their
    /// relative paths, which is where box resource parameters (`/file.ogg`) point.
    pub fn save_as_folder(&self, dir: &Path, name: &str) -> Result<PathBuf, String> {
        if self.format == Format::Pml {
            if name.is_empty() || name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*']) || name == "." || name == ".." {
                return Err("Choose a valid project name.".into());
            }
            if !dir.is_dir() || std::fs::read_dir(dir).map_err(|e| e.to_string())?.next().is_some() {
                return Err("Choose an empty folder. Existing files will not be overwritten.".into());
            }
            let mut files = BTreeMap::new();
            for f in &self.files {
                if !matches!(f.kind, "project" | "behavior" | "manifest") {
                    let rel = relative(&f.path)?;
                    let bytes = self.read_file(&rel).ok_or_else(|| format!("Cannot copy missing file {rel}"))?;
                    files.insert(rel, bytes);
                }
            }
            files.extend(self.pending.clone());
            for b in &self.behaviors { files.insert(relative(&b.source)?, b.doc.write().into_bytes()); }
            if let Some((rel, m)) = &self.manifest { files.insert(relative(rel)?, m.write().into_bytes()); }
            let mut pml = self.pml.clone().ok_or("Project has no PML document")?;
            let root = pml.root().unwrap();
            crate::edit::set_raw(&mut pml, root, "name", name);
            let project_name = format!("{name}.pml");
            if files.contains_key(&project_name) { return Err("Project name conflicts with a resource.".into()); }
            files.insert(project_name.clone(), pml.write().into_bytes());
            // All inputs are read before writing, so missing resources cannot leave a partial copy.
            for (rel, bytes) in files {
                let target = dir.join(relative(&rel)?);
                if let Some(parent) = target.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                use std::io::Write;
                std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                    .and_then(|mut file| file.write_all(&bytes)).map_err(|e| format!("Cannot copy {rel}: {e}"))?;
            }
            return Ok(dir.join(project_name));
        }
        let mut behaviors = vec![];
        for (i, b) in self.behaviors.iter().enumerate() {
            let folder = match b.source.rsplit_once('/') {
                Some((folder, _)) => folder.to_string(),
                None => format!("behavior_{}", i + 1),
            };
            behaviors.push((folder, b.doc.write().into_bytes()));
        }
        let mut files: Vec<(String, Vec<u8>)> = self.archive.iter().filter(|(k, _)| !k.ends_with("manifest.xml")).map(|(k, v)| (k.clone(), v.clone())).collect();
        files.extend(self.pending.iter().map(|(k, v)| (k.clone(), v.clone())));
        write_folder(dir, name, &behaviors, &files)?;
        Ok(dir.join(format!("{name}.pml")))
    }

    /// A robot package contains the manifest at its root, current behaviors and listed assets.
    pub fn package_files(&self) -> Result<BTreeMap<String, Vec<u8>>, String> {
        let manifest = self.manifest_fields().ok_or("Build a package from a PML project with a manifest.")?;
        let mut validated = self.clone();
        validated.set_manifest_fields(&manifest)?;
        let mut files = BTreeMap::new();
        for f in self.files.iter().filter(|f| !matches!(f.kind, "project" | "manifest" | "behavior" | "translation")) {
            let path = relative(&f.path)?;
            files.insert(path.clone(), self.read_file(&path).ok_or_else(|| format!("Package resource is missing: {path}"))?);
        }
        // Like Choregraphe: the supported languages' `.ts` go through lrelease and only the `.qm` ships.
        if let Some(pml) = &self.pml {
            for t in pml.kid(pml.root().unwrap(), "Translations").into_iter().flat_map(|s| pml.kids(s, "Translation")) {
                let lang = pml.attr_or(t, "language");
                if manifest.supported_languages.contains(&lang) {
                    let src = relative(&pml.attr_or(t, "src"))?;
                    let ts = self.read_file(&src).ok_or_else(|| format!("Package resource is missing: {src}"))?;
                    files.insert(relative(&format!("translations/translation_{lang}.qm"))?, crate::translations::compile(&ts)?);
                }
            }
        }
        for b in &self.behaviors { files.insert(relative(&b.source)?, b.doc.write().into_bytes()); }
        if files.contains_key("manifest.xml") { return Err("A resource conflicts with the package manifest.".into()); }
        files.insert("manifest.xml".into(), self.manifest.as_ref().unwrap().1.write().into_bytes());
        Ok(files)
    }

    /// The application package (`.pkg`) of the current, possibly unsaved, project.
    pub fn package(&self) -> Result<Vec<u8>, String> {
        Ok(crate::pkg::zip(&self.package_files()?))
    }

    /// Reads a project file: staged contents first, then the archive, then the project folder.
    pub fn read_file(&self, rel: &str) -> Option<Vec<u8>> {
        if self.deleted.contains(rel) { return None; }
        self.pending.get(rel).or_else(|| self.archive.get(rel)).cloned().or_else(|| std::fs::read(self.root_dir()?.join(rel)).ok())
    }

    /// Stages `content` for `rel` (unstaged when it equals the file on disk or is `None`).
    pub fn stage(&mut self, rel: &str, content: Option<Vec<u8>>) {
        let on_disk = self.root_dir().and_then(|d| std::fs::read(d.join(rel)).ok());
        match content {
            Some(c) if Some(&c) != on_disk.as_ref() => {
                self.deleted.remove(rel);
                self.pending.insert(rel.to_string(), c);
            }
            Some(_) => {
                self.deleted.remove(rel);
                self.pending.remove(rel);
            }
            None => {
                self.pending.remove(rel);
            }
        }
    }

    /// Adds a QiChat dialog: `<name>/<name>.dlg` plus one `<name>_<code>.top` per language,
    /// listed under the PML's `<Dialogs>` and `<Topics>`. The files are one undo step.
    pub fn add_dialog(&mut self, h: &mut crate::edit::History, name: &str, languages: &[String]) -> Result<crate::edit::HistoryState, String> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("A topic name may only contain letters, numbers and underscores.".into());
        }
        if languages.is_empty() {
            return Err("Select at least one language.".into());
        }
        let dlg = format!("{name}/{name}.dlg");
        if self.read_file(&dlg).is_some() || self.files.iter().any(|f| f.path.starts_with(&format!("{name}/"))) {
            return Err(format!("A dialog named \"{name}\" already exists."));
        }
        let mut text = format!("multilanguage:{name}\n");
        let mut files = vec![];
        let e = crate::xml::escape_attr;
        let mut topics = vec![];
        for lang in languages {
            let code = LANGUAGES.iter().find(|l| l.0 == lang).ok_or_else(|| format!("Unknown language {lang}"))?.1;
            let top = format!("{name}/{name}_{code}.top");
            text += &format!("  {code}:{name}_{code}.top\n");
            files.push((top.clone(), format!("topic: ~{name}()\nlanguage: {code}\n\n").into_bytes()));
            topics.push(Listing::new("Topics", format!("<Topic name=\"{}\" src=\"{}\" topicName=\"{}\" language=\"{}\" />", e(&format!("{name}_{code}")), e(&top), e(name), e(lang)), &top, "topic"));
        }
        files.insert(0, (dlg.clone(), text.into_bytes()));
        self.pml.as_ref().ok_or("This operation needs a .pml project")?;
        let mut listings = vec![Listing::new("Dialogs", format!("<Dialog name=\"{}\" src=\"{}\" />", e(name), e(&dlg)), &dlg, "dialog")];
        listings.extend(topics);
        crate::edit::set_listed_files(self, h, &format!("Add dialog {name}"), files, listings)
    }

    /// Exports one behavior and the project's resource files as a legacy `.crg` archive.
    pub fn export_crg(&self, behavior: &str) -> Result<Vec<u8>, String> {
        let b = self.behaviors.iter().find(|b| b.id == behavior).ok_or("Unknown behavior")?;
        let mut files = vec![("behavior.xar".to_string(), b.doc.write().into_bytes())];
        let resources = self.files.iter().filter(|f| f.kind == "resource").map(|f| f.path.clone());
        for path in resources.chain(self.pending.keys().cloned()) {
            if !files.iter().any(|f| f.0 == path) {
                if let Some(bytes) = self.read_file(&path) {
                    files.push((path, bytes));
                }
            }
        }
        crate::crg::write(&files, &b.name)
    }

    /// Adds a behavior folder with a Diagram or Timeline root (written on save).
    pub fn add_behavior(&mut self, name: &str, root: crate::edit::NewBoxKind) -> Result<String, String> {
        let dir = self.root_dir().ok_or("This operation needs a .pml project")?;
        let folder = if name.is_empty() {
            (1..).map(|i| format!("behavior_{i}")).find(|f| !dir.join(f).exists() && !self.behaviors.iter().any(|b| b.name == *f)).unwrap()
        } else {
            name.to_string()
        };
        crate::project::relative(&folder)?;
        if folder.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*']) || dir.join(&folder).exists() || self.behaviors.iter().any(|b| b.name == folder) {
            return Err(format!("A behavior named \"{folder}\" already exists or the name is invalid."));
        }
        let sec = self.pml_section("BehaviorDescriptions")?;
        crate::edit::insert_raw(self.pml.as_mut().unwrap(), sec, &format!("<BehaviorDescription name=\"behavior\" src=\"{}\" xar=\"behavior.xar\" />", crate::xml::escape_attr(&folder)))?;
        if let Some((_, m)) = self.manifest.as_mut() {
            let root = m.root().unwrap();
            let contents = match m.kid(root, "contents") { Some(c) => c, None => crate::edit::insert_raw(m, root, "<contents />")? };
            crate::edit::insert_raw(m, contents, &format!("<behaviorContent path=\"{}\">\n    <nature>interactive</nature>\n    <permissions/>\n</behaviorContent>", crate::xml::escape_attr(&folder)))?;
        }
        let file = format!("{folder}/behavior.xar");
        self.push_behavior(folder, file, crate::templates::behavior(root).as_bytes())?;
        self.structure_dirty = true;
        Ok(self.behaviors.last().unwrap().id.clone())
    }

    /// Removes a behavior from the project lists. Its files stay on disk (never deleted silently).
    pub fn remove_behavior(&mut self, h: &mut crate::edit::History, id: &str) -> Result<(), String> {
        let i = self.behaviors.iter().position(|b| b.id == id).ok_or("Unknown behavior")?;
        let folder = self.behaviors[i].source.trim_end_matches("/behavior.xar").to_string();
        if let Some(pml) = self.pml.as_mut() {
            let pkg = pml.root().unwrap();
            if let Some(sec) = pml.kid(pkg, "BehaviorDescriptions") {
                for d in pml.kids(sec, "BehaviorDescription").filter(|&d| pml.attr(d, "src").as_deref() == Some(&folder)).collect::<Vec<_>>() {
                    crate::edit::detach_raw(pml, d);
                }
            }
        }
        if let Some((_, m)) = self.manifest.as_mut() {
            let root = m.root().unwrap();
            if let Some(c) = m.kid(root, "contents") {
                for d in m.kids(c, "behaviorContent").filter(|&d| m.attr(d, "path").as_deref() == Some(&folder)).collect::<Vec<_>>() {
                    crate::edit::detach_raw(m, d);
                }
            }
        }
        let b = self.behaviors.remove(i);
        h.forget(&b.id);
        self.files.retain(|f| f.behavior_id.as_deref() != Some(&b.id));
        self.structure_dirty = true;
        Ok(())
    }

    /// Removes everything listed under `folder`: behaviors, resources, dialogs, topics and
    /// translations. Files stay on disk unless `delete_path` marks them too.
    pub fn remove_folder(&mut self, h: &mut crate::edit::History, folder: &str) -> Result<(), String> {
        let folder = relative(folder)?;
        let inside = |path: &str| path.strip_prefix(folder.as_str()).is_some_and(|rest| rest.starts_with('/'));
        if self.files.iter().any(|f| matches!(f.kind, "project" | "manifest") && inside(&f.path)) {
            return Err("This folder holds the project file or its manifest.".into());
        }
        for id in self.behaviors.iter().filter(|b| inside(&b.source)).map(|b| b.id.clone()).collect::<Vec<_>>() {
            self.remove_behavior(h, &id)?;
        }
        for f in self.files.iter().filter(|f| inside(&f.path)).cloned().collect::<Vec<_>>() {
            let section = match f.kind { "resource" => "Resources", "dialog" => "Dialogs", "topic" => "Topics", "translation" => "Translations", _ => continue };
            self.list(&Listing::new(section, String::new(), &f.path, f.kind), false)?;
            self.pending.remove(&f.path);
        }
        Ok(())
    }

    /// Marks `rel` (a file or a folder) for deletion on the next save. Files the project still
    /// lists are kept, so remove their listings first.
    pub fn delete_path(&mut self, rel: &str) -> Result<(), String> {
        let rel = relative(rel)?;
        let dir = self.root_dir().ok_or("Deleting files needs a project folder")?;
        let mut checked = dir.clone();
        for component in rel.split('/').collect::<Vec<_>>().split_last().unwrap().1 {
            checked.push(component);
            if checked.is_symlink() { return Err("Cannot delete files through a symbolic link.".into()); }
        }
        let mut found = vec![];
        collect_files(&dir, &rel, &mut found);
        found.extend(self.pending.keys().filter(|k| *k == &rel || k.starts_with(&format!("{rel}/"))).cloned());
        for path in found {
            if self.listed(&path) { continue; }
            self.pending.remove(&path);
            self.deleted.insert(path);
        }
        Ok(())
    }

    /// The files `delete_path` would add (and a following save delete), without changing anything.
    pub fn deletion_preview(&self, work: impl FnOnce(&mut Project) -> Result<(), String>) -> Result<Vec<String>, String> {
        let mut after = self.clone();
        work(&mut after)?;
        Ok(after.deleted.difference(&self.deleted).cloned().collect())
    }

    fn listed(&self, path: &str) -> bool {
        self.files.iter().any(|f| f.path == path) || self.behaviors.iter().any(|b| b.source == path)
    }

    /// Renames a listed resource and its structured box references. Old files stay on disk.
    pub fn rename_resource(&mut self, path: &str, target: &str) -> Result<(), String> {
        let target = relative(target)?;
        if !self.files.iter().any(|f| f.path == path && f.kind == "resource") { return Err("Select a resource file.".into()); }
        if self.files.iter().any(|f| f.path == target) || self.read_file(&target).is_some() || self.root_dir().is_some_and(|dir| dir.join(&target).exists()) {
            return Err(format!("A file already exists at {target}"));
        }
        if let Some(root) = self.root_dir() {
            let mut checked = root;
            for component in target.split('/') {
                checked.push(component);
                if checked.is_symlink() { return Err("Resource destinations cannot contain symbolic links.".into()); }
            }
        }
        let bytes = self.read_file(path).ok_or("The resource is missing")?;
        let pml = self.pml.as_mut().ok_or("Resources need a project folder")?;
        if let Some(sec) = pml.kid(pml.root().unwrap(), "Resources") {
            for n in pml.kids(sec, "File").filter(|&n| pml.attr(n, "src").as_deref() == Some(path)).collect::<Vec<_>>() {
                crate::edit::set_raw(pml, n, "src", &target);
            }
        }
        for behavior in &mut self.behaviors {
            let d = &mut behavior.doc;
            for n in 0..d.nodes.len() {
                if d.nodes[n].parent.is_none() { continue; }
                if d.is_el(n, "Parameter") && d.attr_raw(n, "content_type") == Some("4") {
                    for attr in ["value", "default_value"] {
                        if let Some(value) = d.attr(n, attr) {
                            if value == path || value == format!("/{path}") {
                                crate::edit::set_raw(d, n, attr, &format!("{}{target}", if value.starts_with('/') { "/" } else { "" }));
                            }
                        }
                    }
                }
                if d.is_el(n, "bitmap") && (d.text(n) == path || d.text(n) == format!("/{path}")) {
                    let value = format!("{}{target}", if d.text(n).starts_with('/') { "/" } else { "" });
                    for child in d.el(n).children.clone() { d.nodes[child].parent = None; }
                    let text = d.new_text(crate::xml::escape_text(&value));
                    d.nodes[text].parent = Some(n);
                    d.el_mut(n).children = vec![text];
                }
            }
        }
        for f in &mut self.files { if f.path == path { f.path = target.clone(); } }
        self.pending.remove(path);
        self.stage(&target, Some(bytes));
        self.structure_dirty = true;
        Ok(())
    }

    pub fn remove_resource(&mut self, path: &str) -> Result<(), String> {
        if !self.files.iter().any(|f| f.path == path && f.kind == "resource") { return Err("Select a resource file.".into()); }
        self.list(&Listing::new("Resources", String::new(), path, "resource"), false)?;
        self.pending.remove(path);
        Ok(())
    }

    /// Reads the editable manifest fields.
    pub fn manifest_fields(&self) -> Option<Manifest> {
        let (_, m) = self.manifest.as_ref()?;
        let root = m.root().unwrap();
        let langs = |tag: &str| m.kid(root, tag).map(|s| m.kids(s, "language").map(|l| m.text(l)).collect()).unwrap_or_default();
        let names = m.kid(root, "names").map(|n| m.kids(n, "name").map(|x| (m.attr_or(x, "lang"), m.text(x))).collect()).unwrap_or_default();
        let descriptions = m.kid(root, "descriptions").map(|n| m.kids(n, "description").map(|x| (m.attr_or(x, "lang"), m.text(x))).collect()).unwrap_or_default();
        let behaviors = m.kid(root, "contents").map(|c| m.kids(c, "behaviorContent").map(|b| behavior_fields(m, b)).collect());
        let requirements = m.kid(root, "requirements").map_or_else(Requirements::default, |r| Requirements {
            naoqi: m.kids(r, "naoqiRequirement").map(|n| (m.attr_or(n, "minVersion"), m.attr_or(n, "maxVersion"))).collect(),
            robots: m.kids(r, "robotRequirement").map(|n| RobotRequirement { model: m.attr_or(n, "model"), min_head_version: m.attr_or(n, "minHeadVersion"), max_head_version: m.attr_or(n, "maxHeadVersion"), min_body_version: m.attr_or(n, "minBodyVersion"), max_body_version: m.attr_or(n, "maxBodyVersion") }).collect(),
        });
        Some(Manifest { descriptions, uuid: m.attr_or(root, "uuid"), version: m.attr_or(root, "version"), names, supported_languages: langs("supportedLanguages"), behaviors, requirements: Some(requirements) })
    }

    /// Updates uuid, version and titles (per language). Other manifest content is preserved.
    pub fn set_manifest_fields(&mut self, f: &Manifest) -> Result<(), String> {
        let id_ok = !f.uuid.is_empty() && f.uuid.len() <= 50 && f.uuid.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') && f.uuid.chars().any(|c| c.is_ascii_lowercase() || c == '-' || c == '_');
        if !id_ok {
            return Err("An ID may contain up to 50 alphanumeric lowercase letters, dashes or underscores.".into());
        }
        let parts: Vec<_> = f.version.split('.').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
            return Err("The version must have three numbers, e.g. 1.0.0.".into());
        }
        for langs in [f.names.iter().map(|v| &v.0).collect::<Vec<_>>(), f.descriptions.iter().map(|v| &v.0).collect(), f.supported_languages.iter().collect()] {
            let mut seen = std::collections::HashSet::new();
            for lang in langs {
                if lang.is_empty() || !lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || !seen.insert(lang) {
                    return Err("Language codes must be nonempty and unique within each list.".into());
                }
            }
        }
        let (_, m) = self.manifest.as_mut().ok_or("This project has no manifest")?;
        let root = m.root().unwrap();
        crate::edit::set_raw(m, root, "uuid", &f.uuid);
        crate::edit::set_raw(m, root, "version", &f.version);
        let names = match m.kid(root, "names") { Some(n) => n, None => crate::edit::insert_raw(m, root, "<names />")? };
        for x in m.kids(names, "name").collect::<Vec<_>>() {
            crate::edit::detach_raw(m, x);
        }
        for (lang, title) in &f.names {
            crate::edit::insert_raw(m, names, &format!("<name lang=\"{}\">{}</name>", crate::xml::escape_attr(lang), crate::xml::escape_text(title)))?;
        }
        for (section, tag, rows) in [
            ("descriptions", "description", f.descriptions.clone()),
            ("supportedLanguages", "language", f.supported_languages.iter().map(|l| (String::new(), l.clone())).collect()),
        ] {
            let sec = match m.kid(root, section) { Some(n) => n, None => crate::edit::insert_raw(m, root, &format!("<{section} />"))? };
            for n in m.kids(sec, tag).collect::<Vec<_>>() { crate::edit::detach_raw(m, n); }
            for (lang, text) in rows {
                let attr = if lang.is_empty() { String::new() } else { format!(" lang=\"{}\"", crate::xml::escape_attr(&lang)) };
                crate::edit::insert_raw(m, sec, &format!("<{tag}{attr}>{}</{tag}>", crate::xml::escape_text(&text)))?;
            }
        }
        if let Some(behaviors) = &f.behaviors {
            let contents = m.kid(root, "contents").ok_or("The manifest lists no behaviors")?;
            for b in behaviors {
                let node = m.kids(contents, "behaviorContent").find(|&n| m.attr(n, "path").as_deref() == Some(b.path.as_str())).ok_or_else(|| format!("The manifest has no behavior {}", b.path))?;
                // The Properties dialog sends every entry back: untouched ones keep their XML.
                if behavior_fields(m, node) != *b {
                    set_behavior_fields(m, node, b)?;
                }
            }
        }
        if let Some(r) = &f.requirements {
            let current = self.manifest_fields().and_then(|f| f.requirements).unwrap_or_default();
            let (_, m) = self.manifest.as_mut().unwrap();
            if current != *r {
                set_requirements(m, root, r)?;
            }
        }
        self.metadata.insert("UUID".into(), f.uuid.clone());
        self.metadata.insert("Version".into(), f.version.clone());
        if let Some((_, title)) = f.names.first() { self.metadata.insert("Title".into(), title.clone()); }
        self.structure_dirty = true;
        Ok(())
    }
}

/// A PML entry (`<File>`, `<Dialog>`, `<Topic>`, `<Translation>`) listing the project file `path`.
#[derive(Clone, Debug)]
pub struct Listing {
    pub section: &'static str,
    pub xml: String,
    pub path: String,
    pub kind: &'static str,
}

impl Listing {
    pub fn new(section: &'static str, xml: String, path: &str, kind: &'static str) -> Listing {
        Listing { section, xml, path: path.to_string(), kind }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub enum FilePolicy {
    /// Keep the project's file ("Use old attached file").
    UseExisting,
    /// Overwrite with the library's file ("Use new attached file").
    Replace,
    /// Copy as `name_1.ext` and point the box at it ("Rename new attached file").
    Rename,
}

#[derive(serde::Serialize, Debug)]
#[serde(tag = "result", content = "value", rename_all = "camelCase")]
pub enum Inserted {
    Conflicts(Vec<String>),
    Done(crate::edit::HistoryState),
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub uuid: String,
    pub version: String,
    /// (language, title)
    pub names: Vec<(String, String)>,
    pub supported_languages: Vec<String>,
    #[serde(default)]
    pub descriptions: Vec<(String, String)>,
    /// Behavior properties, one per `<behaviorContent>` (`None` when saving: unchanged).
    #[serde(default)]
    pub behaviors: Option<Vec<BehaviorManifest>>,
    /// NAOqi and robot requirements (`None` when saving: unchanged).
    #[serde(default)]
    pub requirements: Option<Requirements>,
}

/// A behavior's `<behaviorContent>`: its properties in the package manifest.
/// Lists are (language, text), conditions are (purpose, expression).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BehaviorManifest {
    pub path: String,
    /// `interactive` or `solitary`.
    pub nature: String,
    pub user_requestable: bool,
    pub names: Vec<(String, String)>,
    pub descriptions: Vec<(String, String)>,
    pub tags: Vec<(String, String)>,
    pub trigger_sentences: Vec<(String, String)>,
    pub loading_responses: Vec<(String, String)>,
    pub conditions: Vec<(String, String)>,
    pub permissions: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Requirements {
    /// (minVersion, maxVersion); an empty bound is left out.
    pub naoqi: Vec<(String, String)>,
    pub robots: Vec<RobotRequirement>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RobotRequirement {
    /// `NAO` or `JULIETTE` (Pepper).
    pub model: String,
    pub min_head_version: String,
    pub max_head_version: String,
    pub min_body_version: String,
    pub max_body_version: String,
}

/// The permissions a behavior can declare in its manifest.
pub const PERMISSIONS: &[&str] = &["canRunInSleep", "canRunOnPod", "canStandUp", "canSitDown"];

fn behavior_fields(m: &Doc, b: NodeId) -> BehaviorManifest {
    let rows = |section: &str, tag: &str, key: &str| m.kid(b, section).map(|s| m.kids(s, tag).map(|x| (m.attr_or(x, key), m.text(x))).collect()).unwrap_or_default();
    let permissions = m.kid(b, "permissions").map(|p| m.el(p).children.iter().filter(|&&c| matches!(m.nodes[c].kind, crate::xml::Kind::Element(_))).map(|&c| m.name(c).to_string()).collect()).unwrap_or_default();
    BehaviorManifest {
        path: m.attr_or(b, "path"),
        nature: m.kid(b, "nature").map(|n| m.text(n)).unwrap_or_default(),
        user_requestable: m.kid(b, "userRequestable").is_some(),
        names: rows("names", "name", "lang"),
        descriptions: rows("descriptions", "description", "lang"),
        tags: rows("tags", "tag", "lang"),
        trigger_sentences: rows("triggerSentences", "sentence", "lang"),
        loading_responses: rows("loadingResponses", "sentence", "lang"),
        conditions: rows("autonomous", "condition", "purpose"),
        permissions,
    }
}

/// Rewrites a `<behaviorContent>` in manifest order: per-language
/// maps sorted by key, empty sections left out, conditions as CDATA, `<permissions/>` always.
/// Elements it does not know stay after them.
fn set_behavior_fields(m: &mut Doc, node: NodeId, b: &BehaviorManifest) -> Result<(), String> {
    if !matches!(b.nature.as_str(), "interactive" | "solitary") {
        return Err("A behavior's nature is interactive or solitary.".into());
    }
    if let Some(p) = b.permissions.iter().find(|p| !PERMISSIONS.contains(&p.as_str())) {
        return Err(format!("Unknown permission {p}; use {}.", PERMISSIONS.join(", ")));
    }
    for (key, rows) in [("language", &b.names), ("language", &b.descriptions), ("language", &b.tags), ("language", &b.trigger_sentences), ("language", &b.loading_responses), ("purpose", &b.conditions)] {
        if rows.iter().any(|(k, _)| k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
            return Err(format!("Each entry needs a {key} (letters, digits, - or _)."));
        }
    }
    for rows in [&b.names, &b.descriptions] {
        let mut langs: Vec<_> = rows.iter().map(|r| &r.0).collect();
        langs.sort();
        if langs.windows(2).any(|w| w[0] == w[1]) {
            return Err("A behavior has one name and one description per language.".into());
        }
    }
    if b.conditions.iter().any(|c| c.1.contains("]]>")) {
        return Err("A condition cannot contain \"]]>\".".into());
    }
    let known = ["userRequestable", "nature", "names", "descriptions", "tags", "triggerSentences", "loadingResponses", "autonomous", "permissions"];
    for c in m.el(node).children.clone() {
        if matches!(m.nodes[c].kind, crate::xml::Kind::Element(_)) && known.contains(&m.name(c)) {
            crate::edit::detach_raw(m, c);
        }
    }
    let esc = crate::xml::escape_text;
    let section = |name: &str, tag: &str, key: &str, rows: &[(String, String)], cdata: bool| -> Option<String> {
        let mut rows = rows.to_vec();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let body: String = rows.iter().map(|(k, v)| format!("\n <{tag} {key}=\"{}\">{}</{tag}>", crate::xml::escape_attr(k), if cdata { format!("<![CDATA[{v}]]>") } else { esc(v) })).collect();
        (!rows.is_empty()).then(|| format!("<{name}>{body}\n</{name}>"))
    };
    let mut parts = vec![];
    if b.user_requestable {
        parts.push("<userRequestable/>".to_string());
    }
    parts.push(format!("<nature>{}</nature>", esc(&b.nature)));
    parts.extend(section("names", "name", "lang", &b.names, false));
    parts.extend(section("descriptions", "description", "lang", &b.descriptions, false));
    parts.extend(section("tags", "tag", "lang", &b.tags, false));
    parts.extend(section("triggerSentences", "sentence", "lang", &b.trigger_sentences, false));
    parts.extend(section("loadingResponses", "sentence", "lang", &b.loading_responses, false));
    parts.extend(section("autonomous", "condition", "purpose", &b.conditions, true));
    parts.push(if b.permissions.is_empty() { "<permissions/>".into() } else { format!("<permissions>{}\n</permissions>", b.permissions.iter().map(|p| format!("\n <{p}/>")).collect::<String>()) });
    for xml in parts {
        crate::edit::insert_raw(m, node, &xml)?;
    }
    Ok(())
}

/// Rewrites `<requirements>` (left out when empty), in the manifest's attribute order.
fn set_requirements(m: &mut Doc, root: NodeId, r: &Requirements) -> Result<(), String> {
    let version_ok = |v: &str| v.is_empty() || v.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if r.naoqi.iter().any(|(a, b)| !version_ok(a) || !version_ok(b)) || r.robots.iter().any(|x| [&x.min_head_version, &x.max_head_version, &x.min_body_version, &x.max_body_version].iter().any(|v| !version_ok(v))) {
        return Err("Versions are numbers separated by dots, e.g. 2.8.".into());
    }
    if r.robots.iter().any(|x| x.model.is_empty()) {
        return Err("A robot requirement needs a model.".into());
    }
    if let Some(old) = m.kid(root, "requirements") {
        crate::edit::detach_raw(m, old);
    }
    let attrs = |pairs: &[(&str, &String)]| pairs.iter().filter(|p| !p.1.is_empty()).map(|(k, v)| format!(" {k}=\"{}\"", crate::xml::escape_attr(v))).collect::<String>();
    let mut body = String::new();
    for (min, max) in &r.naoqi {
        body += &format!("\n <naoqiRequirement{}/>", attrs(&[("minVersion", min), ("maxVersion", max)]));
    }
    for x in &r.robots {
        body += &format!("\n <robotRequirement{}/>", attrs(&[("model", &x.model), ("minHeadVersion", &x.min_head_version), ("maxHeadVersion", &x.max_head_version), ("minBodyVersion", &x.min_body_version), ("maxBodyVersion", &x.max_body_version)]));
    }
    if !body.is_empty() {
        crate::edit::insert_raw(m, root, &format!("<requirements>{body}\n</requirements>"))?;
    }
    Ok(())
}

/// Writes `<dir>/<name>.pml`, `manifest.xml`, the behavior folders (`folder/behavior.xar`) and
/// resource files, refusing to overwrite an existing project.
/// Collects the files at or under `rel` (symbolic links count as files and are never followed).
fn collect_files(root: &Path, rel: &str, out: &mut Vec<String>) {
    let path = root.join(rel);
    let Ok(meta) = std::fs::symlink_metadata(&path) else { return };
    if !meta.is_dir() { return out.push(rel.to_string()); }
    for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
        if let Some(name) = entry.file_name().to_str() { collect_files(root, &format!("{rel}/{name}"), out); }
    }
}

/// The folder holding a behavior's files, or its `.xar` alone when it sits at the project root.
pub fn behavior_files(source: &str) -> &str {
    source.rsplit_once('/').map_or(source, |(dir, _)| dir)
}

pub fn write_folder(dir: &Path, name: &str, behaviors: &[(String, Vec<u8>)], resources: &[(String, Vec<u8>)]) -> Result<(), String> {
    if name.is_empty() || name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*']) {
        return Err("Can not contain any of these characters: \\ / < > : \" | ? *".into());
    }
    let pml = dir.join(format!("{name}.pml"));
    if pml.exists() {
        return Err(format!("{} already exists.", pml.display()));
    }
    let uuid = format!("{}-{:06x}", name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect::<String>(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()) & 0xffffff);
    let e = crate::xml::escape_attr;
    let descriptions: String = behaviors.iter().map(|(f, _)| format!("\n        <BehaviorDescription name=\"behavior\" src=\"{}\" xar=\"behavior.xar\" />", e(f))).collect();
    let section = |tag: &str, items: String| if items.is_empty() { format!("<{tag} />") } else { format!("<{tag}>{items}\n    </{tag}>") };
    let files: String = resources.iter().map(|(p, _)| format!("\n        <File name=\"{}\" src=\"{}\" />", e(&Path::new(p).file_stem().unwrap_or_default().to_string_lossy()), e(p))).collect();
    let contents: String = behaviors.iter().map(|(f, _)| format!("\n  <behaviorContent path=\"{}\">\n   <nature>interactive</nature>\n   <permissions/>\n  </behaviorContent>", e(f))).collect();
    let mut out = vec![
        (format!("{name}.pml"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<Package name=\"{}\" format_version=\"5\">\n    <Manifest src=\"manifest.xml\" />\n    {}\n    <Dialogs />\n    {}\n    <Topics />\n    <IgnoredPaths />\n</Package>\n", e(name), section("BehaviorDescriptions", descriptions), section("Resources", files)).into_bytes()),
        ("manifest.xml".into(), format!("<?xml version='1.0' encoding='UTF-8'?>\n<package version=\"0.0.0\" uuid=\"{}\">\n <names>\n  <name lang=\"en_US\">{}</name>\n </names>\n <supportedLanguages>\n  <language>en_US</language>\n </supportedLanguages>\n <descriptionLanguages>\n  <language>en_US</language>\n </descriptionLanguages>\n <contents>{contents}\n </contents>\n</package>\n", e(&uuid), crate::xml::escape_text(name)).into_bytes()),
    ];
    out.extend(behaviors.iter().map(|(f, x)| (format!("{f}/behavior.xar"), x.clone())));
    out.extend(resources.iter().cloned());
    // Never overwrite: the folder may already hold someone's work.
    if let Some((rel, _)) = out.iter().find(|(rel, _)| dir.join(rel).exists()) {
        return Err(format!("{} already exists in this folder. Choose an empty folder.", rel));
    }
    for (rel, bytes) in &out {
        let target = dir.join(relative(rel)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("Cannot create {}: {err}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|err| format!("Cannot write {}: {err}", target.display()))?;
    }
    Ok(())
}
