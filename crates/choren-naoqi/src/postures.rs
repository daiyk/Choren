//! The install's full posture graph (`share/naoqi/robot_posture/postures_naoh25.xml`, a Boost
//! serialization archive), imported at runtime. ALRobotPosture walks it when it is loaded;
//! otherwise it uses the descriptor's 8 named postures.

use choren_core::xml::{Doc, NodeId};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

#[derive(Clone, Debug)]
pub struct Node {
    pub id: i64,
    /// Empty for the unnamed intermediate postures.
    pub name: String,
    pub family: String,
    /// Body joint angles (radians), in the descriptor's `body_names` order.
    pub angles: Vec<f64>,
    /// Torso roll and pitch (radians).
    pub inertial: [f64; 2],
    /// (neighbour id, cost, speed coefficient)
    pub neighbours: Vec<(i64, f64, f64)>,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
}

fn items(d: &Doc, n: Option<NodeId>) -> Vec<NodeId> {
    n.map(|n| d.kids(n, "item").collect()).unwrap_or_default()
}

fn num(d: &Doc, n: NodeId, k: &str) -> f64 {
    d.kid(n, k).and_then(|x| d.text(x).trim().parse().ok()).unwrap_or(0.0)
}

pub fn parse(xml: &str, joints: usize) -> Result<Graph, String> {
    // The DOCTYPE line is not needed and the XML tree does not keep it.
    let xml = xml.replace("<!DOCTYPE boost_serialization>", "");
    let d = Doc::parse(&xml)?;
    let root = d.root().ok_or("empty posture file")?;
    let postures = d.kid(root, "postures").ok_or("no <postures> in the posture file")?;
    let mut nodes = vec![];
    for item in items(&d, Some(postures)) {
        let p = d.kid(item, "second").ok_or("posture without data")?;
        let text = |k: &str| d.kid(p, k).map(|x| d.text(x).trim().to_string()).unwrap_or_default();
        let sig = d.kid(p, "signature");
        let list = |k: &str| items(&d, sig.and_then(|s| d.kid(s, k))).iter().map(|&i| d.text(i).trim().parse::<f64>().unwrap_or(0.0)).collect::<Vec<_>>();
        let (angles, inertial) = (list("jointAngles"), list("inertialAngles"));
        let id = num(&d, p, "id") as i64;
        if angles.len() != joints || inertial.len() != 2 {
            return Err(format!("posture {id} has {} joint angles, expected {joints}", angles.len()));
        }
        let neighbours = items(&d, d.kid(p, "neighbours")).iter().map(|&n| (num(&d, n, "postureIndex") as i64, num(&d, n, "cost"), num(&d, n, "speedCoefficient"))).collect();
        nodes.push(Node { id, name: text("name"), family: text("familyName"), angles, inertial: [inertial[0], inertial[1]], neighbours });
    }
    Ok(Graph { nodes })
}

impl Graph {
    /// The posture closest to the current joint angles and torso pitch (hands excluded).
    pub fn nearest(&self, angles: &[f64], torso_pitch: f64, hands: &[usize]) -> usize {
        let d = |n: &Node| n.angles.iter().zip(angles).enumerate().filter(|(i, _)| !hands.contains(i)).map(|(_, (a, b))| (a - b).powi(2)).sum::<f64>() + 4.0 * (n.inertial[1] - torso_pitch).powi(2);
        (0..self.nodes.len()).min_by(|&a, &b| d(&self.nodes[a]).total_cmp(&d(&self.nodes[b]))).unwrap_or(0)
    }

    /// Cheapest path (Dijkstra over the costs) from node index `from` to the posture named `to`.
    pub fn path(&self, from: usize, to: &str) -> Option<Vec<usize>> {
        self.path_avoiding(from, to, &[])
    }

    /// Like `path`, with entering any of the `avoid` posture ids costing 99 more (ALRobotPosture
    /// raises one sitting-arm variant's cost from 1 to 100).
    pub fn path_avoiding(&self, from: usize, to: &str, avoid: &[i64]) -> Option<Vec<usize>> {
        let index: HashMap<i64, usize> = self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut best = vec![f64::INFINITY; self.nodes.len()];
        let mut prev = vec![usize::MAX; self.nodes.len()];
        let mut heap = BinaryHeap::from([(Reverse(0u64), from)]);
        best[from] = 0.0;
        while let Some((Reverse(c), n)) = heap.pop() {
            let cost = f64::from_bits(c);
            if cost > best[n] {
                continue;
            }
            if self.nodes[n].name == to {
                let mut path = vec![n];
                while let Some(&p) = path.last().map(|&l| &prev[l]).filter(|&&p| p != usize::MAX) {
                    path.push(p);
                }
                path.reverse();
                return Some(path);
            }
            for &(m, w, _) in &self.nodes[n].neighbours {
                let Some(&m) = index.get(&m) else { continue };
                let c = cost + w.max(0.0) + if avoid.contains(&self.nodes[m].id) { 99.0 } else { 0.0 };
                if c < best[m] {
                    best[m] = c;
                    prev[m] = n;
                    heap.push((Reverse(c.to_bits()), m)); // non-negative floats order like their bits
                }
            }
        }
        None
    }

    /// Speed coefficient of the edge `a → b` (1 when absent).
    pub fn speed(&self, a: usize, b: usize) -> f64 {
        self.nodes[a].neighbours.iter().find(|n| n.0 == self.nodes[b].id).map_or(1.0, |n| n.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, name: &str, a: f64, next: &[(i64, f64)]) -> String {
        let n: String = next.iter().map(|(m, c)| format!("<item><postureIndex>{m}</postureIndex><cost>{c}</cost><enableAntiCollision>1</enableAntiCollision><speedCoefficient>0.5</speedCoefficient></item>")).collect();
        format!("<item><first>{id}</first><second><id>{id}</id><name>{name}</name><familyId>1</familyId><familyName>F</familyName><neighbours><count>{}</count>{n}</neighbours><signature><jointAngles><count>2</count><item>{a}</item><item>0</item></jointAngles><inertialAngles><count>2</count><item>0</item><item>0</item></inertialAngles></signature></second></item>", next.len())
    }

    #[test]
    fn cheapest_path_through_unnamed_postures() {
        let xml = format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE boost_serialization>\n<boost_serialization><postures><count>4</count>{}{}{}{}</postures></boost_serialization>",
            node(1, "Sit", 0.0, &[(2, 1.0), (3, 5.0)]),
            node(2, "", 1.0, &[(4, 1.0)]),
            node(3, "", 2.0, &[(4, 1.0)]),
            node(4, "Stand", 3.0, &[])
        );
        let g = parse(&xml, 2).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.nearest(&[2.1, 0.0], 0.0, &[]), 2);
        let ids: Vec<i64> = g.path(0, "Stand").unwrap().iter().map(|&i| g.nodes[i].id).collect();
        assert_eq!(ids, [1, 2, 4]);
        assert_eq!(g.speed(0, 1), 0.5);
        assert!(g.path(3, "Sit").is_none());
        let ids: Vec<i64> = g.path_avoiding(0, "Stand", &[2]).unwrap().iter().map(|&i| g.nodes[i].id).collect();
        assert_eq!(ids, [1, 3, 4]);
        assert!(parse(&xml, 26).is_err());
    }
}
