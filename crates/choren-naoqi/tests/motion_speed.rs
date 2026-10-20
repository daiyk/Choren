//! The editor speed override keeps timeline completion synchronized with robot movement.
use choren_core::project::Project;
use choren_qi::{Client, Value};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn speed_controls_timeline_completion_and_can_change_during_a_gesture() {
    let dir = std::env::temp_dir().join(format!("choren-motion-speed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = Project::create(&dir, "speed").unwrap();
    std::fs::write(dir.join(&p.behaviors[0].source), r#"
<ChoregrapheProject><Box id="-1" name="Root"><Input id="2" name="onStart" type="1" nature="2"/><Output id="4" name="onStopped" type="1" nature="1"/>
<Timeline enable="0"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram>
<Box id="10" name="Gesture"><Input id="2" name="onStart" type="1" nature="2"/><Output id="4" name="onStopped" type="1" nature="1"/>
<Timeline enable="1" fps="25" size="21"><ActuatorList><ActuatorCurve actuator="HeadYaw" mute="0" unit="0"><Key frame="1" value="0"/><Key frame="21" value="30"/></ActuatorCurve></ActuatorList></Timeline></Box>
<Link outputowner="0" indexofoutput="2" inputowner="10" indexofinput="2"/>
<Link outputowner="10" indexofoutput="4" inputowner="0" indexofinput="4"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>
"#).unwrap();
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    for (speed, change, min, max) in [(1.0, false, 0.65, 2.0), (0.25, false, 2.8, 5.0), (2.0, false, 0.3, 1.3), (0.25, true, 0.6, 2.0)] {
        c.call("ChorenSim", "setAngles", vec![Value::List(vec!["HeadYaw".into()]), Value::floats([0.0])]).await.unwrap();
        c.call("ChorenSim", "setMotionSpeed", vec![Value::Float(speed)]).await.unwrap();
        let state = c.call("ChorenSim", "getState", vec![]).await.unwrap();
        let Value::Map(fields) = state.plain() else { panic!() };
        assert_eq!(fields.iter().find(|(k, _)| k.as_str() == Some("motionSpeed")).unwrap().1.as_f64(), Some(speed as f64));
        let began = Instant::now();
        c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
        if change {
            tokio::time::sleep(Duration::from_millis(400)).await;
            c.call("ChorenSim", "setMotionSpeed", vec![Value::Float(2.0)]).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(6), async {
            while c.call("ALBehaviorManager", "isBehaviorRunning", vec![name.as_str().into()]).await.unwrap().as_bool() == Some(true) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let elapsed = began.elapsed().as_secs_f64();
        assert!((min..max).contains(&elapsed), "speed={speed}, live change={change}: {elapsed}s");
        let yaw = c.call("ALMotion", "getAngles", vec!["HeadYaw".into(), true.into()]).await.unwrap();
        assert!((yaw.items()[0].as_f64().unwrap() - std::f64::consts::PI / 6.0).abs() < 0.02, "timeline must not cut the slowed gesture short: {yaw:?}");
    }
    assert!(c.call("ChorenSim", "setMotionSpeed", vec![Value::Float(0.0)]).await.unwrap_err().contains("between"));
    std::fs::remove_dir_all(dir).unwrap();
}
