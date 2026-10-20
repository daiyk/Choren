//! The install's NAO meshes parse (skipped without a Choregraphe install).
use std::path::PathBuf;

pub fn share() -> PathBuf {
    std::env::var_os("CHOREGRAPHE_SHARE").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Choregraphe 2.8.8/Choregraphe.app/Contents/Resources/choregraphe-2.8.8-mac/share/choregraphe")
    })
}

#[test]
fn every_nao_mesh_parses() {
    let dir = share().join("../alrobotmodel/meshes");
    let Ok(entries) = std::fs::read_dir(&dir) else { eprintln!("install absent, skipped"); return };
    let mut n = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !(name.starts_with("nao") && name.ends_with(".mesh")) { continue }
        let m = choren_robot::ogre::parse(&std::fs::read(e.path()).unwrap()).unwrap_or_else(|err| panic!("{name}: {err}"));
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for s in &m.submeshes {
            assert!(!s.indices.is_empty() && s.normals.len() == s.positions.len(), "{name}");
            for p in s.positions.chunks(3) { for c in 0..3 { lo[c] = lo[c].min(p[c]); hi[c] = hi[c].max(p[c]); } }
        }
        println!("{name} {} {:?} {lo:.3?} {hi:.3?}", m.version, m.submeshes.iter().map(|s| s.material.as_str()).collect::<Vec<_>>());
        n += 1;
    }
    assert_eq!(n, 33);
    let colors = choren_robot::ogre::material_colors(&std::fs::read_to_string(dir.join("nao.material")).unwrap());
    assert!(colors.iter().any(|c| c.0 == "NaoMat_MainSkin"));
}

/// Calibration: with zero offsets, each child joint lies inside its parent link's mesh (2 cm slack: the shoulders sit just outside the torso shell),
/// which pins every mesh frame to the descriptor's joint frames.
#[test]
fn meshes_line_up_with_the_joints() {
    let dir = share().join("../alrobotmodel/meshes");
    if !dir.is_dir() { eprintln!("install absent, skipped"); return }
    let r = choren_robot::Robot::load("nao6").unwrap();
    let bounds = |mesh: &str| {
        let m = choren_robot::ogre::parse(&std::fs::read(dir.join(format!("{mesh}.mesh"))).unwrap()).unwrap();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for p in m.submeshes.iter().flat_map(|s| s.positions.chunks(3)) { for c in 0..3 { lo[c] = lo[c].min(p[c] as f64); hi[c] = hi[c].max(p[c] as f64); } }
        (lo, hi)
    };
    let mut checked = 0;
    for j in &r.joints {
        // The child's origin, expressed in the nearest ancestor link that has a mesh.
        let (mut link, mut at) = (j.parent.clone(), j.origin);
        while let Some(l) = link.clone().filter(|l| !r.meshes.contains_key(l)) {
            let p = &r.joints[r.joint_index(&l).unwrap()];
            at = [at[0] + p.origin[0], at[1] + p.origin[1], at[2] + p.origin[2]];
            link = p.parent.clone();
        }
        let link = link.unwrap_or_else(|| "Torso".into());
        let (lo, hi) = bounds(&r.meshes[&link]);
        for c in 0..3 {
            assert!(at[c] > lo[c] - 0.02 && at[c] < hi[c] + 0.02, "{} in {link}: {at:?} not in {lo:?}..{hi:?}", j.name);
        }
        checked += 1;
    }
    assert_eq!(checked, 26);
}
