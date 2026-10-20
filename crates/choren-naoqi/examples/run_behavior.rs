//! Runs one behavior of a project on a headless virtual NAO 6 and prints the robot log, to check
//! a project against the simulation without the editor.
//!
//! `cargo run -p choren-naoqi --example run_behavior -- <project.pml> <behavior folder> [seconds] [share dir] [box id]`
//!
//! With a box id, that root-diagram box's onStart is activated as from the editor.
use choren_qi::{Client, Value};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (pml, behavior) = (args.next().ok_or("project .pml expected")?, args.next().ok_or("behavior folder expected")?);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await?;
    let share = args.next().filter(|s| !s.is_empty());
    let activate: Option<i64> = args.next().and_then(|s| s.parse().ok());
    if let Some(share) = share {
        let graph = std::path::Path::new(&share).join("naoqi/robot_posture/postures_naoh25.xml");
        vr.robotd.load_posture_graph(&graph)?;
    }
    let c = Client::connect(&vr.addr.to_string()).await?;
    let mut log = c.subscribe("ChorenSim", "log").await?;
    let mut spoken = c.subscribe("ChorenSim", "spoken").await?;
    let uuid = c.call("PackageManager", "_install", vec![pml.as_str().into()]).await?;
    let name = format!("{}/{behavior}", uuid.as_str().unwrap_or_default());
    c.call("ALBehaviorManager", "startBehavior", vec![name.as_str().into()]).await?;
    println!("started {name}");
    if let Some(id) = activate {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let path = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(0), Value::Int(id)])]);
        c.call("ChorenSim", "activateInput", vec![name.as_str().into(), path, Value::Int(2), Value::Void]).await?;
        println!("activated box {id} onStart");
    }
    let end = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        tokio::select! {
            Some(v) = log.recv() => {
                let f = v.items();
                let s = |i: usize| f.get(i).and_then(Value::as_str).unwrap_or("").to_string();
                println!("[{}] {}: {}", s(0), s(1), s(2));
            }
            Some(v) = spoken.recv() => println!("[say] {}", v.items().first().and_then(Value::as_str).unwrap_or("")),
            _ = tokio::time::sleep_until(end) => break,
        }
    }
    let running = c.call("ALBehaviorManager", "isBehaviorRunning", vec![name.as_str().into()]).await?;
    println!("running after {seconds} s: {running:?}");
    Ok(())
}
