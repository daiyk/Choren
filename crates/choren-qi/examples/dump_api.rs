//! Prints the services of a running NAOqi (or Choren virtual robot) as JSON lines:
//! `{"service": …, "methods": [[name, params, ret]…], "signals": [[name, sig]…]}`, or an error.
//! Services hosted by other processes are reached through the endpoints the directory gives.
//!
//! `cargo run -p choren-qi --example dump_api -- tcp://127.0.0.1:9559 [Service…]`
use choren_qi::Client;

fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            c if (c as u32) < 0x20 => out += &format!("\\u{:04x}", c as u32),
            c => out.push(c),
        }
    }
    out + "\""
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "tcp://127.0.0.1:9559".into());
    let client = Client::connect(&url).await?;
    let mut names: Vec<String> = args.collect();
    if names.is_empty() {
        names = client.services().await?;
    }
    for name in names {
        match tokio::time::timeout(std::time::Duration::from_secs(10), client.service(&name)).await {
            Ok(Ok((_, meta))) => {
                let methods: Vec<String> = meta.methods.values().map(|m| format!("[{},{},{}]", q(&m.name), q(&m.params), q(&m.ret))).collect();
                let signals: Vec<String> = meta.signals.values().map(|s| format!("[{},{}]", q(&s.name), q(&s.sig))).collect();
                println!("{{\"service\":{},\"methods\":[{}],\"signals\":[{}]}}", q(&name), methods.join(","), signals.join(","));
            }
            Ok(Err(e)) => println!("{{\"service\":{},\"error\":{}}}", q(&name), q(&e)),
            Err(_) => println!("{{\"service\":{},\"error\":\"timeout\"}}", q(&name)),
        }
    }
    Ok(())
}
