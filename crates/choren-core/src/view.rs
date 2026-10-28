//! Serializable read views for the UI. Field names match `app/src/format/types.ts`; every editable
//! entity also carries `node`, the session NodeId that edit commands address.

use crate::project::Project;
use crate::xml::{Doc, NodeId};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    pub node: NodeId,
    pub id: i64,
    pub name: String,
    pub r#type: i64,
    pub type_size: i64,
    pub nature: i64,
    pub inner: bool,
    pub tooltip: String,
    /// ALMemory key of an event input (`stm_value_name`, nature 4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stm: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub node: NodeId,
    pub id: i64,
    pub name: String,
    pub content_type: i64,
    pub value: String,
    pub default_value: String,
    pub inherits_from_parent: bool,
    pub tooltip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    pub custom_choice: bool,
    pub choices: Vec<Choice>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Choice {
    pub node: NodeId,
    pub value: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Resource {
    pub node: NodeId,
    pub name: String,
    pub r#type: String,
    pub timeout: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub node: NodeId,
    pub out_owner: i64,
    pub out_port: i64,
    pub in_owner: i64,
    pub in_port: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Diagram {
    pub node: NodeId,
    pub scale: f64,
    pub boxes: Vec<BoxView>,
    pub links: Vec<Link>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BehaviorKeyframe {
    pub node: NodeId,
    pub name: String,
    pub index: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram: Option<Diagram>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BehaviorLayer {
    pub mute: bool,
    pub node: NodeId,
    pub name: String,
    pub keyframes: Vec<BehaviorKeyframe>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ActuatorCurve {
    pub node: NodeId,
    pub actuator: String,
    pub frames: Vec<f64>,
    pub values: Vec<f64>,
    /// 0 degree, 1 dimensionless (hands), -1 legacy.
    pub unit: i64,
    pub mute: bool,
    pub keys: Vec<CurveKey>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Tangent {
    pub node: NodeId,
    pub interp: String,
    pub abscissa: f64,
    pub ordinate: f64,
}

/// A curve key; a key without tangents uses Choregraphe's automatic Bezier interpolation.
#[derive(Serialize, Clone, Debug)]
pub struct CurveKey {
    pub node: NodeId,
    pub frame: f64,
    pub value: f64,
    pub smooth: bool,
    pub symmetrical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Tangent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Tangent>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub node: NodeId,
    pub enabled: bool,
    pub fps: f64,
    pub size: f64,
    /// Play range (`end_frame` -1 = up to `size`).
    pub resources_acquisition: i64,
    pub start_frame: i64,
    pub end_frame: i64,
    pub layers: Vec<BehaviorLayer>,
    pub curves: Vec<ActuatorCurve>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BoxView {
    pub node: NodeId,
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub x: f64,
    pub y: f64,
    pub tooltip: String,
    pub bitmaps: Vec<String>,
    pub script: String,
    pub plugin: String,
    #[serde(rename = "pluginText", skip_serializing_if = "Option::is_none")]
    pub plugin_text: Option<String>,
    #[serde(rename = "dialogFile", skip_serializing_if = "Option::is_none")]
    pub dialog_file: Option<String>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub params: Vec<Param>,
    pub resources: Vec<Resource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Behavior {
    pub id: String,
    pub name: String,
    pub source: String,
    pub root: BoxView,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub path: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<bool>,
}

impl ProjectFile {
    pub fn new(path: String, kind: &'static str) -> Self {
        ProjectFile { path, kind, behavior_id: None, missing: None }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Diagnostic {
    pub id: String,
    pub severity: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Diagnostic {
    pub fn warning(path: &str, message: &str) -> Self {
        Diagnostic { id: path.into(), severity: "warning", message: message.into(), path: Some(path.into()) }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ProjectDocument {
    pub id: String,
    pub source: String,
    pub name: String,
    pub format: crate::project::Format,
    pub behaviors: Vec<Behavior>,
    pub files: Vec<ProjectFile>,
    pub metadata: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Choren's own project data (`<name>.chorenproj`), e.g. `treeOrder`.
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub choren: serde_json::Map<String, serde_json::Value>,
}

pub fn project(p: &Project) -> ProjectDocument {
    ProjectDocument {
        id: p.source.clone(),
        source: p.source.clone(),
        name: p.name.clone(),
        format: p.format,
        behaviors: p.behaviors.iter().map(|b| behavior(b)).collect(),
        files: p.files.clone(),
        metadata: p.metadata.clone(),
        diagnostics: p.diagnostics.clone(),
        choren: p.choren.clone(),
    }
}

pub fn behavior(b: &crate::project::Behavior) -> Behavior {
    Behavior { id: b.id.clone(), name: b.name.clone(), source: b.source.clone(), root: box_view(&b.doc, root_box(&b.doc)) }
}

pub fn root_box(d: &Doc) -> NodeId {
    d.kid(d.root().unwrap(), "Box").unwrap()
}

fn int(d: &Doc, n: NodeId, k: &str) -> i64 {
    d.attr_num(n, k, 0.0) as i64
}

fn port(d: &Doc, n: NodeId) -> Port {
    Port { node: n, id: int(d, n, "id"), name: d.attr_or(n, "name"), r#type: int(d, n, "type"), nature: int(d, n, "nature"), type_size: d.attr_num(n, "type_size", 1.0) as i64, inner: d.attr_raw(n, "inner") == Some("1"), tooltip: d.attr_or(n, "tooltip"), stm: d.attr(n, "stm_value_name") }
}

pub fn diagram(d: &Doc, n: NodeId) -> Diagram {
    Diagram {
        node: n,
        scale: d.attr_num(n, "scale", 100.0),
        boxes: d.kids(n, "Box").map(|b| box_view(d, b)).collect(),
        links: d
            .kids(n, "Link")
            .map(|l| Link { node: l, out_owner: int(d, l, "outputowner"), out_port: int(d, l, "indexofoutput"), in_owner: int(d, l, "inputowner"), in_port: int(d, l, "indexofinput") })
            .collect(),
    }
}

fn timeline(d: &Doc, t: NodeId) -> Timeline {
    Timeline {
        node: t,
        enabled: d.attr_raw(t, "enable") == Some("1"),
        fps: d.attr_num(t, "fps", 25.0),
        size: d.attr_num(t, "size", 0.0),
        resources_acquisition: int(d, t, "resources_acquisition"),
        start_frame: d.attr_num(t, "start_frame", 1.0) as i64,
        end_frame: d.attr_num(t, "end_frame", -1.0) as i64,
        layers: d
            .kids(t, "BehaviorLayer")
            .map(|l| BehaviorLayer {
                node: l,
                name: d.attr_or(l, "name"),
                mute: d.attr_raw(l, "mute") == Some("1"),
                keyframes: d
                    .kids(l, "BehaviorKeyframe")
                    .map(|k| BehaviorKeyframe { node: k, name: d.attr_or(k, "name"), index: d.attr_num(k, "index", 1.0) as i64, diagram: d.kid(k, "Diagram").map(|x| diagram(d, x)) })
                    .collect(),
            })
            .collect(),
        curves: d
            .kids(t, "ActuatorList")
            .flat_map(|a| d.kids(a, "ActuatorCurve").collect::<Vec<_>>())
            .map(|c| {
                let keys: Vec<_> = d.kids(c, "Key").collect();
                let tangent = |k: NodeId, side: &str| {
                    d.kids(k, "Tangent").find(|&t| d.attr_raw(t, "side") == Some(side)).map(|t| Tangent { node: t, interp: d.attr_or(t, "interpType"), abscissa: d.attr_num(t, "abscissaParam", 0.0), ordinate: d.attr_num(t, "ordinateParam", 0.0) })
                };
                ActuatorCurve {
                    node: c,
                    actuator: d.attr_or(c, "actuator"),
                    frames: keys.iter().map(|&k| d.attr_num(k, "frame", 0.0)).collect(),
                    values: keys.iter().map(|&k| d.attr_num(k, "value", 0.0)).collect(),
                    unit: int(d, c, "unit"),
                    mute: d.attr_raw(c, "mute") == Some("1"),
                    keys: keys
                        .iter()
                        .map(|&k| CurveKey {
                            node: k,
                            frame: d.attr_num(k, "frame", 0.0),
                            value: d.attr_num(k, "value", 0.0),
                            smooth: d.attr_raw(k, "smooth") == Some("1"),
                            symmetrical: d.attr_raw(k, "symmetrical") == Some("1"),
                            left: tangent(k, "left"),
                            right: tangent(k, "right"),
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

/// Ignore XML formatting whitespace around CDATA script and plugin content.
fn content_text(d: &Doc, content: NodeId) -> String {
    let cdata: Vec<&str> = d.el(content).children.iter().filter_map(|&c| match &d.nodes[c].kind {
        crate::xml::Kind::CData(t) => Some(t.as_str()),
        _ => None,
    }).collect();
    if cdata.is_empty() { d.text(content) } else { cdata.concat() }
}

pub fn box_view(d: &Doc, b: NodeId) -> BoxView {
    BoxView {
        node: b,
        id: int(d, b, "id"),
        name: d.attr_or(b, "name"),
        category: d.attr(b, "choren_category"),
        x: d.attr_num(b, "x", 0.0),
        y: d.attr_num(b, "y", 0.0),
        tooltip: d.attr_or(b, "tooltip"),
        bitmaps: d.kids(b, "bitmap").map(|x| d.text(x)).collect(),
        script: d.kid(b, "script").and_then(|s| d.kid(s, "content")).map(|c| content_text(d, c)).unwrap_or_default(),
        plugin: d.attr_or(b, "plugin"),
        plugin_text: (d.attr_raw(b, "plugin") == Some("textedit_plugin")).then(||
            d.kid(b, "pluginContent").and_then(|p| d.kid(p, "text")).map(|t| content_text(d, t)).unwrap_or_default()
        ),
        dialog_file: d.kid(b, "dialogFile").map(|f| d.text(f)),
        inputs: d.kids(b, "Input").map(|p| port(d, p)).collect(),
        outputs: d.kids(b, "Output").map(|p| port(d, p)).collect(),
        params: d
            .kids(b, "Parameter")
            .map(|p| Param {
                node: p,
                id: int(d, p, "id"),
                name: d.attr_or(p, "name"),
                content_type: int(d, p, "content_type"),
                value: d.attr_or(p, "value"),
                default_value: d.attr_or(p, "default_value"),
                inherits_from_parent: d.attr_raw(p, "inherits_from_parent") == Some("1"),
                tooltip: d.attr_or(p, "tooltip"),
                min: d.attr(p, "min"),
                max: d.attr(p, "max"),
                custom_choice: d.attr_raw(p, "custom_choice") == Some("1"),
                choices: d.kids(p, "Choice").map(|c| Choice { node: c, value: d.attr_or(c, "value") }).collect(),
            })
            .collect(),
        resources: d.kids(b, "Resource").map(|r| Resource { node: r, name: d.attr_or(r, "name"), r#type: d.attr_or(r, "type"), timeout: d.attr_or(r, "timeout") }).collect(),
        timeline: d.kid(b, "Timeline").map(|t| timeline(d, t)),
    }
}
