//! Robot descriptors. NAO 6 is the only model for now; another robot is another JSON file
//! (`robots/<id>.json`) with the same shape. Values come from public NAO documentation; postures
//! are Choren's own.

pub mod ogre;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Joint {
    pub name: String,
    pub parent: Option<String>,
    /// Offset from the parent joint (metres), in the parent's frame.
    pub origin: [f64; 3],
    /// Rotation axis; zero for the hands, whose value is an opening ratio.
    pub axis: [f64; 3],
    pub min: f64,
    pub max: f64,
    pub max_velocity: f64,
    /// Fourth `ALMotion.getLimits` value (maximum torque, as NAOqi 2.8 reports it).
    #[serde(default)]
    pub max_torque: f64,
    pub chain: String,
    /// Mechanically coupled joints follow this joint and ignore their own commands.
    #[serde(default)]
    pub coupled_to: Option<String>,
}

/// A joint whose range depends on another joint's angle (NAO's head and ankle anti-collision
/// limits): `points` are `[angle of by, min, max]`, linearly interpolated between points.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoupledLimit {
    pub joint: String,
    pub by: String,
    pub points: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Posture {
    pub name: String,
    pub family: String,
    /// Torso tilt about Y (radians) while in this posture, for grounding the body.
    pub torso_pitch: f64,
    /// Values in `body_names` order.
    pub angles: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Touch {
    pub name: String,
    pub event: String,
    pub key: String,
    pub link: Option<String>,
}

/// ALLeds names as NAOqi 2.8 lists them (`listGroups`/`listGroup`, `listLEDs`/`listLED`): each
/// group or LED name → its DCM actuators (`Face/Led/Red/Left/0Deg/Actuator/Value`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leds {
    pub groups: BTreeMap<String, Vec<String>>,
    pub leds: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Walk {
    pub max_x: f64,
    pub max_y: f64,
    pub max_theta: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Robot {
    pub id: String,
    pub name: String,
    pub family: String,
    pub version: String,
    /// `ActuatorList@model` values that belong to this robot.
    pub actuator_list_models: Vec<String>,
    pub robot_config: BTreeMap<String, String>,
    /// Motion cycle: ALMotion computes one command per joint every period (24 ms on Choregraphe's
    /// virtual robot, `ALMotion.getMotionCycleTime`).
    pub control_period_ms: u64,
    /// Ranges that depend on another joint (anti-collision limits).
    #[serde(default)]
    pub coupled_limits: Vec<CoupledLimit>,
    pub body_names: Vec<String>,
    pub joints: Vec<Joint>,
    pub chains: BTreeMap<String, Vec<String>>,
    /// Chain names in NAOqi's order (`getBodyNames("Chains")`).
    #[serde(default)]
    pub chain_order: Vec<String>,
    /// Other `getBodyNames` groups ("Arms", "Joints", "Actuators", …) → joints.
    #[serde(default)]
    pub body_groups: BTreeMap<String, Vec<String>>,
    /// Joint angles Choregraphe's virtual robot reports right after it starts.
    #[serde(default)]
    pub initial_angles: Vec<f64>,
    pub mirror: BTreeMap<String, String>,
    pub tips: BTreeMap<String, (String, [f64; 3])>,
    pub postures: Vec<Posture>,
    pub posture_graph: Vec<(String, String)>,
    pub leds: Leds,
    pub touch: Vec<Touch>,
    pub walk: Walk,
    /// Link (joint name, or "Torso") → install mesh name in `share/alrobotmodel/meshes`. The
    /// meshes are authored in their link's frame, so they need no offset.
    #[serde(default)]
    pub meshes: BTreeMap<String, String>,
    /// ALResourceManager tree: group → members. Names that are not groups are leaves.
    #[serde(default)]
    pub resources: BTreeMap<String, Vec<String>>,
    /// Posture graph file in the install's `share/naoqi/robot_posture`.
    #[serde(default)]
    pub install_postures: Option<String>,
}

pub const MODELS: &[&str] = &["nao6"];

impl Robot {
    pub fn load(id: &str) -> Result<Robot, String> {
        let json = match id {
            "nao6" => include_str!("../robots/nao6.json"),
            _ => return Err(format!("Unknown robot model {id}. Available: {}", MODELS.join(", "))),
        };
        serde_json::from_str(json).map_err(|e| format!("Invalid robot descriptor {id}: {e}"))
    }

    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }

    /// Expands NAOqi joint names: a joint, a chain ("LArm"), "Body"/"JointActuators", or "Joints".
    pub fn expand(&self, names: &[String]) -> Result<Vec<usize>, String> {
        let mut out = vec![];
        for n in names {
            match n.as_str() {
                "Body" => out.extend(0..self.joints.len()),
                _ => match self.chains.get(n).or_else(|| self.body_groups.get(n)) {
                    Some(c) => out.extend(c.iter().filter_map(|j| self.joint_index(j))),
                    None => out.push(self.joint_index(n).ok_or_else(|| format!("Could not parse \"{n}\" as a valid joint name, chain name or \"Body\""))?),
                },
            }
        }
        Ok(out)
    }

    /// The actuators an ALLeds name addresses: a group, an LED, or an actuator (with or without
    /// its `/Actuator/Value` suffix).
    pub fn led_actuators(&self, name: &str) -> Option<Vec<String>> {
        if let Some(a) = self.leds.groups.get(name).or_else(|| self.leds.leds.get(name)) {
            return Some(a.clone());
        }
        let full = if name.ends_with("/Actuator/Value") { name.to_string() } else { format!("{name}/Actuator/Value") };
        self.led_actuator_names().contains(&full).then(|| vec![full])
    }

    pub fn led_actuator_names(&self) -> std::collections::BTreeSet<String> {
        self.leds.groups.values().chain(self.leds.leds.values()).flatten().cloned().collect()
    }

    /// The leaf resources under `name` (itself when it is a leaf). Two resources conflict when
    /// their leaves overlap, so "All motors" conflicts with "LShoulderPitch".
    pub fn resource_leaves(&self, name: &str) -> std::collections::BTreeSet<String> {
        match self.resources.get(name) {
            Some(kids) => kids.iter().flat_map(|k| self.resource_leaves(k)).collect(),
            None => [name.to_string()].into(),
        }
    }

    pub fn clamp(&self, i: usize, v: f64) -> f64 {
        v.clamp(self.joints[i].min, self.joints[i].max)
    }

    /// The range of `limit.joint` when `limit.by` is at `x`.
    pub fn coupled_range(limit: &CoupledLimit, x: f64) -> (f64, f64) {
        let p = &limit.points;
        let Some(i) = p.iter().position(|q| q[0] >= x) else { return p.last().map_or((f64::MIN, f64::MAX), |q| (q[1], q[2])) };
        if i == 0 {
            return (p[0][1], p[0][2]);
        }
        let (a, b) = (p[i - 1], p[i]);
        let u = (x - a[0]) / (b[0] - a[0]);
        (a[1] + u * (b[1] - a[1]), a[2] + u * (b[2] - a[2]))
    }

    pub fn posture(&self, name: &str) -> Option<&Posture> {
        self.postures.iter().find(|p| p.name == name)
    }

    /// The named posture closest to `angles` and the torso pitch (squared differences, hands
    /// excluded). Lying on the back and on the belly differ only by the torso.
    pub fn classify(&self, angles: &[f64], torso_pitch: f64) -> &Posture {
        let d = |p: &Posture| {
            let joints: f64 = p.angles.iter().zip(angles).enumerate().filter(|(i, _)| self.joints[*i].axis != [0.0; 3]).map(|(_, (a, b))| (a - b).powi(2)).sum();
            joints + 4.0 * (p.torso_pitch - torso_pitch).powi(2)
        };
        self.postures.iter().min_by(|a, b| d(a).total_cmp(&d(b))).unwrap()
    }

    /// Shortest posture sequence from `from` to `to` (breadth-first: transitions cost the same).
    pub fn posture_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in &self.posture_graph {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let mut prev: HashMap<&str, &str> = HashMap::new();
        let mut queue = VecDeque::from([from]);
        while let Some(n) = queue.pop_front() {
            if n == to {
                let mut path = vec![to.to_string()];
                let mut cur = to;
                while let Some(&p) = prev.get(cur) {
                    path.push(p.to_string());
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            for &m in adj.get(n).into_iter().flatten() {
                if m != from && !prev.contains_key(m) {
                    prev.insert(m, n);
                    queue.push_back(m);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nao6_is_consistent() {
        let r = Robot::load("nao6").unwrap();
        assert_eq!(r.joints.len(), 26);
        assert_eq!(r.joints.iter().map(|j| j.name.clone()).collect::<Vec<_>>(), r.body_names);
        for p in &r.postures {
            assert_eq!(p.angles.len(), 26, "{}", p.name);
            // Limits are NAOqi's 32-bit floats; the install's postures touch them.
            for (i, v) in p.angles.iter().enumerate() {
                assert!(*v >= r.joints[i].min - 1e-6 && *v <= r.joints[i].max + 1e-6, "{} {} = {v}", p.name, r.joints[i].name);
            }
            assert_eq!(r.classify(&p.angles, p.torso_pitch).name, p.name);
        }
        for j in &r.joints {
            assert!(j.parent.as_ref().is_none_or(|p| r.joint_index(p).is_some()));
        }
        assert_eq!(r.expand(&["LArm".into()]).unwrap().len(), 6);
        assert_eq!(r.expand(&["Body".into()]).unwrap().len(), 26);
        assert!(r.expand(&["Nose".into()]).is_err());
        assert_eq!(r.posture_path("Stand", "LyingBack").unwrap(), ["Stand", "StandInit", "Crouch", "Sit", "LyingBack"]);
        assert_eq!(r.posture_path("Sit", "Sit").unwrap(), ["Sit"]);
        assert_eq!(r.resource_leaves("All motors").len(), 26);
        assert!(r.resource_leaves("Arms").contains("LHand"));
        assert_eq!(r.resource_leaves("Standing").len(), 1);
        assert!(r.meshes.keys().all(|l| l == "Torso" || r.joint_index(l).is_some()));
    }
}
