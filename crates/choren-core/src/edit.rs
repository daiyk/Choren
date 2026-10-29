//! Edit engine: typed commands compile to reversible primitive operations on the lossless tree.
//! One `apply` call is one undo step, applied atomically.

use crate::project::Project;
use crate::rules;
use crate::xml::{escape_attr, Doc, Kind, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
enum Op {
    Attr { node: NodeId, key: String, before: Option<String>, after: Option<String> },
    /// Inserted (`insert`) or removed child at `index` of `parent`.
    Child { parent: NodeId, index: usize, node: NodeId, insert: bool },
    Text { node: NodeId, before: String, after: String },
}

fn run(d: &mut Doc, op: &Op, forward: bool) {
    match op {
        Op::Attr { node, key, before, after } => {
            let raw = if forward { after } else { before }.clone();
            let order = if matches!(d.name(*node), "Input" | "Output" | "Parameter") { PORT_ATTRS } else { &[] };
            d.set_attr_ordered(*node, key, raw, order);
        }
        Op::Child { parent, index, node, insert } => {
            if *insert == forward {
                d.el_mut(*parent).children.insert(*index, *node);
                d.nodes[*node].parent = Some(*parent);
            } else {
                d.el_mut(*parent).children.remove(*index);
                d.nodes[*node].parent = None;
            }
        }
        Op::Text { node, before, after } => {
            let v = if forward { after } else { before }.clone();
            if let Kind::Text(t) | Kind::CData(t) = &mut d.nodes[*node].kind {
                *t = v;
            }
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub enum NewBoxKind {
    Python,
    Diagram,
    Timeline,
    Dialog,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum PortKind {
    Input,
    Output,
    Parameter,
}

/// Key interpolation as Choregraphe's curve editor names it ("Bezier automatic" = no tangents).
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Interp {
    Constant,
    Linear,
    Bezier,
    BezierAuto,
}

/// The simplifier `SimplifyCurves` runs.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum SimplifyMode {
    /// Choren's own: drops the keys that linear interpolation between the kept keys reproduces
    /// within `tolerance` (`motion::rdp_simplify`); the kept keys' handles stay as they are.
    #[default]
    Choren,
    /// Choregraphe-compatible basic mode: drops keys whose removal keeps the sampled curve's mean
    /// error under `tolerance` (Choregraphe's default 0.5), re-aiming the neighbours' handles
    /// (`motion::simplify`).
    Choregraphe,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum MirrorMode {
    /// Exchange left and right.
    Flip,
    LeftToRight,
    RightToLeft,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    /// Sets (or with `null` removes) unescaped attribute values, validated per element type.
    SetAttributes { node: NodeId, values: BTreeMap<String, Option<String>> },
    MoveBoxes { moves: Vec<(NodeId, f64, f64)> },
    SetScript { r#box: NodeId, text: String },
    /// `plugin_language` is a Set Language box's language (`SetLanguageBoxCommand`): its
    /// `pluginContent` and the script the plugin generates from it.
    SetBoxContent { r#box: NodeId, bitmaps: Option<Vec<String>>, plugin_text: Option<String>, plugin_language: Option<String> },
    /// A Dialog box runs `dialog_file`, a `.dlg` relative to the behavior folder.
    NewBox { diagram: NodeId, kind: NewBoxKind, name: String, x: f64, y: f64, #[serde(default)] dialog_file: Option<String> },
    /// `EditDialogBoxCommand`: points a Dialog box at another `.dlg`.
    SetDialogFile { r#box: NodeId, file: String },
    /// `SortIOCommand`: every input, every output or every parameter of one box, in the new
    /// order. Ids are kept.
    SortPorts { r#box: NodeId, ports: Vec<NodeId> },
    /// Adds a `<Box>` element (from a library `box.xar` or a template) with a fresh id.
    AddBox { diagram: NodeId, xml: String, x: f64, y: f64 },
    AddPort { r#box: NodeId, kind: PortKind, name: String, values: BTreeMap<String, String> },
    AddLink { diagram: NodeId, out_owner: i64, out_port: i64, in_owner: i64, in_port: i64 },
    /// Inserts a child element; the parent/child pairs Choregraphe allows are whitelisted.
    InsertXml { parent: NodeId, xml: String },
    Remove { nodes: Vec<NodeId> },
    Paste { diagram: NodeId, xml: String, dx: f64, dy: f64 },
    /// Wraps boxes into a new diagram box; links that cross the selection go through new
    /// boundary ports named `%1_%2` (box name, port name), as `ConvertToBoxCommand` does.
    ConvertToBox { diagram: NodeId, boxes: Vec<NodeId>, name: String },
    /// "Store joints in keyframe": the robot's `angles` (radians, descriptor joint order) for a
    /// joint group (Whole body, Head, Arms, Legs, a chain or a joint) become keys at `frame`.
    StoreKeys { timeline: NodeId, frame: i64, group: String, angles: Vec<f64> },
    /// "Remove joints from keyframe" for a group, at `frame` or at every frame up to `to`.
    RemoveKeys { timeline: NodeId, frame: i64, group: String, #[serde(default)] to: Option<i64> },
    /// Moves curve keys by `delta` frames, keeping each curve sorted.
    MoveKeys { keys: Vec<NodeId>, delta: i64 },
    SetInterpolation { keys: Vec<NodeId>, interp: Interp },
    /// Flip or Mirror the timeline's motion with the robot's left/right joint map, for the
    /// whole timeline or the keys in `frames` (first, last).
    Mirror { timeline: NodeId, mode: MirrorMode, #[serde(default)] frames: Option<(i64, i64)> },
    /// `SetActuatorCurveValuesCommand`: `value * scale + offset` for the keys (tangent
    /// ordinates scale too). Consecutive edits of the same keys merge.
    SetCurveValues { keys: Vec<NodeId>, scale: f64, offset: f64 },
    /// `MoveTangentCommand`: a Bezier handle's offsets. Consecutive moves of one handle merge.
    MoveTangent { tangent: NodeId, abscissa: f64, ordinate: f64 },
    /// Pastes `copy_curve_keys` output with its first key at `frame`, replacing keys at the same
    /// frames and creating missing curves.
    PasteCurveKeys { timeline: NodeId, xml: String, frame: i64 },
    /// Curve Simplify over the keys in `from..=to`; `curves` empty = every curve. `mode` picks
    /// the simplifier (see [`SimplifyMode`]); `advanced` (angularity margin, granularity;
    /// Choregraphe's defaults 10 and 5) runs the Choregraphe-compatible advanced mode
    /// (`motion::advanced_simplify`) and needs `mode: Choregraphe`.
    SimplifyCurves { timeline: NodeId, curves: Vec<NodeId>, from: i64, to: i64, tolerance: f64, #[serde(default)] mode: SimplifyMode, #[serde(default)] advanced: Option<(f64, f64)> },
    /// `TranslateKeyframesCommand`: shifts behavior keyframes by `delta` frames.
    TranslateKeyframes { keyframes: Vec<NodeId>, delta: i64 },
    /// Pastes `copy_keyframes` output into a layer with its first keyframe at `index`; taken
    /// names get `_copy`.
    PasteKeyframes { layer: NodeId, xml: String, index: i64 },
    /// `Change/MoveTimelineRangeCommand`: start and end frames (`end` -1 = up to the size).
    SetTimelineRange { timeline: NodeId, start: i64, end: i64 },
    /// `StretchTimelineRangeCommand`: a new range, with the keys and keyframes inside the old
    /// range scaled into it.
    StretchTimelineRange { timeline: NodeId, start: i64, end: i64 },
    /// Motion recording: `[actuator, value in file units, is hand]` become keys
    /// at `frame` (`record::store_keys`).
    RecordKeys { timeline: NodeId, frame: i64, values: Vec<(String, f64, bool)> },
}

/// Choregraphe's attribute order on ports, for attributes added later (stm_value_name, min…).
const PORT_ATTRS: &[&str] = &["name", "inherits_from_parent", "content_type", "value", "default_value", "min", "max", "custom_choice", "type", "type_size", "nature", "stm_value_name", "inner", "tooltip", "id"];

const ALLOWED_CHILDREN: &[(&str, &[&str])] = &[
    ("Box", &["Resource", "bitmap", "Timeline", "pluginContent"]),
    ("Timeline", &["BehaviorLayer", "ActuatorList"]),
    ("BehaviorLayer", &["BehaviorKeyframe"]),
    ("BehaviorKeyframe", &["Diagram"]),
    ("ActuatorList", &["ActuatorCurve"]),
    ("ActuatorCurve", &["Key"]),
    ("Key", &["Tangent"]),
    ("Parameter", &["Choice"]),
];

/// Choregraphe's child order inside a Box; new children go after their group.
const BOX_ORDER: &[&str] = &["dialogFile", "bitmap", "script", "pluginContent", "Input", "Output", "Parameter", "Resource", "Timeline"];
/// Manifest children in the order existing packages use.
const PACKAGE_ORDER: &[&str] = &["names", "descriptions", "supportedLanguages", "descriptionLanguages", "contents", "requirements", "services", "executableFiles"];
const BEHAVIOR_CONTENT_ORDER: &[&str] = &["userRequestable", "nature", "names", "descriptions", "tags", "triggerSentences", "loadingResponses", "autonomous", "permissions"];

struct Tx<'a> {
    d: &'a mut Doc,
    /// The behavior file, project-relative.
    behavior: &'a str,
    ops: Vec<Op>,
    created: Vec<NodeId>,
}

impl Tx<'_> {
    fn push(&mut self, op: Op) {
        run(self.d, &op, true);
        self.ops.push(op);
    }

    fn set(&mut self, node: NodeId, key: &str, value: Option<&str>) {
        let after = value.map(escape_attr);
        let before = self.d.attr_raw(node, key).map(str::to_string);
        if before != after {
            self.push(Op::Attr { node, key: key.into(), before, after });
        }
    }

    fn ws(&mut self, s: String) -> NodeId {
        self.d.new_text(s)
    }

    /// Attaches a detached element under `parent`, adding Choregraphe-style indentation.
    fn insert(&mut self, parent: NodeId, child: NodeId) {
        let depth = self.d.depth(parent);
        let unit = self.d.indent_unit();
        self.d.reformat(child, depth + 1, unit);
        let pad = |n: usize| format!("\n{}", " ".repeat(n * unit));
        let tag = self.d.name(child).to_string();
        let order: &[&str] = match self.d.name(parent) {
            "Box" => BOX_ORDER,
            "package" => PACKAGE_ORDER,
            "behaviorContent" => BEHAVIOR_CONTENT_ORDER,
            _ => &[],
        };
        let rank = |t: &str| order.iter().position(|&o| o == t);
        let kids = self.d.el(parent).children.clone();
        let els: Vec<usize> = (0..kids.len()).filter(|&i| matches!(self.d.nodes[kids[i]].kind, Kind::Element(_))).collect();
        let after = els.iter().rev().find(|&&i| {
            let t = self.d.name(kids[i]);
            t == tag || matches!((rank(t), rank(&tag)), (Some(a), Some(b)) if a <= b)
        });
        let before_first = els.first().filter(|_| after.is_none() && rank(&tag).is_some());
        if let Some(&i) = after.or(if before_first.is_none() { els.last() } else { None }) {
            let w = self.ws(pad(depth + 1));
            self.push(Op::Child { parent, index: i + 1, node: w, insert: true });
            self.push(Op::Child { parent, index: i + 2, node: child, insert: true });
        } else if let Some(&i) = before_first {
            let w = self.ws(pad(depth + 1));
            self.push(Op::Child { parent, index: i, node: child, insert: true });
            self.push(Op::Child { parent, index: i + 1, node: w, insert: true });
        } else {
            // An emptied parent keeps its old indentation only: lay it out afresh.
            while let Some(&c) = self.d.el(parent).children.last().filter(|&&c| matches!(&self.d.nodes[c].kind, Kind::Text(t) if t.trim().is_empty())) {
                let index = self.d.el(parent).children.len() - 1;
                self.push(Op::Child { parent, index, node: c, insert: false });
            }
            let open = self.ws(pad(depth + 1));
            let close = self.ws(pad(depth));
            let n = self.d.el(parent).children.len();
            self.push(Op::Child { parent, index: n, node: open, insert: true });
            self.push(Op::Child { parent, index: n + 1, node: child, insert: true });
            self.push(Op::Child { parent, index: n + 2, node: close, insert: true });
        }
        self.created.push(child);
    }

    /// Attaches a detached element just before `sibling`, with the same indentation.
    fn insert_before(&mut self, parent: NodeId, child: NodeId, sibling: NodeId) {
        let depth = self.d.depth(parent);
        let unit = self.d.indent_unit();
        self.d.reformat(child, depth + 1, unit);
        let i = self.d.el(parent).children.iter().position(|&c| c == sibling).unwrap();
        let w = self.ws(format!("\n{}", " ".repeat((depth + 1) * unit)));
        self.push(Op::Child { parent, index: i, node: child, insert: true });
        self.push(Op::Child { parent, index: i + 1, node: w, insert: true });
        self.created.push(child);
    }

    /// Detaches an element and the indentation before it.
    fn detach(&mut self, node: NodeId) {
        let Some(parent) = self.d.nodes[node].parent else { return };
        let i = self.d.el(parent).children.iter().position(|&c| c == node).unwrap();
        self.push(Op::Child { parent, index: i, node, insert: false });
        if i > 0 {
            let prev = self.d.el(parent).children[i - 1];
            if matches!(&self.d.nodes[prev].kind, Kind::Text(t) if t.trim().is_empty()) {
                self.push(Op::Child { parent, index: i - 1, node: prev, insert: false });
            }
        }
        // An element that was `<x />` before `insert` padded it becomes `<x />` again.
        let blank = |d: &Doc, c: NodeId| matches!(&d.nodes[c].kind, Kind::Text(t) if t.trim().is_empty());
        if self.d.el(parent).empty && self.d.el(parent).children.iter().all(|&c| blank(self.d, c)) {
            while let Some(&c) = self.d.el(parent).children.last() {
                let index = self.d.el(parent).children.len() - 1;
                self.push(Op::Child { parent, index, node: c, insert: false });
            }
        }
    }

    fn fragment(&mut self, xml: &str, tag: &str) -> Result<NodeId, String> {
        let n = self.d.parse_fragment(xml)?;
        if self.d.name(n) != tag {
            return Err(format!("Expected a <{tag}> element"));
        }
        Ok(n)
    }

    fn diagram(&self, n: NodeId) -> Result<(), String> {
        if self.d.is_el(n, "Diagram") { Ok(()) } else { Err("No current diagram".into()) }
    }

    fn add_box(&mut self, diagram: NodeId, b: NodeId, x: f64, y: f64) -> Result<i64, String> {
        self.place_box(diagram, b, x, y, None)
    }

    /// Adds a box, keeping id `keep` if it is still free, else taking the first free id.
    fn place_box(&mut self, diagram: NodeId, b: NodeId, x: f64, y: f64, keep: Option<i64>) -> Result<i64, String> {
        self.diagram(diagram)?;
        let id = keep.filter(|&i| id_available(self.d, diagram, i)).unwrap_or_else(|| first_id_available(self.d, diagram));
        self.insert(diagram, b);
        self.set(b, "id", Some(&id.to_string()));
        self.set(b, "x", Some(&fmt_num(x)));
        self.set(b, "y", Some(&fmt_num(y)));
        Ok(id)
    }

    fn command(&mut self, c: Command) -> Result<(), String> {
        match c {
            Command::SetAttributes { node, values } => {
                validate_attrs(self.d, node, &values)?;
                for (k, v) in &values {
                    self.set(node, k, v.as_deref());
                }
            }
            Command::MoveBoxes { moves } => {
                for (b, x, y) in moves {
                    if !self.d.is_el(b, "Box") {
                        return Err("Only boxes can be moved".into());
                    }
                    self.set(b, "x", Some(&fmt_num(x)));
                    self.set(b, "y", Some(&fmt_num(y)));
                }
            }
            Command::SetScript { r#box, text } => self.set_script(r#box, &text)?,
            Command::SetBoxContent { r#box, bitmaps, plugin_text, plugin_language } => {
                if !self.d.is_el(r#box, "Box") { return Err("Select a box.".into()); }
                if let Some(paths) = bitmaps {
                    let old: Vec<_> = self.d.kids(r#box, "bitmap").collect();
                    for n in old { self.detach(n); }
                    for path in paths {
                        let child = self.fragment(&format!("<bitmap>{}</bitmap>", crate::xml::escape_text(&path)), "bitmap")?;
                        self.insert(r#box, child);
                    }
                }
                if let Some(text) = plugin_text {
                    if self.d.attr_raw(r#box, "plugin") != Some("textedit_plugin") {
                        return Err("Text content requires a Comment box.".into());
                    }
                    let old: Vec<_> = self.d.kids(r#box, "pluginContent").collect();
                    for n in old { self.detach(n); }
                    let child = self.fragment(&format!("<pluginContent><text>{}</text></pluginContent>", crate::xml::escape_text(&text)), "pluginContent")?;
                    self.insert(r#box, child);
                }
                if let Some(language) = plugin_language {
                    if self.d.attr_raw(r#box, "plugin") != Some("setlanguage_plugin") {
                        return Err("A language requires a Set Language box.".into());
                    }
                    if !crate::templates::SPEECH_LANGUAGES.contains(&language.as_str()) {
                        return Err(format!("Unknown robot language {language}"));
                    }
                    // Keep the services switched off (`<tts use="0"/>`), replace the language.
                    let old: Vec<_> = self.d.kids(r#box, "pluginContent").collect();
                    let mut kept = String::new();
                    let on = |name: &str| !old.iter().flat_map(|&p| self.d.kids(p, name)).any(|n| self.d.attr_raw(n, "use") == Some("0"));
                    let (tts, asr, dialog) = (on("tts"), on("asr"), on("dialog"));
                    for &p in &old {
                        for k in self.d.el(p).children.clone() {
                            if matches!(&self.d.nodes[k].kind, Kind::Element(_)) && self.d.name(k) != "language" { kept.push_str("\n    "); self.d.write_node(k, &mut kept); }
                        }
                    }
                    for n in old { self.detach(n); }
                    let child = self.fragment(&format!("<pluginContent>\n    <language>{}</language>{kept}\n</pluginContent>", crate::xml::escape_text(&language)), "pluginContent")?;
                    self.insert(r#box, child);
                    self.set_script(r#box, &crate::templates::set_language_script(tts, asr, dialog, &language))?;
                }
            },
            Command::NewBox { diagram, kind, name, x, y, dialog_file } => {
                rules::box_name(&name)?;
                let b = self.fragment(&crate::templates::new_box(kind, &name), "Box")?;
                self.add_box(diagram, b, x, y)?;
                if let Some(f) = dialog_file {
                    self.set_dialog_file(b, &f)?;
                }
            }
            Command::SetDialogFile { r#box, file } => self.set_dialog_file(r#box, &file)?,
            Command::SortPorts { r#box, ports } => {
                let tag = ports.first().map(|&p| self.d.name(p).to_string()).unwrap_or_default();
                let (mut have, mut want): (Vec<NodeId>, Vec<NodeId>) = (self.d.kids(r#box, &tag).collect(), ports.clone());
                have.sort();
                want.sort();
                if !matches!(tag.as_str(), "Input" | "Output" | "Parameter") || have != want {
                    return Err("Sort all the inputs, outputs or parameters of one box.".into());
                }
                ports.iter().for_each(|&p| self.detach(p));
                ports.into_iter().for_each(|p| self.insert(r#box, p));
            }
            Command::AddBox { diagram, xml, x, y } => {
                let b = self.fragment(&xml, "Box")?;
                self.add_box(diagram, b, x, y)?;
            }
            Command::AddPort { r#box, kind, name, values } => {
                if !self.d.is_el(r#box, "Box") {
                    return Err("Ports belong to boxes".into());
                }
                rules::io_name(self.d, r#box, None, &name, kind == PortKind::Input && values.get("nature").map(String::as_str) == Some("4"))?;
                let id = rules::ports(self.d, r#box).map(|p| self.d.attr_num(p, "id", 0.0) as i64).max().unwrap_or(0) + 1;
                let p = self.fragment(&crate::templates::port(kind, &name, id, &values), match kind {
                    PortKind::Input => "Input",
                    PortKind::Output => "Output",
                    PortKind::Parameter => "Parameter",
                })?;
                self.insert(r#box, p);
            }
            Command::AddLink { diagram, out_owner, out_port, in_owner, in_port } => {
                self.diagram(diagram)?;
                rules::link(self.d, diagram, (out_owner, out_port), (in_owner, in_port))?;
                let xml = format!(r#"<Link inputowner="{in_owner}" indexofinput="{in_port}" outputowner="{out_owner}" indexofoutput="{out_port}" />"#);
                let l = self.fragment(&xml, "Link")?;
                self.insert(diagram, l);
            }
            Command::InsertXml { parent, xml } => {
                let n = self.d.parse_fragment(&xml)?;
                let (p, c) = (self.d.name(parent).to_string(), self.d.name(n).to_string());
                if !ALLOWED_CHILDREN.iter().any(|(a, kids)| *a == p && kids.contains(&c.as_str())) {
                    return Err(format!("A <{c}> cannot be added to a <{p}>"));
                }
                // Keyframes and curve keys stay sorted.
                let order = match c.as_str() {
                    "BehaviorKeyframe" => Some("index"),
                    "Key" => Some("frame"),
                    _ => None,
                };
                let Some(attr) = order else {
                    self.insert(parent, n);
                    return Ok(());
                };
                self.insert_sorted(parent, n, attr)?;
            }
            Command::Remove { nodes } => {
                for n in nodes {
                    self.remove(n)?;
                }
            }
            Command::Paste { diagram, xml, dx, dy } => self.paste(diagram, &xml, dx, dy)?,
            Command::ConvertToBox { diagram, boxes, name } => self.convert_to_box(diagram, &boxes, &name)?,
            Command::StoreKeys { timeline, frame, group, angles } => self.store_keys(timeline, frame, &group, &angles)?,
            Command::RemoveKeys { timeline, frame, group, to } => {
                self.timeline(timeline)?;
                let actuators = crate::motion::group(&choren_robot::Robot::load("nao6")?, &group)?;
                let doomed: Vec<NodeId> = curves(self.d, timeline).into_iter().filter(|&c| actuators.contains(&self.d.attr_or(c, "actuator"))).flat_map(|c| keys_in(self.d, c, frame, to.unwrap_or(frame))).collect();
                doomed.into_iter().for_each(|k| self.detach(k));
            }
            Command::MoveKeys { keys, delta } => self.shift(&keys, delta, ("Key", "frame", 0))?,
            Command::TranslateKeyframes { keyframes, delta } => self.shift(&keyframes, delta, ("BehaviorKeyframe", "index", 1))?,
            Command::PasteKeyframes { layer, xml, index } => self.paste_keyframes(layer, &xml, index)?,
            Command::SetTimelineRange { timeline, start, end } => {
                self.timeline(timeline)?;
                rules::timeline_range(start, end)?;
                self.set(timeline, "start_frame", Some(&start.to_string()));
                self.set(timeline, "end_frame", Some(&end.to_string()));
            }
            Command::StretchTimelineRange { timeline, start, end } => self.stretch(timeline, start, end)?,
            Command::SetCurveValues { keys, scale, offset } => {
                if !scale.is_finite() || !offset.is_finite() { return Err("Curve transforms must be finite.".into()); }
                for k in keys {
                    if !self.d.is_el(k, "Key") {
                        return Err("Only curve keys have values".into());
                    }
                    let value = self.d.attr_num(k, "value", 0.0) * scale + offset;
                    if !value.is_finite() { return Err("The transformed curve value is too large.".into()); }
                    let v = crate::motion::fmt_val(value);
                    self.set(k, "value", Some(&v));
                    for t in self.d.kids(k, "Tangent").collect::<Vec<_>>() {
                        let ordinate = self.d.attr_num(t, "ordinateParam", 0.0) * scale;
                        if !ordinate.is_finite() { return Err("The transformed tangent is too large.".into()); }
                        let o = crate::motion::fmt_val(ordinate);
                        self.set(t, "ordinateParam", Some(&o));
                    }
                }
            }
            Command::MoveTangent { tangent, abscissa, ordinate } => {
                if !self.d.is_el(tangent, "Tangent") || self.d.attr_raw(tangent, "interpType") != Some("bezier") {
                    return Err("Only Bezier tangents can be moved".into());
                }
                let f = crate::motion::fmt_val;
                let values = BTreeMap::from([("abscissaParam".to_string(), Some(f(abscissa))), ("ordinateParam".to_string(), Some(f(ordinate)))]);
                validate_attrs(self.d, tangent, &values)?;
                if !abscissa.is_finite() || !ordinate.is_finite() { return Err("Tangent coordinates must be finite.".into()); }
                values.iter().for_each(|(k, v)| self.set(tangent, k, v.as_deref()));
                let key = self.d.nodes[tangent].parent.ok_or("Tangent has no key")?;
                let symmetrical = self.d.attr_raw(key, "symmetrical") == Some("1");
                if symmetrical || self.d.attr_raw(key, "smooth") == Some("1") {
                    let other = self.d.kids(key, "Tangent").find(|&n| n != tangent && self.d.attr_raw(n, "interpType") == Some("bezier"));
                    if let Some(other) = other {
                        let dx = if symmetrical { -abscissa } else { self.d.attr_num(other, "abscissaParam", 0.0) };
                        let dy = if symmetrical { -ordinate } else if abscissa != 0.0 { dx * ordinate / abscissa } else { 0.0 };
                        if !dy.is_finite() { return Err("The coupled tangent is too large.".into()); }
                        self.set(other, "abscissaParam", Some(&f(dx)));
                        self.set(other, "ordinateParam", Some(&f(dy)));
                    }
                }
            }
            Command::PasteCurveKeys { timeline, xml, frame } => self.paste_curve_keys(timeline, &xml, frame)?,
            Command::SimplifyCurves { timeline, curves: only, from, to, tolerance, mode, advanced } => {
                self.timeline(timeline)?;
                if tolerance.is_nan() || tolerance < 0.0 {
                    return Err("The error margin must not be negative.".into());
                }
                if mode == SimplifyMode::Choren && advanced.is_some() {
                    return Err("Advanced mode is only in the Choregraphe-compatible simplifier.".into());
                }
                let all = curves(self.d, timeline);
                if only.iter().any(|c| !all.contains(c)) {
                    return Err("Select curves of this timeline.".into());
                }
                let fps = self.d.attr_num(timeline, "fps", 25.0) as i32;
                for c in all.into_iter().filter(|c| only.is_empty() || only.contains(c)) {
                    if mode == SimplifyMode::Choren {
                        let keys = keys_in(self.d, c, from, to);
                        let pts: Vec<(f64, f64)> = keys.iter().map(|&k| (self.d.attr_num(k, "frame", 0.0), self.d.attr_num(k, "value", 0.0))).collect();
                        let keep = crate::motion::rdp_simplify(&pts, tolerance);
                        keys.iter().zip(keep).filter(|(_, keep)| !keep).for_each(|(&k, _)| self.detach(k));
                    } else {
                        self.simplify_curve(c, from, to, tolerance as f32, advanced.map(|(a, g)| (a as f32, g as f32)), fps)?;
                    }
                }
            }
            Command::SetInterpolation { keys, interp } => {
                for k in keys {
                    self.set_interpolation(k, interp)?;
                }
            }
            Command::Mirror { timeline, mode, frames: None } => self.mirror(timeline, mode)?,
            Command::Mirror { timeline, mode, frames: Some(range) } => self.mirror_range(timeline, mode, range)?,
            Command::RecordKeys { timeline, frame, values } => {
                for c in crate::record::store_keys(self.d, timeline, frame, &values, crate::record::NAO_MODELS)? {
                    self.command(c)?;
                }
            }
        }
        Ok(())
    }

    /// Simplify on one curve: the keys in `from..=to` as the simplifier holds them (a
    /// `Tangent` as params against its neighbour, an automatic side as a third of the way at the
    /// automatic slope), then the removals, then the `Tangent`s of the keys whose params changed
    /// rewritten against their new neighbours, as Choregraphe saves them. Advanced mode builds
    /// every range key anew: no smooth or symmetrical flag, and each side
    /// between two range keys a Bezier `Tangent`.
    fn simplify_curve(&mut self, curve: NodeId, from: i64, to: i64, margin: f32, advanced: Option<(f32, f32)>, fps: i32) -> Result<(), String> {
        let d = &*self.d;
        let all: Vec<NodeId> = d.kids(curve, "Key").collect();
        let frame = |k: NodeId| d.attr_num(k, "frame", 0.0) as i32;
        let point = |i: usize| (frame(all[i]), d.attr_num(all[i], "value", 0.0) as f32);
        let range: Vec<usize> = (0..all.len()).filter(|&i| (from..=to).contains(&(frame(all[i]) as i64))).collect();
        let keys: Vec<crate::motion::SimplifyKey> = range
            .iter()
            .map(|&i| {
                let (prev, cur, next) = (i.checked_sub(1).map(point), point(i), (i + 1 < all.len()).then(|| point(i + 1)));
                let slope = prev.zip(next).map_or(0.0, |(p, n)| crate::motion::auto_slope(p, cur, n));
                let side = |name: &str, neighbour: i32| match d.kids(all[i], "Tangent").find(|&t| d.attr_raw(t, "side") == Some(name)) {
                    Some(t) => crate::motion::tangent_params(cur.0, neighbour, (d.attr_num(t, "abscissaParam", 0.0) as f32, d.attr_num(t, "ordinateParam", 0.0) as f32)),
                    None => (crate::motion::THIRD, slope),
                };
                let (left, right) = (side("left", prev.map_or(0, |p| p.0)), side("right", next.map_or(-1, |n| n.0)));
                crate::motion::SimplifyKey { frame: cur.0, value: cur.1, left, right }
            })
            .collect();
        // the range keys that stay, with their new params (basic mode: `None` unless they changed)
        let (keep, new): (Vec<bool>, Vec<Option<crate::motion::SimplifyKey>>) = match advanced {
            None => {
                let (keep, params) = crate::motion::simplify(&keys, margin, fps);
                (keep, keys.iter().zip(params).map(|(&k, ps)| ps.map(|ps| crate::motion::SimplifyKey { left: ps, right: ps, ..k })).collect())
            }
            Some((angularity, granularity)) => {
                let new = crate::motion::advanced_simplify(&keys, margin, angularity, granularity, fps);
                (new.iter().map(Option::is_some).collect(), new)
            }
        };
        range.iter().zip(&keep).filter(|(_, keep)| !**keep).for_each(|(&i, _)| self.detach(all[i]));
        let left: Vec<NodeId> = self.d.kids(curve, "Key").collect();
        let frames: Vec<i32> = left.iter().map(|&k| self.d.attr_num(k, "frame", 0.0) as i32).collect();
        let f = |v: f32| crate::motion::fmt_val(v as f64);
        for (j, (&i, key)) in range.iter().zip(new).enumerate() {
            let (Some(key), Some(at)) = (key, left.iter().position(|&k| k == all[i])) else { continue };
            let (prev, next) = (at.checked_sub(1).map_or(0, |p| frames[p]), frames.get(at + 1).copied().unwrap_or(-1));
            if advanced.is_some() {
                self.set(all[i], "smooth", None);
                self.set(all[i], "symmetrical", None);
            }
            let tangent = |d: &Doc, name: &str| d.kids(all[i], "Tangent").find(|&t| d.attr_raw(t, "side") == Some(name));
            for (side, ps, neighbour, inner) in [("left", key.left, prev, j > 0), ("right", key.right, next, j + 1 < range.len())] {
                let (dx, dy) = crate::motion::tangent_offset(key.frame, neighbour, ps);
                let t = match tangent(self.d, side) {
                    Some(t) => t,
                    None if advanced.is_some() && inner => {
                        let t = self.fragment(&format!("<Tangent side=\"{side}\" interpType=\"bezier\" abscissaParam=\"{}\" ordinateParam=\"{}\" />", f(dx), f(dy)), "Tangent")?;
                        match tangent(self.d, "right").filter(|_| side == "left") {
                            Some(right) => self.insert_before(all[i], t, right),
                            None => self.insert(all[i], t),
                        }
                        continue;
                    }
                    None => continue,
                };
                if advanced.is_some() && inner {
                    self.set(t, "interpType", Some("bezier"));
                }
                self.set(t, "abscissaParam", Some(&f(dx)));
                self.set(t, "ordinateParam", Some(&f(dy)));
            }
        }
        Ok(())
    }

    /// Inserts a keyframe or curve key before the first sibling with a larger `attr`.
    fn insert_sorted(&mut self, parent: NodeId, n: NodeId, attr: &str) -> Result<(), String> {
        let c = self.d.name(n).to_string();
        let at = self.d.attr_num(n, attr, 0.0);
        if self.d.kids(parent, &c).any(|k| self.d.attr_num(k, attr, -1.0) == at) {
            return Err(if c == "Key" { "There is already a key at this frame.".into() } else { "Index not available.".into() });
        }
        let next = self.d.kids(parent, &c).find(|&k| self.d.attr_num(k, attr, 0.0) > at);
        match next {
            Some(next) => self.insert_before(parent, n, next),
            None => self.insert(parent, n),
        }
        Ok(())
    }

    fn timeline(&self, n: NodeId) -> Result<(), String> {
        if self.d.is_el(n, "Timeline") { Ok(()) } else { Err("No timeline selected".into()) }
    }

    fn convert_to_box(&mut self, diagram: NodeId, boxes: &[NodeId], name: &str) -> Result<(), String> {
        self.diagram(diagram)?;
        rules::box_name(name)?;
        if boxes.is_empty() || boxes.iter().any(|&b| !self.d.is_el(b, "Box") || self.d.nodes[b].parent != Some(diagram)) {
            return Err("Select boxes of the current diagram to convert them into a box.".into());
        }
        let d = &*self.d;
        let id_of = |b: NodeId| d.attr_num(b, "id", 0.0) as i64;
        let inside: HashMap<i64, NodeId> = boxes.iter().map(|&b| (id_of(b), b)).collect();
        let x = boxes.iter().map(|&b| d.attr_num(b, "x", 0.0)).fold(f64::MAX, f64::min);
        let y = boxes.iter().map(|&b| d.attr_num(b, "y", 0.0)).fold(f64::MAX, f64::min);
        let (mut internal, mut incoming, mut outgoing) = (vec![], vec![], vec![]);
        for l in d.kids(diagram, "Link") {
            let g = |k| d.attr_num(l, k, -1.0) as i64;
            let link = (g("outputowner"), g("indexofoutput"), g("inputowner"), g("indexofinput"));
            match (inside.contains_key(&link.0), inside.contains_key(&link.2)) {
                (true, true) => internal.push(d.subtree_string(l)),
                (false, true) => incoming.push(link),
                (true, false) => outgoing.push(link),
                _ => {}
            }
        }
        // One boundary port per inner end; the port copies that end's type and nature.
        let mut ports: Vec<(PortKind, (i64, i64), String, BTreeMap<String, String>)> = vec![];
        let ends = incoming.iter().map(|l| (PortKind::Input, (l.2, l.3))).chain(outgoing.iter().map(|l| (PortKind::Output, (l.0, l.1))));
        for (kind, end) in ends {
            if ports.iter().any(|p| p.0 == kind && p.1 == end) {
                continue;
            }
            let b = inside[&end.0];
            let port = rules::ports(d, b).find(|&p| d.attr_num(p, "id", -1.0) as i64 == end.1).ok_or("Link port not found.")?;
            let clean = |s: String| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect::<String>();
            let base = format!("{}_{}", clean(d.attr_or(b, "name")), clean(d.attr_or(port, "name")));
            let taken = |n: &str| ports.iter().any(|p| p.2 == n) || ["onLoad", "onStart", "onStop", "onStopped"].contains(&n);
            let pname = if taken(&base) { (1..).map(|i| format!("{base}_{i}")).find(|n| !taken(n)).unwrap() } else { base };
            let nature = d.attr_num(port, "nature", 0.0) as i64;
            let (nature, tip) = match kind {
                PortKind::Input => (if d.is_el(port, "Input") && (1..=3).contains(&nature) { nature } else { 1 }, "input"),
                _ => (if nature == 1 { 1 } else { 2 }, "output"),
            };
            let values = BTreeMap::from([
                ("type".to_string(), rules::port_type(d, port).to_string()),
                ("nature".to_string(), nature.to_string()),
                ("tooltip".to_string(), format!("This {tip} has been automatically generated\nby converting several boxes into a single box.")),
            ]);
            ports.push((kind, end, pname, values));
        }
        let copies: Vec<String> = boxes.iter().map(|&b| d.subtree_string(b)).collect();
        for &b in boxes {
            self.remove(b)?;
        }
        let nb = self.fragment(&crate::templates::new_box(NewBoxKind::Diagram, name), "Box")?;
        let new_id = self.add_box(diagram, nb, x, y)?;
        let inner = inner_diagrams(self.d, nb)[0];
        // Inner boxes keep their ids: the new diagram is empty.
        for xml in copies.iter().chain(&internal) {
            let c = self.d.parse_fragment(xml)?;
            self.insert(inner, c);
        }
        let mut ids = HashMap::new();
        for (kind, end, pname, values) in ports {
            self.command(Command::AddPort { r#box: nb, kind, name: pname.clone(), values })?;
            let pid = rules::ports(self.d, nb).find(|&p| self.d.attr(p, "name").as_deref() == Some(&pname)).map(|p| self.d.attr_num(p, "id", 0.0) as i64).unwrap();
            let (o, i) = if kind == PortKind::Input { ((0, pid), end) } else { (end, (0, pid)) };
            self.command(Command::AddLink { diagram: inner, out_owner: o.0, out_port: o.1, in_owner: i.0, in_port: i.1 })?;
            ids.insert((kind == PortKind::Input, end), pid);
        }
        for (oo, op, io, ip) in incoming {
            self.command(Command::AddLink { diagram, out_owner: oo, out_port: op, in_owner: new_id, in_port: ids[&(true, (io, ip))] })?;
        }
        for (oo, op, io, ip) in outgoing {
            self.command(Command::AddLink { diagram, out_owner: new_id, out_port: ids[&(false, (oo, op))], in_owner: io, in_port: ip })?;
        }
        Ok(())
    }

    fn store_keys(&mut self, timeline: NodeId, frame: i64, group: &str, angles: &[f64]) -> Result<(), String> {
        self.timeline(timeline)?;
        if frame < 0 {
            return Err("Frame must not be negative.".into());
        }
        let robot = choren_robot::Robot::load("nao6")?;
        let mut values = vec![];
        for name in crate::motion::group(&robot, group)? {
            let i = robot.joint_index(&name).unwrap();
            let a = *angles.get(i).ok_or("The robot state has no value for every joint.")?;
            // Timelines store degrees; the hands store their opening ratio.
            values.push((name, if robot.joints[i].axis == [0.0; 3] { a } else { a.to_degrees() }));
        }
        for (actuator, v) in &values {
            let joint = &robot.joints[robot.joint_index(actuator).unwrap()];
            let value = crate::motion::fmt_val(*v);
            let curve = self.curve_for(timeline, actuator, if joint.axis == [0.0; 3] { "1" } else { "0" })?;
            match key_at(self.d, curve, frame) {
                Some(k) => self.set(k, "value", Some(&value)),
                None => {
                    let k = self.fragment(&format!("<Key frame=\"{frame}\" value=\"{value}\" />"), "Key")?;
                    self.insert_sorted(curve, k, "frame")?;
                }
            }
        }
        Ok(())
    }

    /// Moves curve keys (`frame` ≥ 0) or behavior keyframes (`index` ≥ 1) by `delta`, keeping
    /// their parents sorted.
    fn shift(&mut self, nodes: &[NodeId], delta: i64, (tag, attr, min): (&str, &str, i64)) -> Result<(), String> {
        if nodes.iter().any(|&k| !self.d.is_el(k, tag)) {
            return Err(if tag == "Key" { "Only curve keys can be moved".into() } else { "Only behavior keyframes can be moved".into() });
        }
        let moved = nodes.iter().map(|&k| (k, self.d.attr_num(k, attr, 0.0) as i64 + delta)).collect();
        self.place(moved, attr, min)
    }

    /// Detaches every node, then reinserts each at its new `attr` value (no collisions among them).
    fn place(&mut self, moved: Vec<(NodeId, i64)>, attr: &str, min: i64) -> Result<(), String> {
        if moved.iter().any(|&(_, v)| v < min) {
            return Err(if min == 0 { "Frame must not be negative.".into() } else { "Index must be at least 1.".into() });
        }
        let moved: Vec<_> = moved.into_iter().map(|(k, v)| (self.d.nodes[k].parent.unwrap(), k, v)).collect();
        for &(_, k, _) in &moved {
            self.detach(k);
        }
        for (parent, k, v) in moved {
            self.set(k, attr, Some(&v.to_string()));
            self.insert_sorted(parent, k, attr)?;
        }
        Ok(())
    }

    fn stretch(&mut self, timeline: NodeId, start: i64, end: i64) -> Result<(), String> {
        self.timeline(timeline)?;
        rules::timeline_range(start, end)?;
        let size = self.d.attr_num(timeline, "size", 0.0) as i64;
        let end_of = |e: i64| if e == -1 { size } else { e };
        let (s0, e0) = (self.d.attr_num(timeline, "start_frame", 1.0) as i64, end_of(self.d.attr_num(timeline, "end_frame", -1.0) as i64));
        let e1 = end_of(end);
        if e0 <= s0 || e1 <= start {
            return Err("The timeline range is empty.".into());
        }
        let factor = (e1 - start) as f64 / (e0 - s0) as f64;
        let map = |v: f64| start + ((v - s0 as f64) * factor).round() as i64;
        let keys: Vec<NodeId> = curves(self.d, timeline).into_iter().flat_map(|c| keys_in(self.d, c, s0, e0)).collect();
        for &k in &keys {
            for t in self.d.kids(k, "Tangent").collect::<Vec<_>>() {
                let a = crate::motion::fmt_val(self.d.attr_num(t, "abscissaParam", 0.0) * factor);
                self.set(t, "abscissaParam", Some(&a));
            }
        }
        let moved = keys.into_iter().map(|k| (k, map(self.d.attr_num(k, "frame", 0.0)))).collect();
        self.place(moved, "frame", 0)?;
        let frames: Vec<NodeId> = self.d.kids(timeline, "BehaviorLayer").flat_map(|l| self.d.kids(l, "BehaviorKeyframe").collect::<Vec<_>>()).collect();
        let moved = frames.into_iter().map(|k| (k, self.d.attr_num(k, "index", 0.0) as i64)).filter(|&(_, i)| (s0..=e0).contains(&i)).map(|(k, i)| (k, map(i as f64))).collect();
        self.place(moved, "index", 1)?;
        self.set(timeline, "start_frame", Some(&start.to_string()));
        self.set(timeline, "end_frame", Some(&end.to_string()));
        // size -1 is computed when Choregraphe saves (max of last key frame and end frame).
        if size != -1 && e1 > size {
            self.set(timeline, "size", Some(&e1.to_string()));
        }
        Ok(())
    }

    fn set_dialog_file(&mut self, b: NodeId, file: &str) -> Result<(), String> {
        if !self.d.is_el(b, "Box") {
            return Err("Dialog files belong to boxes".into());
        }
        crate::project::from_behavior(self.behavior, file)?;
        let file = crate::project::normalize(file);
        if !file.ends_with(".dlg") {
            return Err("Choose a dialog (.dlg) file.".into());
        }
        let raw = crate::xml::escape_text(&file);
        let text = self.d.kid(b, "dialogFile").and_then(|el| self.d.el(el).children.iter().copied().find(|&c| matches!(self.d.nodes[c].kind, Kind::Text(_))));
        match text {
            Some(t) => {
                let Kind::Text(before) = self.d.nodes[t].kind.clone() else { unreachable!() };
                self.push(Op::Text { node: t, before, after: raw });
            }
            None => {
                if let Some(el) = self.d.kid(b, "dialogFile") {
                    self.detach(el);
                }
                let el = self.fragment(&format!("<dialogFile>{raw}</dialogFile>"), "dialogFile")?;
                self.insert(b, el);
            }
        }
        Ok(())
    }

    /// The timeline's curve for `actuator`, created (with the `ActuatorList`) when missing.
    fn curve_for(&mut self, timeline: NodeId, actuator: &str, unit: &str) -> Result<NodeId, String> {
        if let Some(c) = curves(self.d, timeline).into_iter().find(|&c| self.d.attr(c, "actuator").as_deref() == Some(actuator)) {
            return Ok(c);
        }
        let list = match self.d.kid(timeline, "ActuatorList") {
            Some(l) => l,
            None => {
                let l = self.fragment("<ActuatorList model=\"\" />", "ActuatorList")?;
                self.insert(timeline, l);
                l
            }
        };
        let c = self.fragment(&format!("<ActuatorCurve name=\"value\" actuator=\"{}\" recordable=\"1\" mute=\"0\" unit=\"{}\" />", escape_attr(actuator), escape_attr(unit)), "ActuatorCurve")?;
        self.insert(list, c);
        Ok(c)
    }

    fn paste_curve_keys(&mut self, timeline: NodeId, xml: &str, frame: i64) -> Result<(), String> {
        self.timeline(timeline)?;
        let clip = self.fragment(xml, "ChorenCurveKeys")?;
        let groups: Vec<NodeId> = self.d.kids(clip, "ActuatorCurve").collect();
        let first = groups.iter().flat_map(|&g| self.d.kids(g, "Key").collect::<Vec<_>>()).map(|k| self.d.attr_num(k, "frame", 0.0) as i64).min().unwrap_or(0);
        for g in groups {
            let curve = self.curve_for(timeline, &self.d.attr_or(g, "actuator"), &self.d.attr_or(g, "unit"))?;
            for k in self.d.kids(g, "Key").collect::<Vec<_>>() {
                let at = frame + self.d.attr_num(k, "frame", 0.0) as i64 - first;
                if at < 0 {
                    return Err("Frame must not be negative.".into());
                }
                if let Some(old) = key_at(self.d, curve, at) {
                    self.detach(old);
                }
                self.d.el_mut(g).children.retain(|&c| c != k);
                self.d.nodes[k].parent = None;
                self.set(k, "frame", Some(&at.to_string()));
                self.insert_sorted(curve, k, "frame")?;
            }
        }
        Ok(())
    }

    fn paste_keyframes(&mut self, layer: NodeId, xml: &str, index: i64) -> Result<(), String> {
        if !self.d.is_el(layer, "BehaviorLayer") {
            return Err("No behavior layer selected".into());
        }
        let clip = self.fragment(xml, "ChorenKeyframes")?;
        let frames: Vec<NodeId> = self.d.kids(clip, "BehaviorKeyframe").collect();
        let first = frames.iter().map(|&k| self.d.attr_num(k, "index", 1.0) as i64).min().unwrap_or(1);
        // Pasted keyframes keep their names, and a
        // keyframe whose index is taken moves to the next free one.
        for k in frames {
            let mut at = index + self.d.attr_num(k, "index", 1.0) as i64 - first;
            if at < 1 {
                return Err("Index must be at least 1.".into());
            }
            while self.d.kids(layer, "BehaviorKeyframe").any(|s| self.d.attr_num(s, "index", 1.0) as i64 == at) {
                at += 1;
            }
            self.d.el_mut(clip).children.retain(|&c| c != k);
            self.d.nodes[k].parent = None;
            self.set(k, "index", Some(&at.to_string()));
            self.insert_sorted(layer, k, "index")?;
        }
        Ok(())
    }

    /// Mirror or Flip limited to the keys in `first..=last`: keys move (Flip) or are copied
    /// (Left/Right) to the partner curve, whose keys in the range they replace.
    fn mirror_range(&mut self, timeline: NodeId, mode: MirrorMode, (first, last): (i64, i64)) -> Result<(), String> {
        self.timeline(timeline)?;
        let robot = choren_robot::Robot::load("nao6")?;
        let (from, to) = match mode {
            MirrorMode::Flip => ("", ""),
            MirrorMode::LeftToRight => ("L", "R"),
            MirrorMode::RightToLeft => ("R", "L"),
        };
        let (mut doomed, mut moves) = (vec![], vec![]);
        for c in curves(self.d, timeline) {
            let actuator = self.d.attr_or(c, "actuator");
            let keys = keys_in(self.d, c, first, last);
            let negate = crate::motion::mirror_negates(&robot, &actuator);
            match (robot.mirror.get(&actuator), mode) {
                (None, MirrorMode::Flip) if negate => keys.into_iter().for_each(|k| self.negate_key(k)),
                (Some(p), MirrorMode::Flip) => moves.extend(keys.into_iter().map(|k| (k, p.clone(), self.d.attr_or(c, "unit"), negate))),
                (Some(p), _) if actuator.starts_with(from) => {
                    for k in keys {
                        let copy = self.d.parse_fragment(&self.d.subtree_string(k))?;
                        moves.push((copy, p.clone(), self.d.attr_or(c, "unit"), negate));
                    }
                }
                (Some(_), _) if actuator.starts_with(to) => doomed.extend(keys),
                _ => {}
            }
        }
        doomed.iter().chain(moves.iter().map(|m| &m.0)).for_each(|&k| self.detach(k));
        for (k, partner, unit, negate) in moves {
            let curve = self.curve_for(timeline, &partner, &unit)?;
            if negate {
                self.negate_key(k);
            }
            self.insert_sorted(curve, k, "frame")?;
        }
        Ok(())
    }

    fn negate_key(&mut self, k: NodeId) {
        let v = crate::motion::negate(self.d.attr_raw(k, "value").unwrap_or("0"));
        self.set(k, "value", Some(&v));
        for t in self.d.kids(k, "Tangent").collect::<Vec<_>>() {
            let v = crate::motion::negate(self.d.attr_raw(t, "ordinateParam").unwrap_or("0"));
            self.set(t, "ordinateParam", Some(&v));
        }
    }

    fn set_interpolation(&mut self, k: NodeId, interp: Interp) -> Result<(), String> {
        if !self.d.is_el(k, "Key") {
            return Err("Interpolation applies to curve keys".into());
        }
        for t in self.d.kids(k, "Tangent").collect::<Vec<_>>() {
            self.detach(t);
        }
        let curve = self.d.nodes[k].parent.unwrap();
        let keys: Vec<NodeId> = self.d.kids(curve, "Key").collect();
        let i = keys.iter().position(|&x| x == k).unwrap();
        let pt = |n: NodeId| (self.d.attr_num(n, "frame", 0.0), self.d.attr_num(n, "value", 0.0));
        let (left, right) = crate::motion::auto_tangents(i.checked_sub(1).map(|j| pt(keys[j])), pt(k), keys.get(i + 1).map(|&n| pt(n)));
        let (name, left, right) = match interp {
            Interp::BezierAuto => return Ok(()),
            Interp::Constant => ("constant", (0.0, 0.0), (0.0, 0.0)),
            Interp::Linear => ("linear", (0.0, 0.0), (0.0, 0.0)),
            Interp::Bezier => ("bezier", left, right),
        };
        for (side, (x, y)) in [("left", left), ("right", right)] {
            let f = crate::motion::fmt_val;
            let t = self.fragment(&format!("<Tangent side=\"{side}\" interpType=\"{name}\" abscissaParam=\"{}\" ordinateParam=\"{}\" />", f(x), f(y)), "Tangent")?;
            self.insert(k, t);
        }
        Ok(())
    }

    fn mirror(&mut self, timeline: NodeId, mode: MirrorMode) -> Result<(), String> {
        self.timeline(timeline)?;
        let robot = choren_robot::Robot::load("nao6")?;
        let side = |c: NodeId, d: &Doc| d.attr_or(c, "actuator");
        let all = curves(self.d, timeline);
        let (from, to) = match mode {
            MirrorMode::Flip => ('_', '_'),
            MirrorMode::LeftToRight => ('L', 'R'),
            MirrorMode::RightToLeft => ('R', 'L'),
        };
        if mode != MirrorMode::Flip {
            let doomed: Vec<NodeId> = all.iter().copied().filter(|&c| side(c, self.d).starts_with(to) && robot.mirror.contains_key(&side(c, self.d))).collect();
            doomed.into_iter().for_each(|c| self.detach(c));
        }
        for c in all {
            let actuator = side(c, self.d);
            let partner = robot.mirror.get(&actuator).cloned();
            let target = match mode {
                MirrorMode::Flip => c,
                _ if actuator.starts_with(from) && partner.is_some() => {
                    let copy = self.d.parse_fragment(&self.d.subtree_string(c))?;
                    self.insert(self.d.nodes[c].parent.unwrap(), copy);
                    copy
                }
                _ => continue,
            };
            if let Some(p) = &partner {
                self.set(target, "actuator", Some(p));
            } else if mode != MirrorMode::Flip {
                continue;
            }
            if crate::motion::mirror_negates(&robot, &actuator) {
                self.d.kids(target, "Key").collect::<Vec<_>>().into_iter().for_each(|k| self.negate_key(k));
            }
        }
        Ok(())
    }

    fn set_script(&mut self, b: NodeId, text: &str) -> Result<(), String> {
        if !self.d.is_el(b, "Box") {
            return Err("Scripts belong to boxes".into());
        }
        if text.contains("]]>") {
            return Err("Script cannot contain \"]]>\"".into());
        }
        let content = match self.d.kid(b, "script").and_then(|s| self.d.kid(s, "content")) {
            Some(c) => c,
            None => {
                let s = self.fragment("<script language=\"4\">\n    <content><![CDATA[]]></content>\n</script>", "script")?;
                self.insert(b, s);
                self.d.kid(s, "content").unwrap()
            }
        };
        let kids = self.d.el(content).children.clone();
        match kids.iter().copied().find(|&k| matches!(self.d.nodes[k].kind, Kind::CData(_))) {
            Some(cd) => {
                let Kind::CData(before) = self.d.nodes[cd].kind.clone() else { unreachable!() };
                self.push(Op::Text { node: cd, before, after: text.to_string() });
            }
            None => {
                self.d.nodes.push(crate::xml::Node { parent: None, kind: Kind::CData(text.to_string()) });
                let cd = self.d.nodes.len() - 1;
                self.push(Op::Child { parent: content, index: kids.len(), node: cd, insert: true });
            }
        }
        Ok(())
    }

    fn remove(&mut self, n: NodeId) -> Result<(), String> {
        if self.d.nodes[n].parent.is_none() {
            return Ok(()); // already removed in this transaction
        }
        let tag = self.d.name(n).to_string();
        match tag.as_str() {
            "Box" => {
                let diagram = self.d.nodes[n].parent.unwrap();
                if !self.d.is_el(diagram, "Diagram") {
                    return Err("The root box cannot be deleted".into());
                }
                let id = self.d.attr_num(n, "id", 0.0) as i64;
                let links: Vec<_> = self.d.kids(diagram, "Link").filter(|&l| owner_is(self.d, l, id, None)).collect();
                links.into_iter().for_each(|l| self.detach(l));
            }
            "Input" | "Output" | "Parameter" => {
                // LinksBreaker: drop links to this port in the parent diagram and on the box's own rails.
                let b = self.d.nodes[n].parent.unwrap();
                let (bid, pid) = (self.d.attr_num(b, "id", 0.0) as i64, self.d.attr_num(n, "id", -1.0) as i64);
                let mut doomed = vec![];
                if let Some(diagram) = self.d.nodes[b].parent.filter(|&p| self.d.is_el(p, "Diagram")) {
                    doomed.extend(self.d.kids(diagram, "Link").filter(|&l| owner_is(self.d, l, bid, Some(pid))));
                }
                for diagram in inner_diagrams(self.d, b) {
                    doomed.extend(self.d.kids(diagram, "Link").filter(|&l| owner_is(self.d, l, 0, Some(pid))));
                }
                doomed.into_iter().for_each(|l| self.detach(l));
            }
            _ => {}
        }
        self.detach(n);
        Ok(())
    }

    /// Each pasted box is renamed by [`unique_box_name`] against the target's boxes and the boxes
    /// pasted before it, then added in order keeping its id when free.
    fn paste(&mut self, diagram: NodeId, xml: &str, dx: f64, dy: f64) -> Result<(), String> {
        self.diagram(diagram)?;
        let clip = self.fragment(xml, "ChorenClipboard")?;
        let mut ids = HashMap::new();
        let mut names: Vec<String> = self.d.kids(diagram, "Box").map(|b| self.d.attr(b, "name").unwrap_or_default()).collect();
        for b in self.d.kids(clip, "Box").collect::<Vec<_>>() {
            let old = self.d.attr_num(b, "id", 0.0) as i64;
            let (x, y) = (self.d.attr_num(b, "x", 0.0) + dx, self.d.attr_num(b, "y", 0.0) + dy);
            self.d.el_mut(clip).children.retain(|&c| c != b);
            self.d.nodes[b].parent = None;
            let name = unique_box_name(&self.d.attr(b, "name").unwrap_or_default(), &names);
            ids.insert(old, self.place_box(diagram, b, x, y, Some(old))?);
            self.set(b, "name", Some(&name));
            names.push(name);
        }
        for l in self.d.kids(clip, "Link").collect::<Vec<_>>() {
            let g = |k| self.d.attr_num(l, k, 0.0) as i64;
            if let (Some(&o), Some(&i)) = (ids.get(&g("outputowner")), ids.get(&g("inputowner"))) {
                self.command(Command::AddLink { diagram, out_owner: o, out_port: g("indexofoutput"), in_owner: i, in_port: g("indexofinput") })?;
            }
        }
        Ok(())
    }
}

fn owner_is(d: &Doc, link: NodeId, owner: i64, port: Option<i64>) -> bool {
    let g = |k| d.attr_num(link, k, -1.0) as i64;
    (g("outputowner") == owner && port.is_none_or(|p| g("indexofoutput") == p)) || (g("inputowner") == owner && port.is_none_or(|p| g("indexofinput") == p))
}

fn curves(d: &Doc, timeline: NodeId) -> Vec<NodeId> {
    d.kids(timeline, "ActuatorList").flat_map(|l| d.kids(l, "ActuatorCurve").collect::<Vec<_>>()).collect()
}

fn key_at(d: &Doc, curve: NodeId, frame: i64) -> Option<NodeId> {
    d.kids(curve, "Key").find(|&k| d.attr_num(k, "frame", -1.0) as i64 == frame)
}

fn keys_in(d: &Doc, curve: NodeId, first: i64, last: i64) -> Vec<NodeId> {
    d.kids(curve, "Key").filter(|&k| (first..=last).contains(&(d.attr_num(k, "frame", -1.0) as i64))).collect()
}

fn inner_diagrams(d: &Doc, b: NodeId) -> Vec<NodeId> {
    d.kids(b, "Timeline")
        .flat_map(|t| d.kids(t, "BehaviorLayer").collect::<Vec<_>>())
        .flat_map(|l| d.kids(l, "BehaviorKeyframe").collect::<Vec<_>>())
        .flat_map(|k| d.kids(k, "Diagram").collect::<Vec<_>>())
        .collect()
}

/// Lowest unused box id ≥ 1.
pub fn first_id_available(d: &Doc, diagram: NodeId) -> i64 {
    (1..).find(|&i| id_available(d, diagram, i)).unwrap()
}

/// Ids start at 1 and are unique among the diagram's boxes.
pub fn id_available(d: &Doc, diagram: NodeId, id: i64) -> bool {
    id >= 1 && !d.kids(diagram, "Box").any(|b| d.attr_num(b, "id", 0.0) as i64 == id)
}

/// Drops a trailing " (n)", then takes the first of `base`,
/// `base (1)`, `base (2)`, … that no box in `taken` is named.
pub fn unique_box_name(name: &str, taken: &[String]) -> String {
    let base = name
        .strip_suffix(')')
        .and_then(|s| s.rsplit_once(" ("))
        .filter(|(b, n)| !b.is_empty() && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        .map_or(name, |(b, _)| b);
    let mut candidate = base.to_string();
    for n in 1.. {
        if !taken.contains(&candidate) {
            break;
        }
        candidate = format!("{base} ({n})");
    }
    candidate
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

fn validate_attrs(d: &Doc, node: NodeId, values: &BTreeMap<String, Option<String>>) -> Result<(), String> {
    let tag = d.name(node);
    if let Some(Some(category)) = values.get("choren_category") {
        if tag != "Box" || !matches!(category.as_str(), "subgraph" | "timeline" | "timer" | "terminal" | "script" | "comment") {
            return Err("Unknown box appearance category".into());
        }
    }
    let get = |k: &str| values.get(k).cloned().unwrap_or_else(|| d.attr(node, k));
    let num = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
    if values.contains_key("id") && matches!(tag, "Box" | "Input" | "Output" | "Parameter") {
        return Err("Ids are managed by Choren".into());
    }
    match (tag, values.get("name")) {
        ("Box", Some(Some(n))) => rules::box_name(n)?,
        ("Input" | "Output" | "Parameter", Some(n)) => rules::io_name(d, d.nodes[node].parent.unwrap(), Some(node), n.as_deref().unwrap_or(""), get("nature").as_deref() == Some("4"))?,
        ("BehaviorLayer" | "BehaviorKeyframe", Some(n)) => rules::sibling_name(d, node, n.as_deref().unwrap_or(""))?,
        _ => {}
    }
    if tag == "BehaviorKeyframe" {
        if let Some(i) = values.get("index") {
            rules::keyframe_index(d, node, i.as_deref().unwrap_or(""))?;
        }
    }
    if tag == "Timeline" {
        if values.contains_key("fps") && !get("fps").and_then(|v| v.parse::<i64>().ok()).is_some_and(|v| v >= 1) {
            return Err("The frame rate must be a whole number of at least 1.".into());
        }
        if values.contains_key("resources_acquisition") && !matches!(get("resources_acquisition").as_deref(), Some("0" | "1" | "2")) {
            return Err("Unknown resources acquisition mode.".into());
        }
        if values.contains_key("start_frame") || values.contains_key("end_frame") {
            let int = |k: &str, d: i64| get(k).map_or(Ok(d), |v| v.parse::<i64>().map_err(|_| "Frames must be whole numbers.".to_string()));
            rules::timeline_range(int("start_frame", 1)?, int("end_frame", -1)?)?;
        }
    }
    if tag == "Key" && values.contains_key("frame") {
        return Err("Move keys to change their frame".into());
    }
    if tag == "Tangent" {
        for key in ["abscissaParam", "ordinateParam"] {
            if values.contains_key(key) && !num(key).is_some_and(f64::is_finite) { return Err("Tangent coordinates must be finite numbers.".into()); }
        }
        let x = num("abscissaParam").unwrap_or(0.0);
        match get("side").as_deref() {
            Some("left") if x > 0.0 => return Err("abscissa of a left Tangent shall be non-positive".into()),
            Some("right") if x < 0.0 => return Err("abscissa of a right Tangent shall be non-negative".into()),
            _ => {}
        }
    }
    if tag == "Input" && values.contains_key("nature") && get("nature").as_deref() == Some("4") && get("stm_value_name").is_none_or(|k| k.is_empty()) {
        return Err("An input from ALMemory needs a memory key.".into());
    }
    if matches!(tag, "Input" | "Output") && values.contains_key("type_size") && !get("type_size").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v > 0) {
        return Err("A port value count must be a positive whole number.".into());
    }
    if tag == "Resource" {
        if values.contains_key("type") && !matches!(get("type").as_deref(), Some("Lock" | "Wait" | "Stop on demand" | "Callback on demand")) { return Err("Unknown resource policy.".into()); }
        if values.contains_key("timeout") && !num("timeout").is_some_and(|v| v.is_finite() && v >= 0.0) { return Err("Resource timeout must be a nonnegative number.".into()); }
    }
    if tag == "Parameter" && matches!(get("content_type").as_deref(), Some("1" | "2")) {
        rules::param_range(num("min"), num("max"), num("default_value"))?;
    }
    Ok(())
}

/// Inserts an element outside the undo history (project files such as the PML and manifest).
pub fn insert_raw(d: &mut Doc, parent: NodeId, xml: &str) -> Result<NodeId, String> {
    let mut tx = Tx { d, behavior: "", ops: vec![], created: vec![] };
    let n = tx.d.parse_fragment(xml)?;
    tx.insert(parent, n);
    Ok(n)
}

/// Removes an element (and its indentation) outside the undo history.
pub fn detach_raw(d: &mut Doc, node: NodeId) {
    Tx { d, behavior: "", ops: vec![], created: vec![] }.detach(node);
}

/// Sets an unescaped attribute outside the undo history.
pub fn set_raw(d: &mut Doc, node: NodeId, key: &str, value: &str) {
    Tx { d, behavior: "", ops: vec![], created: vec![] }.set(node, key, Some(value));
}

/// Serializes boxes (and the links between them), behavior keyframes or curve keys for the
/// clipboard (`Paste`, `PasteKeyframes`, `PasteCurveKeys`), by the first node's type.
pub fn copy(d: &Doc, boxes: &[NodeId]) -> String {
    match boxes.first().filter(|&&n| n < d.nodes.len() && matches!(d.nodes[n].kind, Kind::Element(_))).map(|&n| d.name(n)) {
        Some("BehaviorKeyframe") => return copy_keyframes(d, boxes),
        Some("Key") => return copy_curve_keys(d, boxes),
        _ => {}
    }
    let mut out = String::from("<ChorenClipboard>");
    let mut ids = vec![];
    let mut diagram = None;
    for &b in boxes.iter().filter(|&&b| d.is_el(b, "Box")) {
        out.push_str(&d.subtree_string(b));
        ids.push(d.attr_num(b, "id", 0.0) as i64);
        diagram = d.nodes[b].parent;
    }
    if let Some(diag) = diagram {
        for l in d.kids(diag, "Link") {
            let g = |k| d.attr_num(l, k, -1.0) as i64;
            if ids.contains(&g("outputowner")) && ids.contains(&g("inputowner")) {
                out.push_str(&d.subtree_string(l));
            }
        }
    }
    out.push_str("</ChorenClipboard>");
    out
}

/// Serializes behavior keyframes for `PasteKeyframes`.
fn copy_keyframes(d: &Doc, keyframes: &[NodeId]) -> String {
    let body: String = keyframes.iter().filter(|&&k| d.is_el(k, "BehaviorKeyframe")).map(|&k| d.subtree_string(k)).collect();
    format!("<ChorenKeyframes>{body}</ChorenKeyframes>")
}

/// Serializes curve keys, grouped by actuator, for `PasteCurveKeys`.
fn copy_curve_keys(d: &Doc, keys: &[NodeId]) -> String {
    let mut out = String::from("<ChorenCurveKeys>");
    let mut seen = vec![];
    for c in keys.iter().filter(|&&k| d.is_el(k, "Key")).filter_map(|&k| d.nodes[k].parent) {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        out += &format!("<ActuatorCurve actuator=\"{}\" unit=\"{}\">", escape_attr(&d.attr_or(c, "actuator")), escape_attr(&d.attr_or(c, "unit")));
        d.kids(c, "Key").filter(|k| keys.contains(k)).for_each(|k| out += &d.subtree_string(k));
        out += "</ActuatorCurve>";
    }
    out + "</ChorenCurveKeys>"
}

struct Transaction {
    /// Stable behavior id (list positions shift when a behavior is removed); `None` for steps
    /// that only change project files.
    behavior: Option<String>,
    label: String,
    ops: Vec<Op>,
    merge: Option<String>,
    /// Project text files (topics, translations): (path, content before, content after).
    files: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)>,
    /// PML entries this step added for new files; undo removes them again.
    listings: Vec<crate::project::Listing>,
    snapshot: Option<(Box<Project>, Box<Project>)>,
}

/// Undo/redo stacks with a save point for the dirty flag.
#[derive(Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    saved: Option<usize>,
    pub revision: u64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    pub revision: u64,
    pub dirty: bool,
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub created: Vec<NodeId>,
    pub undo_steps: Vec<String>,
    pub redo_steps: Vec<String>,
}

impl History {
    pub fn state(&self, created: Vec<NodeId>) -> HistoryState {
        HistoryState {
            revision: self.revision,
            dirty: self.saved != Some(self.undo.len()),
            undo: self.undo.last().map(|t| t.label.clone()),
            redo: self.redo.last().map(|t| t.label.clone()),
            created,
            undo_steps: self.undo.iter().map(|t| t.label.clone()).collect(),
            redo_steps: self.redo.iter().rev().map(|t| t.label.clone()).collect(),
        }
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
    }

    /// Drops every step of a removed behavior. Its id may be reused by a new behavior (automatic
    /// names), whose document the old operations must never touch.
    pub fn forget(&mut self, behavior: &str) {
        let before = self.undo.len() + self.redo.len();
        let other = |t: &Transaction| t.behavior.as_deref() != Some(behavior);
        self.undo.retain(other);
        self.redo.retain(other);
        if self.undo.len() + self.redo.len() != before {
            self.saved = None; // the save point no longer lines up with the stack
            self.revision += 1;
        }
    }

    pub fn fresh() -> Self {
        History { saved: Some(0), ..Default::default() }
    }
}

/// Applies `commands` to one behavior as a single undo step. On error nothing changes.
pub fn apply(p: &mut Project, h: &mut History, behavior: &str, label: &str, commands: Vec<Command>) -> Result<HistoryState, String> {
    let bi = p.behaviors.iter().position(|b| b.id == behavior).ok_or("Unknown behavior")?;
    let b = &mut p.behaviors[bi];
    let (d, source) = (&mut b.doc, b.source.as_str());
    let merge = match commands.as_slice() {
        [Command::MoveBoxes { moves }] => Some(format!("move:{:?}", moves.iter().map(|m| m.0).collect::<Vec<_>>())),
        [Command::SetAttributes { node, values }] if values.keys().all(|k| k == "value") => Some(format!("value:{node}")),
        [Command::SetCurveValues { keys, .. }] => Some(format!("curve values:{keys:?}")),
        [Command::MoveTangent { tangent, .. }] => Some(format!("tangent:{tangent}")),
        _ => None,
    };
    let mut tx = Tx { d, behavior: source, ops: vec![], created: vec![] };
    for c in commands {
        if let Err(e) = c.clone().validate_nodes(tx.d).and_then(|_| tx.command(c)) {
            for op in tx.ops.iter().rev() {
                run(tx.d, op, false);
            }
            return Err(e);
        }
    }
    let (ops, created) = (tx.ops, tx.created);
    if ops.is_empty() {
        return Ok(h.state(created));
    }
    h.redo.clear();
    if h.saved.is_some_and(|s| s > h.undo.len()) {
        h.saved = None;
    }
    let at_save = h.saved == Some(h.undo.len());
    let last = h.undo.last_mut().filter(|t| merge.is_some() && t.merge == merge && t.behavior.as_deref() == Some(behavior) && !at_save);
    match last {
        Some(t) => t.ops.extend(ops),
        None => h.undo.push(Transaction { behavior: Some(behavior.to_string()), label: label.into(), ops, merge, files: vec![], listings: vec![], snapshot: None }),
    }
    h.revision += 1;
    Ok(h.state(created))
}

/// Stages new contents for project files (dialog topics, translations) as one undo step; they
/// are written on save. Consecutive edits of the same single file merge.
pub fn set_files(p: &mut Project, h: &mut History, label: &str, files: Vec<(String, Vec<u8>)>) -> Result<HistoryState, String> {
    set_listed_files(p, h, label, files, vec![])
}

/// `set_files` that also lists new files in the PML, in the same undo step.
pub fn set_listed_files(p: &mut Project, h: &mut History, label: &str, files: Vec<(String, Vec<u8>)>, listings: Vec<crate::project::Listing>) -> Result<HistoryState, String> {
    let mut changes = vec![];
    for (path, bytes) in files {
        let path = crate::project::relative(&path)?;
        let before = p.read_file(&path);
        if before.as_ref() != Some(&bytes) {
            changes.push((path, before, Some(bytes)));
        }
    }
    if changes.is_empty() && listings.is_empty() {
        return Ok(h.state(vec![]));
    }
    for l in &listings {
        p.list(l, true)?;
    }
    for (path, _, after) in &changes {
        p.stage(path, after.clone());
    }
    h.redo.clear();
    if h.saved.is_some_and(|s| s > h.undo.len()) {
        h.saved = None;
    }
    let merge = (changes.len() == 1 && listings.is_empty()).then(|| format!("file:{}", changes[0].0));
    let at_save = h.saved == Some(h.undo.len());
    match h.undo.last_mut().filter(|t| merge.is_some() && t.merge == merge && !at_save) {
        Some(t) => t.files[0].2 = changes.pop().unwrap().2,
        None => h.undo.push(Transaction { behavior: None, label: label.into(), ops: vec![], merge, files: changes, listings, snapshot: None }),
    }
    h.revision += 1;
    Ok(h.state(vec![]))
}

/// Applies a project-structure edit atomically while retaining the behavior node identities.
pub fn change_project(p: &mut Project, h: &mut History, label: &str, work: impl FnOnce(&mut Project) -> Result<(), String>) -> Result<(), String> {
    let before = p.clone();
    let mut after = before.clone();
    work(&mut after)?;
    after.structure_dirty = true;
    h.redo.clear();
    if h.saved.is_some_and(|s| s > h.undo.len()) { h.saved = None; }
    // Deleted files are kept here too, so undoing the step restages them after a save.
    let paths: std::collections::BTreeSet<_> = before.pending.keys().chain(after.pending.keys()).chain(after.deleted.difference(&before.deleted)).cloned().collect();
    let files = paths.into_iter().filter_map(|path| {
        let old = before.read_file(&path);
        let new = after.read_file(&path);
        (old != new).then_some((path, old, new))
    }).collect();
    h.undo.push(Transaction { behavior: None, label: label.into(), ops: vec![], merge: None, files, listings: vec![], snapshot: Some((Box::new(before), Box::new(after.clone()))) });
    *p = after;
    h.revision += 1;
    Ok(())
}

fn replay(p: &mut Project, t: &Transaction, forward: bool) {
    if let Some((before, after)) = &t.snapshot {
        // Later file edits may already have been saved and undone. Keep their staged inverse;
        // only this transaction's file differences are replayed below.
        let pending = std::mem::take(&mut p.pending);
        // Saves clear `deleted`; replay only this step's deletions instead of the snapshot's set.
        let mut deleted = std::mem::take(&mut p.deleted);
        let (from, to) = if forward { (before, after) } else { (after, before) };
        from.deleted.difference(&to.deleted).for_each(|path| { deleted.remove(path); });
        deleted.extend(to.deleted.difference(&from.deleted).cloned());
        *p = to.as_ref().clone();
        p.pending = pending;
        p.deleted = deleted;
        p.structure_dirty = true;
    }
    if let Some(b) = t.behavior.as_ref().and_then(|id| p.behaviors.iter_mut().find(|b| &b.id == id)) {
        if forward {
            t.ops.iter().for_each(|op| run(&mut b.doc, op, true));
        } else {
            t.ops.iter().rev().for_each(|op| run(&mut b.doc, op, false));
        }
    }
    for (path, before, after) in &t.files {
        p.stage(path, if forward { after.clone() } else { before.clone() });
    }
    for l in &t.listings {
        // Fails only without a PML, which a step with listings always has.
        let _ = p.list(l, forward);
    }
}

pub fn undo(p: &mut Project, h: &mut History) -> Option<HistoryState> {
    let t = h.undo.pop()?;
    replay(p, &t, false);
    h.redo.push(t);
    h.revision += 1;
    Some(h.state(vec![]))
}

pub fn redo(p: &mut Project, h: &mut History) -> Option<HistoryState> {
    let t = h.redo.pop()?;
    replay(p, &t, true);
    h.undo.push(t);
    h.revision += 1;
    Some(h.state(vec![]))
}

/// Adds project-file changes `(path, before, after)` to the step just applied, so a library
/// insertion and its attachments undo together.
pub fn add_files_to_last(p: &mut Project, h: &mut History, files: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)>, listings: Vec<crate::project::Listing>) -> Result<(), String> {
    let Some(t) = h.undo.last_mut() else { return Ok(()) };
    for (path, _, after) in &files {
        p.stage(path, after.clone());
    }
    t.files.extend(files);
    t.listings.extend(listings.iter().cloned());
    for l in &listings {
        p.list(l, true)?;
    }
    Ok(())
}

impl Command {
    /// Rejects node ids that are out of range or not attached to the document (trust boundary).
    fn validate_nodes(self, d: &Doc) -> Result<(), String> {
        let ids: Vec<NodeId> = match &self {
            Command::SetAttributes { node, .. } => vec![*node],
            Command::MoveBoxes { moves } => moves.iter().map(|m| m.0).collect(),
            Command::SetBoxContent { r#box, .. } | Command::SetScript { r#box, .. } | Command::AddPort { r#box, .. } | Command::SetDialogFile { r#box, .. } => vec![*r#box],
            Command::SortPorts { r#box, ports } => [vec![*r#box], ports.clone()].concat(),
            Command::SetCurveValues { keys, .. } => keys.clone(),
            Command::MoveTangent { tangent, .. } => vec![*tangent],
            Command::PasteCurveKeys { timeline, .. } | Command::SetTimelineRange { timeline, .. } | Command::StretchTimelineRange { timeline, .. } => vec![*timeline],
            Command::SimplifyCurves { timeline, curves, .. } => [vec![*timeline], curves.clone()].concat(),
            Command::TranslateKeyframes { keyframes, .. } => keyframes.clone(),
            Command::PasteKeyframes { layer, .. } => vec![*layer],
            Command::NewBox { diagram, .. } | Command::AddBox { diagram, .. } | Command::AddLink { diagram, .. } | Command::Paste { diagram, .. } => vec![*diagram],
            Command::InsertXml { parent, .. } => vec![*parent],
            Command::RecordKeys { timeline, .. } => vec![*timeline],
            Command::Remove { nodes } => nodes.clone(),
            Command::ConvertToBox { diagram, boxes, .. } => [vec![*diagram], boxes.clone()].concat(),
            Command::StoreKeys { timeline, .. } | Command::RemoveKeys { timeline, .. } | Command::Mirror { timeline, .. } => vec![*timeline],
            Command::MoveKeys { keys, .. } | Command::SetInterpolation { keys, .. } => keys.clone(),
        };
        let root = d.root().unwrap();
        for n in ids {
            let mut cur = Some(n).filter(|&n| n < d.nodes.len() && matches!(d.nodes[n].kind, Kind::Element(_)));
            let mut attached = false;
            while let Some(c) = cur {
                if c == root {
                    attached = true;
                    break;
                }
                cur = d.nodes[c].parent;
            }
            if !attached {
                return Err(format!("Node {n} does not exist in this behavior"));
            }
        }
        Ok(())
    }
}
