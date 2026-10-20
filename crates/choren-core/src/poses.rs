//! Pose library (`.xap`): folders of named whole-body joint snapshots (degrees [inf]).

use crate::xml::{escape_text, Doc, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pose {
    pub folder: String,
    pub name: String,
    pub description: String,
    pub bitmap: String,
    /// (joint, value) in file order.
    pub motors: Vec<(String, f64)>,
}

fn child_text(d: &Doc, n: NodeId, tag: &str) -> String {
    d.kid(n, tag).map(|c| d.text(c).trim().to_string()).unwrap_or_default()
}

pub fn parse(src: &str) -> Result<(Doc, Vec<Pose>), String> {
    let d = Doc::parse(src)?;
    let root = d.root().unwrap();
    if d.name(root) != "ChoregraphePositionLibrary" {
        return Err("Not a Choregraphe position library".into());
    }
    let poses = d
        .kids(root, "folder")
        .flat_map(|f| d.kids(f, "position").map(move |p| (f, p)).collect::<Vec<_>>())
        .map(|(f, p)| Pose {
            folder: child_text(&d, f, "title"),
            name: child_text(&d, p, "name"),
            description: child_text(&d, p, "description"),
            bitmap: child_text(&d, p, "bitmap"),
            motors: d
                .kid(p, "Motors")
                .map(|m| d.kids(m, "Motor").map(|x| (child_text(&d, x, "name"), child_text(&d, x, "value").parse().unwrap_or(0.0))).collect())
                .unwrap_or_default(),
        })
        .collect();
    Ok((d, poses))
}

/// An empty library document.
pub fn empty() -> Doc {
    Doc::parse("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<ChoregraphePositionLibrary xmlns=\"http://www.ald.softbankrobotics.com/schema/choregraphe/position.xsd\" />\n").unwrap()
}

fn find(d: &Doc, folder: &str, name: &str) -> Option<NodeId> {
    let root = d.root().unwrap();
    d.kids(root, "folder").filter(|&f| child_text(d, f, "title") == folder).flat_map(|f| d.kids(f, "position").collect::<Vec<_>>()).find(|&p| child_text(d, p, "name") == name)
}

/// Adds a pose (creating its folder), replacing a pose of the same name in that folder.
pub fn add(d: &mut Doc, pose: &Pose) -> Result<(), String> {
    if pose.name.trim().is_empty() || pose.folder.trim().is_empty() {
        return Err("A pose needs a folder and a name.".into());
    }
    if pose.motors.iter().any(|(name, value)| name.is_empty() || !value.is_finite()) { return Err("Pose joints need names and finite values.".into()); }
    remove(d, &pose.folder, &pose.name);
    let root = d.root().unwrap();
    let existing = d.kids(root, "folder").find(|&f| child_text(d, f, "title") == pose.folder);
    let folder = match existing {
        Some(f) => f,
        None => crate::edit::insert_raw(d, root, &format!("<folder>\n    <folded>no</folded>\n    <title>{}</title>\n</folder>", escape_text(&pose.folder)))?,
    };
    let motors: String = pose.motors.iter().map(|(n, v)| format!("\n        <Motor>\n            <name>{}</name>\n            <value>{v}</value>\n        </Motor>", escape_text(n))).collect();
    let xml = format!(
        "<position>\n    <name>{}</name>\n    <bitmap>{}</bitmap>\n    <description>{}</description>\n    <Motors>{motors}\n    </Motors>\n</position>",
        escape_text(&pose.name), escape_text(&pose.bitmap), escape_text(&pose.description)
    );
    crate::edit::insert_raw(d, folder, &xml)?;
    Ok(())
}

pub fn remove(d: &mut Doc, folder: &str, name: &str) -> bool {
    match find(d, folder, name) {
        Some(p) => {
            crate::edit::detach_raw(d, p);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_replace_remove_round_trip() {
        let mut d = empty();
        let pose = Pose { folder: "Mine".into(), name: "Wave & hi".into(), description: String::new(), bitmap: String::new(), motors: vec![("HeadYaw".into(), 12.5), ("LHand".into(), 0.3)] };
        add(&mut d, &pose).unwrap();
        add(&mut d, &Pose { motors: vec![("HeadYaw".into(), 1.0)], ..pose.clone() }).unwrap();
        let (_, poses) = parse(&d.write()).unwrap();
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].motors, vec![("HeadYaw".to_string(), 1.0)]);
        let before = d.write();
        assert!(add(&mut d, &Pose {motors: vec![("HeadYaw".into(), f64::NAN)], ..pose.clone()}).is_err());
        assert_eq!(d.write(), before);
        assert!(remove(&mut d, "Mine", "Wave & hi"));
        assert!(parse(&d.write()).unwrap().1.is_empty());
    }
}
