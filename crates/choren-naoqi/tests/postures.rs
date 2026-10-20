//! The install's 168-node posture graph drives goToPosture (skipped without a Choregraphe install).
use choren_naoqi::Robotd;
use choren_robot::Robot;
use std::path::PathBuf;

fn graph_file() -> PathBuf {
    let share = std::env::var_os("CHOREGRAPHE_SHARE").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
    });
    share.join("../naoqi/robot_posture/postures_naoh25.xml")
}

/// Runs a posture change on a simulator stepped as fast as possible, one motion cycle at a time.
async fn go_to(d: &Robotd, target: &str) -> bool {
    let mut fut = std::pin::pin!(d.go_to_posture(target, 1.0));
    loop {
        tokio::select! {
            biased;
            out = &mut fut => return out.unwrap(),
            _ = tokio::task::yield_now() => d.sim.step(d.sim.dt()),
        }
    }
}

#[tokio::test]
async fn go_to_posture_walks_the_install_graph() {
    let file = graph_file();
    if !file.exists() { eprintln!("install absent, skipped"); return }
    let d = Robotd::new(Robot::load("nao6").unwrap());
    assert_eq!(d.load_posture_graph(&file).unwrap(), 168);
    {
        let g = d.posture_graph.lock().unwrap();
        let g = g.as_ref().unwrap();
        let mut named: Vec<&str> = g.nodes.iter().map(|n| n.name.as_str()).filter(|n| !n.is_empty()).collect();
        named.sort();
        assert_eq!(named, ["Crouch", "LyingBack", "LyingBelly", "Sit", "SitRelax", "Stand", "StandInit", "StandZero"]);
        let sit = g.nodes.iter().position(|n| n.name == "Sit").unwrap();
        let path = g.path(sit, "StandInit").unwrap();
        assert!(path.len() > 2, "goes through intermediate postures: {path:?}");
        assert!(path[1..path.len() - 1].iter().all(|&i| g.nodes[i].name.is_empty() || g.nodes[i].name != "StandInit"));
    }
    for target in ["Sit", "StandInit", "LyingBack", "Stand"] {
        assert!(go_to(&d, target).await, "{target}");
        assert_eq!(d.posture().name, target);
    }
}
