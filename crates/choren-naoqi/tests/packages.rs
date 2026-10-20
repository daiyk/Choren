//! Packages on the virtual robot: names, failed reinstalls.
use choren_core::pkg;
use choren_naoqi::runtime::Runtime;
use std::collections::BTreeMap;

fn package(uuid: &str, behaviors: &[(&str, &str)]) -> Vec<u8> {
    let contents: String = behaviors.iter().map(|(f, _)| format!("<behaviorContent path=\"{f}\"><nature>interactive</nature><permissions/></behaviorContent>")).collect();
    let mut files = BTreeMap::from([("manifest.xml".to_string(), format!("<?xml version='1.0' encoding='UTF-8'?>\n<package uuid=\"{uuid}\" version=\"0.0.0\"><contents>{contents}</contents></package>\n").into_bytes())]);
    for (folder, xar) in behaviors {
        if !xar.is_empty() {
            files.insert(format!("{folder}/behavior.xar"), xar.as_bytes().to_vec());
        }
    }
    pkg::zip(&files)
}

const XAR: &str = r#"<?xml version="1.0" encoding="UTF-8" ?><ChoregrapheProject xmlns="http://www.ald.softbankrobotics.com/schema/choregraphe/project.xsd" xar_version="3"><Box name="root" id="-1" localization="8" tooltip="" x="0" y="0"><Input name="onStart" type="1" type_size="1" nature="2" inner="0" tooltip="" id="1" /></Box></ChoregrapheProject>"#;

#[test]
fn uuids_are_one_folder_and_a_broken_reinstall_keeps_the_old_version() {
    let rt = Runtime::default();
    assert_eq!(rt.install_pkg(&package("demo", &[("behavior_1", XAR)])).unwrap(), "demo");
    assert_eq!(rt.installed(), ["demo/behavior_1"]);
    for bad in ["demo/behavior_1", "..", ".", "a\\b", ""] {
        assert!(rt.install_pkg(&package(bad, &[("behavior_1", XAR)])).unwrap_err().contains("Invalid package uuid"), "{bad}");
    }
    // Play's own package name starts with a dot (libpackagesynchronizer).
    assert!(rt.install_pkg(&package(".lastUploadedChoregrapheBehavior", &[("behavior_1", XAR)])).is_ok());
    // A new version whose behavior is missing, escapes the package or is too large fails, and the
    // installed one still works.
    assert!(rt.install_pkg(&package("demo", &[("behavior_1", ""), ("behavior_2", XAR)])).is_err());
    assert!(rt.install_pkg(&package("demo", &[("../behavior_1", "")])).unwrap_err().contains("outside"));
    let huge = format!("{XAR}{}", " ".repeat(17 << 20));
    assert!(rt.install_pkg(&package("demo", &[("behavior_2", &huge)])).unwrap_err().contains("too large"));
    assert!(rt.install_pkg(&package("demo", &[("behavior_2", "<x/>")])).unwrap_err().contains("no root Box"));
    let big_manifest = pkg::zip(&BTreeMap::from([("manifest.xml".to_string(), format!("<package uuid=\"demo\">{}</package>", " ".repeat(17 << 20)).into_bytes())]));
    assert!(rt.install_pkg(&big_manifest).unwrap_err().contains("too large"));
    assert_eq!(rt.installed(), [".lastUploadedChoregrapheBehavior/behavior_1", "demo/behavior_1"]);
    // Uuids differing only in case stay two packages, even on a case-insensitive file system.
    rt.install_pkg(&package("DEMO", &[("other", XAR)])).unwrap();
    assert_eq!(rt.installed(), [".lastUploadedChoregrapheBehavior/behavior_1", "DEMO/other", "demo/behavior_1"]);
    // A package replacing one `install` registered from a project folder leaves that folder alone.
    let project = std::env::temp_dir().join(format!("choren-packages-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).unwrap();
    choren_core::project::Project::create(&project, "local").unwrap();
    let local = rt.install(project.to_str().unwrap()).unwrap();
    rt.install_pkg(&package(&local, &[("behavior_1", XAR)])).unwrap();
    assert!(project.join("local.pml").exists());
    std::fs::remove_dir_all(&project).unwrap();
    // Concurrent reinstalls of one package each land whole; a listed path counts once.
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| rt.install_pkg(&package("demo", &[("behavior_2", XAR), ("behavior_2", XAR)])).unwrap());
        }
    });
    assert!(rt.installed().contains(&"demo/behavior_2".to_string()));
    // Once the robot removed its packages, a late install fails instead of recreating the folder.
    rt.remove_packages();
    assert!(rt.install_pkg(&package("late", &[("behavior_1", XAR)])).is_err());
}
