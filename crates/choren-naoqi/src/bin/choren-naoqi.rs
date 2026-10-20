//! Headless virtual robot: `choren-naoqi [--robot nao6] [--listen 127.0.0.1:9559] [--python python3]
//! [--share-dir <install>/share]`. With `--share-dir`, postures follow the install's posture graph,
//! as in the app.

#[tokio::main]
async fn main() {
    let mut robot = "nao6".to_string();
    let mut listen = "127.0.0.1:9559".to_string();
    let mut python = choren_naoqi::runtime::PythonConfig::default();
    let mut share: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--robot" => robot = args.next().unwrap_or(robot),
            "--listen" => listen = args.next().unwrap_or(listen),
            "--python" => python.exe = args.next().unwrap_or(python.exe),
            "--python-dir" => python.dir = args.next().map(Into::into).unwrap_or(python.dir),
            "--share-dir" => share = args.next().map(Into::into),
            _ => {
                eprintln!("usage: choren-naoqi [--robot nao6] [--listen HOST:PORT] [--python EXE] [--python-dir DIR] [--share-dir DIR]");
                std::process::exit(2);
            }
        }
    }
    match choren_naoqi::start(&robot, &listen, Some(python)).await {
        Ok(vr) => {
            if let (Some(share), Some(graph)) = (&share, vr.robotd.sim.robot.install_postures.clone()) {
                if let Err(e) = vr.robotd.load_posture_graph(&share.join("naoqi/robot_posture").join(graph)) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            println!("Virtual {robot} listening on tcp://{}", vr.addr);
            let _ = tokio::signal::ctrl_c().await;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
