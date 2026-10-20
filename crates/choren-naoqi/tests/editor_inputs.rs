//! Manual editor starts use the same scripts, links and cleanup as normal flow execution.
use choren_core::project::Project;
use choren_qi::{Client, Value};
use std::time::Duration;

async fn memory_is(c: &Client, key: &str, value: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if c.call("ALMemory", "getData", vec![key.into()]).await.is_ok_and(|v| v.as_str() == Some(value)) { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.unwrap_or_else(|_| panic!("{key} did not become {value}"));
}

async fn activate(c: &Client, name: &str, path: &[[i64; 3]], input: i64) {
    let path = Value::List(path.iter().map(|step| Value::List(step.iter().copied().map(Value::Int).collect())).collect());
    c.call("ChorenSim", "activateInput", vec![name.into(), path, Value::Int(input), Value::Void]).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn independent_starts_in_inactive_keyframes_route_links_and_stop_cleanly() {
    let dir = std::env::temp_dir().join(format!("choren-editor-independent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = Project::create(&dir, "independent").unwrap();
    std::fs::write(dir.join(&p.behaviors[0].source), r#"
<ChoregrapheProject><Box id="-1" name="Root"><Input id="2" name="onStart" type="1" nature="2"/>
<Timeline enable="0"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram>
<Box id="10" name="Unstarted timeline"><Input id="2" name="onStart" type="1" nature="2"/><Input id="3" name="onStop" type="1" nature="3"/>
<Timeline enable="1" fps="25" size="1000"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram/></BehaviorKeyframe><BehaviorKeyframe index="500"><Diagram>
<Box id="20" name="Unstarted flow"><Input id="2" name="onStart" type="1" nature="2"/><Input id="3" name="onStop" type="1" nature="3"/>
<Timeline enable="0"><BehaviorLayer><BehaviorKeyframe index="1"><Diagram>
<Box id="30" name="Independent script"><Input id="2" name="onStart" type="1" nature="2"/><Output id="4" name="pulse" type="1" nature="2"/>
<script><content><![CDATA[class MyClass(GeneratedClass):
    def onLoad(self):
        self.count = 0
        self.session().service("ALMemory").insertData("Editor/State", "loaded")
    def onInput_onStart(self):
        self.count += 1
        self.session().service("ALMemory").insertData("Editor/Count", str(self.count))
        self.pulse()
    def onUnload(self):
        self.session().service("ALMemory").insertData("Editor/State", "unloaded")
]]></content></script><Resource name="Speech" type="Lock" timeout="0"/></Box>
<Box id="31" name="Linked follower"><Input id="2" name="onStart" type="1" nature="2"/><script><content><![CDATA[class MyClass(GeneratedClass):
    def onInput_onStart(self):
        self.session().service("ALMemory").insertData("Editor/Link", "followed")
]]></content></script></Box>
<Box id="32" name="Independent motion"><Input id="2" name="onStart" type="1" nature="2"/><Timeline enable="1" fps="25" size="2500">
<ActuatorList><ActuatorCurve actuator="HeadYaw" mute="0" unit="0"><Key frame="1" value="0"/><Key frame="2500" value="60"/></ActuatorCurve></ActuatorList>
</Timeline></Box>
<Box id="33" name="Parent start marker"><Input id="2" name="onStart" type="1" nature="2"/><script><content><![CDATA[class MyClass(GeneratedClass):
    def onInput_onStart(self):
        self.session().service("ALMemory").insertData("Editor/Parent", "started")
]]></content></script></Box>
<Link outputowner="30" indexofoutput="4" inputowner="31" indexofinput="2"/>
<Link outputowner="0" indexofoutput="2" inputowner="33" indexofinput="2"/>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box>
</Diagram></BehaviorKeyframe></BehaviorLayer></Timeline></Box></ChoregrapheProject>
"#).unwrap();
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    let uuid = c.call("PackageManager", "_install", vec![dir.to_string_lossy().as_ref().into()]).await.unwrap();
    let name = format!("{}/behavior_1", uuid.as_str().unwrap());
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await.unwrap();
    let flow = [[0, 0, 10], [0, 1, 20]];
    let script = [[0, 0, 10], [0, 1, 20], [0, 0, 30]];
    let motion = [[0, 0, 10], [0, 1, 20], [0, 0, 32]];
    activate(&c, &name, &script, 2).await;
    memory_is(&c, "Editor/State", "loaded").await;
    memory_is(&c, "Editor/Count", "1").await;
    memory_is(&c, "Editor/Link", "followed").await;
    activate(&c, &name, &script, 2).await;
    memory_is(&c, "Editor/Count", "2").await; // onLoad must not reset an already loaded box.
    assert!(c.call("ALMemory", "getData", vec!["Editor/Parent".into()]).await.is_err(), "a manual start does not stimulate its ancestors' start rails");
    activate(&c, &name, &motion, 2).await;
    assert_eq!(c.call("ALMotion", "getTaskList", vec![]).await.unwrap().items().len(), 1);
    // Even a never-started enclosing timeline must stop its manually activated descendants.
    activate(&c, &name, &[[0, 0, 10]], 3).await;
    memory_is(&c, "Editor/State", "unloaded").await;
    assert!(c.call("ALMotion", "getTaskList", vec![]).await.unwrap().items().is_empty());
    assert_eq!(c.call("ALResourceManager", "areResourcesFree", vec![Value::List(vec!["Speech".into()])]).await.unwrap(), Value::Bool(true));
    // Starting the individual flow still follows its own rails normally.
    activate(&c, &name, &flow, 2).await;
    memory_is(&c, "Editor/Parent", "started").await;
    activate(&c, &name, &script, 2).await;
    memory_is(&c, "Editor/Count", "1").await;
    activate(&c, &name, &motion, 2).await;
    c.call("ALBehaviorManager", "stopBehavior", vec![name.as_str().into()]).await.unwrap();
    memory_is(&c, "Editor/State", "unloaded").await;
    assert!(c.call("ALMotion", "getTaskList", vec![]).await.unwrap().items().is_empty());
    assert_eq!(c.call("ALResourceManager", "areResourcesFree", vec![Value::List(vec!["Speech".into()])]).await.unwrap(), Value::Bool(true));
    std::fs::remove_dir_all(dir).unwrap();
}
