//! "Update Translations": Localized string parameters (content type 5) are gathered into Qt
//! Linguist files `translations/translation_<lang>.ts`, merged the way `lupdate` merges. Layout
//! follows the files Choregraphe writes: contexts `<behavior.xar>:/<box path>` sorted by name,
//! the parameter value as source, its name as comment, `line="0"` locations, 4-space indent.

use crate::edit::{History, HistoryState};
use crate::project::Project;
use crate::xml::{Doc, NodeId};
use std::collections::BTreeMap;

/// (source, comment) → location file.
type Messages = Vec<(String, String, String)>;

/// Localized strings of every behavior, by context name.
pub fn gather(p: &Project) -> BTreeMap<String, Messages> {
    let mut out: BTreeMap<String, Messages> = BTreeMap::new();
    for b in &p.behaviors {
        let root = crate::view::root_box(&b.doc);
        walk(&b.doc, root, "", &b.source, &mut out);
    }
    out
}

fn walk(d: &Doc, b: NodeId, path: &str, source: &str, out: &mut BTreeMap<String, Messages>) {
    for p in d.kids(b, "Parameter").filter(|&p| d.attr_raw(p, "content_type") == Some("5")) {
        let (value, name) = (d.attr_or(p, "value"), d.attr_or(p, "name"));
        let list = out.entry(format!("{source}:{path}")).or_default();
        if !value.is_empty() && !list.iter().any(|m| m.0 == value && m.1 == name) {
            list.push((value, name, source.to_string()));
        }
    }
    for t in d.kids(b, "Timeline") {
        for l in d.kids(t, "BehaviorLayer") {
            for k in d.kids(l, "BehaviorKeyframe") {
                for g in d.kids(k, "Diagram") {
                    for child in d.kids(g, "Box") {
                        walk(d, child, &format!("{path}/{}", d.attr_or(child, "name")), source, out);
                    }
                }
            }
        }
    }
}

/// `lupdate` escaping: `'` stays literal.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A message read back from an existing file, raw (still escaped) so it is rewritten unchanged.
struct Old {
    source: String,
    comment: String,
    kind: Option<String>,
    translation: String,
}

fn raw_text(d: &Doc, parent: NodeId, tag: &str) -> Option<String> {
    let n = d.kid(parent, tag)?;
    let mut s = String::new();
    for &c in &d.el(n).children {
        d.write_node(c, &mut s);
    }
    Some(s)
}

fn read_old(bytes: &[u8]) -> Result<BTreeMap<String, Vec<Old>>, String> {
    let d = Doc::parse(std::str::from_utf8(bytes).map_err(|_| "Translation file is not UTF-8")?)?;
    let mut out: BTreeMap<String, Vec<Old>> = BTreeMap::new();
    for c in d.kids(d.root().unwrap(), "context") {
        let name = raw_text(&d, c, "name").unwrap_or_default();
        let list = out.entry(name).or_default();
        for m in d.kids(c, "message") {
            let t = d.kid(m, "translation");
            list.push(Old {
                source: raw_text(&d, m, "source").unwrap_or_default(),
                comment: raw_text(&d, m, "comment").unwrap_or_default(),
                kind: t.and_then(|t| d.attr(t, "type")),
                translation: raw_text(&d, m, "translation").unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

/// Builds `translation_<lang>.ts` from the current strings and the previous file. Kept strings keep
/// their translation; new ones are unfinished (pre-filled with the source for `auto_fill`); strings
/// that disappeared become `vanished` (unfinished) or `obsolete` (finished).
pub fn render(lang: &str, auto_fill: bool, current: &BTreeMap<String, Messages>, old: Option<&[u8]>) -> Result<String, String> {
    let old = old.map(read_old).transpose()?.unwrap_or_default();
    let mut names: Vec<String> = current.iter().filter(|(_, m)| !m.is_empty()).map(|(k, _)| esc(k)).chain(old.keys().cloned()).collect();
    names.sort();
    names.dedup();
    let by_raw: BTreeMap<String, &Messages> = current.iter().map(|(k, v)| (esc(k), v)).collect();
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE TS>\n<TS version=\"2.1\" language=\"{}\">\n", esc(lang));
    for name in names {
        let now: &[(String, String, String)] = by_raw.get(&name).map_or(&[], |m| m.as_slice());
        let before = old.get(&name).map_or(&[][..], |v| v.as_slice());
        out += &format!("    <context>\n        <name>{name}</name>\n");
        // lupdate keeps the previous file's message order and appends new strings.
        for o in before {
            match now.iter().find(|(s, c, _)| esc(s) == o.source && esc(c) == o.comment) {
                Some((_, _, file)) => {
                    let kind = match o.kind.as_deref() {
                        Some("obsolete" | "vanished") => Some("unfinished"),
                        k => k,
                    };
                    out += &message(Some(file), &o.source, &o.comment, kind, &o.translation);
                }
                None => {
                    let kind = match o.kind.as_deref() {
                        Some("unfinished") => "vanished",
                        Some(k @ ("obsolete" | "vanished")) => k,
                        _ => "obsolete",
                    };
                    out += &message(None, &o.source, &o.comment, Some(kind), &o.translation);
                }
            }
        }
        for (source, comment, file) in now {
            let (s, c) = (esc(source), esc(comment));
            if !before.iter().any(|o| o.source == s && o.comment == c) {
                let text = if auto_fill { s.clone() } else { String::new() };
                out += &message(Some(file), &s, &c, Some("unfinished"), &text);
            }
        }
        out += "    </context>\n";
    }
    out += "</TS>\n";
    Ok(out)
}

/// One `<message>`; `source`, `comment` and `text` are already escaped.
fn message(file: Option<&str>, source: &str, comment: &str, kind: Option<&str>, text: &str) -> String {
    let location = file.map(|f| format!("            <location filename=\"{}\" line=\"0\"/>\n", esc(f))).unwrap_or_default();
    let t = kind.map(|k| format!(" type=\"{k}\"")).unwrap_or_default();
    format!("        <message>\n{location}            <source>{source}</source>\n            <comment>{comment}</comment>\n            <translation{t}>{text}</translation>\n        </message>\n")
}

/// What Choregraphe's `lrelease` (Qt 5.12, no options) makes of a `.ts` when it builds a package:
/// the `.qm` that QTranslator loads on the robot. Obsolete, vanished and empty unfinished messages
/// are left out, a repeated message keeps its first copy, and a comment is only stored when the
/// message without it is taken. Blocks: hashes, messages, numerus rules (no contexts block).
pub fn compile(ts: &[u8]) -> Result<Vec<u8>, String> {
    let d = Doc::parse(std::str::from_utf8(ts).map_err(|_| "Translation file is not UTF-8")?)?;
    let root = d.root().unwrap();
    let text = |n: NodeId, tag: &str| d.kid(n, tag).map(|k| d.text(k)).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut all = vec![];
    for c in d.kids(root, "context") {
        let ctx = text(c, "name");
        for m in d.kids(c, "message") {
            let key = (ctx.clone(), text(m, "source"), text(m, "comment"));
            if !seen.insert(key.clone()) {
                continue;
            }
            let t = d.kid(m, "translation");
            let forms: Vec<String> = match t {
                Some(t) if d.kid(t, "numerusform").is_some() => d.kids(t, "numerusform").map(|f| d.text(f)).collect(),
                t => vec![t.map(|t| d.text(t)).unwrap_or_default()],
            };
            all.push((key, t.and_then(|t| d.attr(t, "type")), forms));
        }
    }
    let bare: std::collections::HashSet<_> = all.iter().filter(|(k, ..)| k.2.is_empty()).map(|(k, ..)| (&k.0, &k.1)).collect();
    let mut msgs: BTreeMap<(&[u8], &[u8], &[u8]), &[String]> = BTreeMap::new();
    for ((ctx, src, comment), kind, forms) in &all {
        match kind.as_deref() {
            Some("obsolete" | "vanished") => continue,
            Some("unfinished") if forms[0].is_empty() => continue,
            _ => {}
        }
        let stripped = (ctx.as_bytes(), src.as_bytes(), &b""[..]);
        let force = comment.is_empty() || ctx.is_empty() || bare.contains(&(ctx, src));
        let key = if force || msgs.contains_key(&stripped) { (ctx.as_bytes(), src.as_bytes(), comment.as_bytes()) } else { stripped };
        msgs.entry(key).or_insert(forms);
    }
    let mut body = vec![];
    let mut hashes = vec![];
    for ((ctx, src, comment), forms) in msgs {
        hashes.push((elf_hash(src.iter().chain(comment)), body.len() as u32));
        for f in forms {
            let utf16: Vec<u8> = f.encode_utf16().flat_map(u16::to_be_bytes).collect();
            body.push(3);
            body.extend(if f.is_empty() { u32::MAX } else { utf16.len() as u32 }.to_be_bytes());
            body.extend(utf16);
        }
        for (tag, bytes) in [(8, comment), (6, src), (7, ctx)] {
            body.push(tag);
            body.extend((bytes.len() as u32).to_be_bytes());
            body.extend(bytes);
        }
        body.push(1);
    }
    hashes.sort();
    let hashes: Vec<u8> = hashes.iter().flat_map(|(h, o)| [h.to_be_bytes(), o.to_be_bytes()]).flatten().collect();
    let mut out = vec![0x3c, 0xb8, 0x64, 0x18, 0xca, 0xef, 0x9c, 0x95, 0xcd, 0x21, 0x1c, 0xbf, 0x60, 0xa1, 0xbd, 0xdd];
    for (tag, data) in [(0x42, &hashes[..]), (0x69, &body[..]), (0x88, numerus_rules(&d.attr_or(root, "language")))] {
        if !data.is_empty() {
            out.push(tag);
            out.extend((data.len() as u32).to_be_bytes());
            out.extend(data);
        }
    }
    Ok(out)
}

/// Qt's `elfHash`, which stops at a NUL byte and never returns 0.
fn elf_hash<'a>(bytes: impl Iterator<Item = &'a u8>) -> u32 {
    let mut h: u32 = 0;
    for &b in bytes.take_while(|&&b| b != 0) {
        h = (h << 4).wrapping_add(b as u32);
        let g = h & 0xf000_0000;
        h ^= g >> 24;
        h &= !g;
    }
    h.max(1)
}

/// Plural rules lrelease writes for each robot language (others get the English rules).
fn numerus_rules(lang: &str) -> &'static [u8] {
    match lang {
        "ar_SA" => &[0x01, 0x00, 0xff, 0x01, 0x01, 0xff, 0x01, 0x02, 0xff, 0x24, 0x03, 0x0a, 0xff, 0x2a, 0x0b],
        "cs_CZ" => &[0x01, 0x01, 0xff, 0x04, 0x02, 0x04],
        "fr_FR" | "pt_BR" => &[0x03, 0x01],
        "pl_PL" => &[0x01, 0x01, 0xff, 0x14, 0x02, 0x04, 0xfd, 0x2c, 0x0a, 0x13],
        "ru_RU" => &[0x11, 0x01, 0xfd, 0x29, 0x0b, 0xff, 0x14, 0x02, 0x04, 0xfd, 0x2c, 0x0a, 0x13],
        "ja_JP" | "ko_KR" | "tr_TR" | "zh_CN" | "zh_TW" => &[],
        _ => &[0x01, 0x01],
    }
}

/// Regenerates the translation file of every project language (manifest supported languages
/// and languages already listed in the PML) as one undo step, and lists new files in the PML.
pub fn update(p: &mut Project, h: &mut History) -> Result<HistoryState, String> {
    let pml = p.pml.as_ref().ok_or("Translations need a .pml project")?;
    let pkg = pml.root().unwrap();
    let section = pml.kid(pkg, "Translations");
    let mut langs: Vec<String> = section.map(|s| pml.kids(s, "Translation").filter_map(|t| pml.attr(t, "language")).collect()).unwrap_or_default();
    for l in p.manifest_fields().map(|m| m.supported_languages).unwrap_or_default() {
        if !langs.contains(&l) {
            langs.push(l);
        }
    }
    if langs.is_empty() {
        langs.push("en_US".into());
    }
    let auto = section.and_then(|s| pml.attr(s, "auto-fill")).unwrap_or_else(|| langs[0].clone());
    let current = gather(p);
    let mut files = vec![];
    for lang in &langs {
        let path = format!("translations/translation_{lang}.ts");
        let text = render(lang, *lang == auto, &current, p.read_file(&path).as_deref())?;
        files.push((path, text.into_bytes()));
    }
    let e = crate::xml::escape_attr;
    let sec = match p.pml.as_ref().and_then(|x| x.kid(pkg, "Translations")) {
        Some(s) => s,
        None => crate::edit::insert_raw(p.pml.as_mut().unwrap(), pkg, &format!("<Translations auto-fill=\"{}\" />", e(&auto)))?,
    };
    let mut listings = vec![];
    for (path, _) in &files {
        let pml = p.pml.as_ref().unwrap();
        if !pml.kids(sec, "Translation").any(|t| pml.attr(t, "src").as_deref() == Some(path)) {
            let lang = path.trim_start_matches("translations/translation_").trim_end_matches(".ts");
            listings.push(crate::project::Listing::new("Translations", format!("<Translation name=\"translation_{0}\" src=\"{1}\" language=\"{0}\" />", e(lang), e(path)), path, "translation"));
        } else if !p.files.iter().any(|f| &f.path == path) {
            p.files.push(crate::view::ProjectFile { missing: Some(false), ..crate::view::ProjectFile::new(path.clone(), "translation") });
        }
    }
    crate::edit::set_listed_files(p, h, "Update translations", files, listings)
}
