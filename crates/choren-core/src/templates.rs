//! New-box and new-port templates: every kind starts from the default box (onLoad, onStart,
//! onStop, onStopped; localization 8), a script box gets the default script, a diagram box a disabled
//! timeline with one behavior layer, a timeline box an enabled timeline with no layer.

use crate::edit::{NewBoxKind, PortKind};
use crate::xml::escape_attr;
use std::collections::BTreeMap;

// The default Python script for the default ports, without trailing whitespace.
const PYTHON: &str = "class MyClass(GeneratedClass):
    def __init__(self):
        GeneratedClass.__init__(self)

    def onLoad(self):
        #put initialization code here
        pass

    def onUnload(self):
        #put clean-up code here
        pass

    def onInput_onStart(self):
        #self.onStopped() #activate the output of the box
        pass

    def onInput_onStop(self):
        self.onUnload() #it is recommended to reuse the clean-up as the box is stopped
        self.onStopped() #activate the output of the box";

const PORTS: &str = r#"
    <Input name="onLoad" type="1" type_size="1" nature="0" inner="1" tooltip="Signal sent when diagram is loaded." id="1" />
    <Input name="onStart" type="1" type_size="1" nature="2" inner="0" tooltip="Box behavior starts when a signal is received on this input." id="2" />
    <Input name="onStop" type="1" type_size="1" nature="3" inner="0" tooltip="Box behavior stops when a signal is received on this input." id="3" />
    <Output name="onStopped" type="1" type_size="1" nature="1" inner="0" tooltip="Signal sent when box behavior is finished." id="4" />"#;

pub fn new_box(kind: NewBoxKind, name: &str) -> String {
    let name = escape_attr(name);
    let (bitmap, script, timeline) = match kind {
        NewBoxKind::Python => ("box-python-script.png", PYTHON, ""),
        // the default box plus the `dialogFile` (set by the command)
        NewBoxKind::Dialog => ("box-dialog.png", "", ""),
        NewBoxKind::Diagram => ("box-diagram.png", "", "\n    <Timeline enable=\"0\">\n        <BehaviorLayer name=\"behavior_layer1\">\n            <BehaviorKeyframe name=\"keyframe1\" index=\"1\">\n                <Diagram />\n            </BehaviorKeyframe>\n        </BehaviorLayer>\n    </Timeline>"),
        // no layer; size is max(last key frame, end_frame) = -1.
        NewBoxKind::Timeline => ("box-timeLine.png", "", "\n    <Timeline enable=\"1\" fps=\"25\" start_frame=\"1\" end_frame=\"-1\" size=\"-1\" />"),
    };
    format!(
        "<Box name=\"{name}\" id=\"0\" localization=\"8\" tooltip=\"\" x=\"0\" y=\"0\">\n    <bitmap>media/images/box/{bitmap}</bitmap>\n    <script language=\"4\">\n        <content>\n            <![CDATA[{script}]]>\n</content>\n    </script>{PORTS}{timeline}\n</Box>"
    )
}

/// Attribute order follows the corpus: Input/Output `name type type_size nature inner tooltip id`,
/// Parameter `name inherits_from_parent content_type value default_value [min max] tooltip id`.
pub fn port(kind: PortKind, name: &str, id: i64, v: &BTreeMap<String, String>) -> String {
    let get = |k: &str, d: &str| escape_attr(v.get(k).map(String::as_str).unwrap_or(d));
    let name = escape_attr(name);
    match kind {
        PortKind::Parameter => {
            let range = ["min", "max"].iter().filter(|k| v.contains_key(**k)).map(|k| format!(" {k}=\"{}\"", get(k, ""))).collect::<String>();
            format!(
                "<Parameter name=\"{name}\" inherits_from_parent=\"{}\" content_type=\"{}\" value=\"{}\" default_value=\"{}\"{range} tooltip=\"{}\" id=\"{id}\" />",
                get("inherits_from_parent", "0"), get("content_type", "3"), get("value", ""), get("default_value", ""), get("tooltip", "")
            )
        }
        _ => {
            let (tag, nature) = if kind == PortKind::Input { ("Input", "1") } else { ("Output", "2") };
            let stm = v.get("stm_value_name").map(|s| format!(" stm_value_name=\"{}\"", escape_attr(s))).unwrap_or_default();
            format!(
                "<{tag} name=\"{name}\" type=\"{}\" type_size=\"{}\" nature=\"{}\"{stm} inner=\"{}\" tooltip=\"{}\" id=\"{id}\" />",
                get("type", "1"), get("type_size", "1"), get("nature", nature), get("inner", "0"), get("tooltip", "")
            )
        }
    }
}

/// A new `behavior.xar` whose root box is a flow diagram or a timeline ("Root box type").
pub fn behavior(kind: NewBoxKind) -> String {
    let kind = if matches!(kind, NewBoxKind::Timeline) { kind } else { NewBoxKind::Diagram };
    let root = new_box(kind, "root")
        .replacen("id=\"0\"", "id=\"-1\"", 1)
        .replacen("tooltip=\"\"", "tooltip=\"Root box of Choregraphe&apos;s behavior. Highest level possible.\"", 1)
        .replace("media/images/box/box-diagram.png", "media/images/box/root.png")
        .replace("media/images/box/box-timeLine.png", "media/images/box/root.png");
    // The root template has an empty script, so indenting every line cannot alter code.
    let root = root.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n");
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<ChoregrapheProject xmlns=\"http://www.ald.softbankrobotics.com/schema/choregraphe/project.xsd\" xar_version=\"3\">\n{root}\n</ChoregrapheProject>\n")
}
