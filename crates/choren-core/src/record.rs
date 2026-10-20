//! Motion recording and "store joints": joint values become timeline keys through ordinary edit
//! commands, so a recording is one undo step like any other edit.

use crate::edit::Command;
use crate::xml::{escape_attr, Doc, NodeId};

/// `ActuatorList@model` values of NAO timelines.
pub const NAO_MODELS: &[&str] = &["", "Nao", "nao"];

/// Choregraphe writes key values with at most 4 decimals ("-4.3091").
fn number(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Commands that store `values` (actuator, value in file units: degrees, or 0–1 for hands with
/// `unit="1"`) at `frame` of timeline `t`. Existing keys at that frame are replaced, missing
/// curves and the actuator list (`model` is one of `models`, else `""`) are created, and the
/// timeline grows to include the frame.
pub fn store_keys(d: &Doc, t: NodeId, frame: i64, values: &[(String, f64, bool)], models: &[&str]) -> Result<Vec<Command>, String> {
    if !d.is_el(t, "Timeline") {
        return Err("Keys are stored into a timeline".into());
    }
    if frame < 1 {
        return Err("Frames start at 1".into());
    }
    let curve_xml = |name: &str, v: f64, hand: bool| {
        format!(
            r#"<ActuatorCurve name="value" actuator="{}" recordable="1" mute="0" unit="{}"><Key frame="{frame}" value="{}" /></ActuatorCurve>"#,
            escape_attr(name),
            if hand { 1 } else { 0 },
            number(v)
        )
    };
    let mut cmds = vec![];
    let list = d.kids(t, "ActuatorList").find(|&l| models.iter().any(|m| d.attr_raw(l, "model") == Some(*m)));
    match list {
        None => {
            let curves: String = values.iter().map(|(n, v, h)| curve_xml(n, *v, *h)).collect();
            cmds.push(Command::InsertXml { parent: t, xml: format!(r#"<ActuatorList model="">{curves}</ActuatorList>"#) });
        }
        Some(list) => {
            for (name, v, hand) in values {
                let curve = d.kids(list, "ActuatorCurve").find(|&c| d.attr_raw(c, "actuator") == Some(name.as_str()));
                let Some(curve) = curve else {
                    cmds.push(Command::InsertXml { parent: list, xml: curve_xml(name, *v, *hand) });
                    continue;
                };
                match d.kids(curve, "Key").find(|&k| d.attr_num(k, "frame", -1.0) as i64 == frame) {
                    Some(k) => cmds.push(Command::SetAttributes { node: k, values: [("value".to_string(), Some(number(*v)))].into() }),
                    None => cmds.push(Command::InsertXml { parent: curve, xml: format!(r#"<Key frame="{frame}" value="{}" />"#, number(*v)) }),
                }
            }
        }
    }
    if (frame as f64) > d.attr_num(t, "size", 0.0) {
        cmds.push(Command::SetAttributes { node: t, values: [("size".to_string(), Some(frame.to_string()))].into() });
    }
    Ok(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, History, NewBoxKind};
    use crate::project::Project;
    use crate::view;

    #[test]
    fn stored_keys_create_then_replace() {
        let dir = std::env::temp_dir().join(format!("choren-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::create(&dir, "rec").unwrap();
        let bid = p.behaviors[0].id.clone();
        let mut h = History::fresh();
        let d = &p.behaviors[0].doc;
        let r = view::root_box(d);
        let t0 = d.kid(r, "Timeline").unwrap();
        let diagram = d.kid(d.kid(d.kid(t0, "BehaviorLayer").unwrap(), "BehaviorKeyframe").unwrap(), "Diagram").unwrap();
        let b = edit::apply(&mut p, &mut h, &bid, "new", vec![edit::Command::NewBox { diagram, kind: NewBoxKind::Timeline, name: "Rec".into(), x: 0.0, y: 0.0, dialog_file: None }]).unwrap().created[0];
        let t = p.behaviors[0].doc.kid(b, "Timeline").unwrap();
        let store = |p: &mut Project, h: &mut History, frame, yaw: f64| {
            let values = vec![("HeadYaw".into(), yaw, false), ("LHand".into(), 0.25, true)];
            edit::apply(p, h, &bid, "Store joints", vec![edit::Command::RecordKeys { timeline: t, frame, values }]).unwrap();
        };
        store(&mut p, &mut h, 10, 12.345678);
        store(&mut p, &mut h, 20, -5.0);
        store(&mut p, &mut h, 10, 30.0); // replaces the key at frame 10
        store(&mut p, &mut h, 350, 1.0); // past the end: the timeline grows
        let d = &p.behaviors[0].doc;
        let tv = view::box_view(d, b).timeline.unwrap();
        let yaw = tv.curves.iter().find(|c| c.actuator == "HeadYaw").unwrap();
        assert_eq!((yaw.frames.clone(), yaw.values.clone()), (vec![10.0, 20.0, 350.0], vec![30.0, -5.0, 1.0]));
        assert_eq!(tv.size, 350.0);
        assert_eq!(d.kids(t, "ActuatorList").count(), 1);
        let hand = d.kids(d.kid(t, "ActuatorList").unwrap(), "ActuatorCurve").find(|&c| d.attr_raw(c, "actuator") == Some("LHand")).unwrap();
        assert_eq!(d.attr_raw(hand, "unit"), Some("1"));
        assert_eq!(number(12.345678), "12.3457");
        assert!(store_keys(d, b, 1, &[], NAO_MODELS).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
