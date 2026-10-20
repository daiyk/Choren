//! Validation rules for names, ports and links, with the error message for each broken rule.

use crate::xml::{Doc, NodeId};

pub fn box_name(name: &str) -> Result<(), String> {
    if !name.chars().any(char::is_alphanumeric) || name.contains('"') {
        return Err("Please enter a box name, with at least one alphanumerical character, and no \"".into());
    }
    Ok(())
}

/// Input, output and parameter names: `[a-zA-Z0-9_]+`, unique across the box's three port lists.
/// ALMemory event inputs (`memory`) are named after their key, so `/` and `-` are allowed too.
pub fn io_name(d: &Doc, owner: NodeId, port: Option<NodeId>, name: &str, memory: bool) -> Result<(), String> {
    if name.is_empty() {
        return Err("You left the name field empty.".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (memory && (c == '/' || c == '-'))) {
        return Err("Only letters, numbers and underscore characters are allowed.".into());
    }
    let taken = ports(d, owner).any(|p| Some(p) != port && d.attr(p, "name").as_deref() == Some(name));
    if taken {
        return Err("This name is already assigned to an existing input, output or parameter.".into());
    }
    Ok(())
}

/// Layer and keyframe names: at least one letter, unique among siblings.
pub fn sibling_name(d: &Doc, node: NodeId, name: &str) -> Result<(), String> {
    if !name.chars().any(char::is_alphabetic) {
        return Err("Name invalid: please enter a non empty text, with at least one alphabetical character.".into());
    }
    if let Some(p) = d.nodes[node].parent {
        let tag = d.name(node).to_string();
        if d.kids(p, &tag).any(|s| s != node && d.attr(s, "name").as_deref() == Some(name)) {
            return Err("Name not available.".into());
        }
    }
    Ok(())
}

pub fn keyframe_index(d: &Doc, node: NodeId, index: &str) -> Result<(), String> {
    let i: i64 = index.parse().map_err(|_| "Index must be a whole number.")?;
    if i < 1 {
        return Err("Index must be at least 1.".into());
    }
    if let Some(p) = d.nodes[node].parent {
        if d.kids(p, "BehaviorKeyframe").any(|s| s != node && d.attr_raw(s, "index") == Some(index)) {
            return Err("Index not available.".into());
        }
    }
    Ok(())
}

/// Timeline start/end frames: `end` -1 plays up to the timeline size.
pub fn timeline_range(start: i64, end: i64) -> Result<(), String> {
    if start < 0 || (end != -1 && end <= start) {
        return Err("The end frame must come after the start frame.".into());
    }
    Ok(())
}

/// min ≤ max and min ≤ default ≤ max for numeric parameters.
pub fn param_range(min: Option<f64>, max: Option<f64>, default: Option<f64>) -> Result<(), String> {
    if let (Some(a), Some(b)) = (min, max) {
        if a > b {
            return Err("Maximum value should be greater than minimum value.".into());
        }
    }
    if let Some(v) = default {
        if min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m) {
            return Err("Default value should be between minimum and maximum value.".into());
        }
    }
    Ok(())
}

pub fn ports(d: &Doc, owner: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    d.el(owner).children.iter().copied().filter(move |&c| d.is_el(c, "Input") || d.is_el(c, "Output") || d.is_el(c, "Parameter"))
}

/// The box whose diagram this is (Box > Timeline > BehaviorLayer > BehaviorKeyframe > Diagram).
pub fn diagram_owner(d: &Doc, diagram: NodeId) -> Option<NodeId> {
    let mut n = diagram;
    for _ in 0..4 {
        n = d.nodes[n].parent?;
    }
    d.is_el(n, "Box").then_some(n)
}

/// IO type a parameter port gets from its content type (bool → dynamic, int/double → number, else string).
pub fn port_type(d: &Doc, port: NodeId) -> i64 {
    let ct = d.attr_num(port, "content_type", 0.0) as usize;
    if d.is_el(port, "Parameter") { [0, 2, 2, 3, 3, 3].get(ct).copied().unwrap_or(0) } else { d.attr_num(port, "type", 0.0) as i64 }
}

/// `type_size` of a port (an IOType is a type and a size); parameters have none, so 1.
pub fn port_size(d: &Doc, port: NodeId) -> i64 {
    d.attr_num(port, "type_size", 1.0) as i64
}

/// Whether a link can carry one port type to another, on (type, type_size) pairs:
/// anything converts to Dynamic or Bang, Dynamic converts to anything, otherwise sizes must
/// match and the types must be equal or Number → String.
pub fn conversion_possible(from: (i64, i64), to: (i64, i64)) -> bool {
    if to.0 <= 1 || from.0 == 0 {
        return true;
    }
    from.1 == to.1 && (from.0 == to.0 || (from.0 == 2 && to.0 == 3))
}

/// Resolves `owner`/`id` inside a diagram (owner 0 = the diagram's owner box) to a port element.
pub fn find_port(d: &Doc, diagram: NodeId, owner: i64, id: i64) -> Option<(NodeId, bool)> {
    let b = if owner == 0 { diagram_owner(d, diagram)? } else { d.kids(diagram, "Box").find(|&b| d.attr_num(b, "id", 0.0) as i64 == owner)? };
    ports(d, b).find(|&p| d.attr_num(p, "id", -1.0) as i64 == id).map(|p| (p, owner == 0))
}

/// Link validity: same level output → input; across levels input → input / output → output; no duplicates.
pub fn link(d: &Doc, diagram: NodeId, out: (i64, i64), inp: (i64, i64)) -> Result<(), String> {
    let (from, from_parent) = find_port(d, diagram, out.0, out.1).ok_or("Link source port not found.")?;
    let (to, to_parent) = find_port(d, diagram, inp.0, inp.1).ok_or("Link target port not found.")?;
    // A rail port acts reversed: a parent input feeds the diagram, a parent output receives from it.
    let from_ok = if from_parent { d.is_el(from, "Input") } else { d.is_el(from, "Output") };
    let to_ok = if to_parent { d.is_el(to, "Output") } else { !d.is_el(to, "Output") };
    if !from_ok || !to_ok {
        return Err(if from_parent == to_parent {
            "The I/Os are on the same level and therefore should be one input and one output."
        } else {
            "The I/Os are coming from differents levels and therefore should be both Inputs or Outputs."
        }
        .into());
    }
    let dup = d.kids(diagram, "Link").any(|l| {
        let g = |k| d.attr_num(l, k, -1.0) as i64;
        (g("outputowner"), g("indexofoutput"), g("inputowner"), g("indexofinput")) == (out.0, out.1, inp.0, inp.1)
    });
    if dup {
        return Err("A Link between those 2 IOs already exists.".into());
    }
    let (a, b) = ((port_type(d, from), port_size(d, from)), (port_type(d, to), port_size(d, to)));
    if !conversion_possible(a, b) {
        // size shown as "[n]" unless 1
        const T: [&str; 4] = ["Dynamic", "Bang", "Number", "String"];
        let n = |(t, s): (i64, i64)| format!("{}{}", T.get(t as usize).copied().unwrap_or("type unknown"), if s == 1 { String::new() } else { format!("[{s}]") });
        return Err(format!("Cannot convert from {} to {}", n(a), n(b)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_table_matches_choregraphe() {
        // rows: from Dynamic, Bang, Number, String; columns: to the same order (size 1)
        let expected = [[true, true, true, true], [true, true, false, false], [true, true, true, true], [true, true, false, true]];
        for (from, row) in expected.iter().enumerate() {
            for (to, &ok) in row.iter().enumerate() {
                assert_eq!(conversion_possible((from as i64, 1), (to as i64, 1)), ok, "from {from} to {to}");
            }
        }
        // sizes only matter for Number/String targets of a typed source
        assert!(!conversion_possible((2, 2), (2, 1)) && !conversion_possible((2, 1), (3, 3)));
        assert!(conversion_possible((2, 2), (0, 1)) && conversion_possible((3, 2), (1, 5)) && conversion_possible((0, 2), (3, 1)));
        assert!(conversion_possible((2, 3), (3, 3)));
    }
}
