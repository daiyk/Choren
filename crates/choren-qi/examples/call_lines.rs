//! Calls the methods read from stdin and prints `line<TAB>result<TAB>seconds` with the result as
//! JSON and the call's duration, or `ERROR: …` (calls time out after 120 s; posture changes on
//! Choregraphe's virtual robot take up to ~25 s). A line is `Service.method` followed by tab-separated arguments: `s:text`, `i:3`,
//! `f:0.5`, `b:true`, or `l:a,b` (a list of strings). `watch<TAB>Service.signal` prints each later
//! emission as `EVENT<TAB>Service.signal<TAB>args<TAB>seconds` (since start); `sleep<TAB>0.5` waits.
//!
//! `cargo run -p choren-qi --example call_lines -- tcp://127.0.0.1:9559 < methods.txt`
use choren_qi::{Client, Value};
use std::io::BufRead;

fn json(v: &Value) -> String {
    match v.plain() {
        Value::Void => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => if f.is_finite() { format!("{f:?}") } else { "null".into() },
        Value::Double(f) => if f.is_finite() { format!("{f:?}") } else { "null".into() },
        Value::Str(s) => format!("{s:?}"),
        Value::Raw(b) => format!("{:?}", String::from_utf8_lossy(b)),
        Value::List(l) | Value::Tuple(l) => format!("[{}]", l.iter().map(json).collect::<Vec<_>>().join(",")),
        Value::Map(m) => format!("[{}]", m.iter().map(|(k, v)| format!("[{},{}]", json(k), json(v))).collect::<Vec<_>>().join(",")),
        Value::Dynamic(_) => unreachable!(),
        Value::Object(..) => "null".into(),
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let url = std::env::args().nth(1).unwrap_or_else(|| "tcp://127.0.0.1:9559".into());
    let client = Client::connect(&url).await?;
    let start = std::time::Instant::now();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(signal) = line.trim_end().strip_prefix("watch\t") {
            let (service, name) = signal.split_once('.').ok_or("watch Service.signal")?;
            let mut rx = client.subscribe(service, name).await?;
            let signal = signal.to_string();
            tokio::spawn(async move {
                while let Some(v) = rx.recv().await {
                    println!("EVENT\t{signal}\t{}\t{:.2}s", json(&v), start.elapsed().as_secs_f64());
                }
            });
            continue;
        }
        if let Some(secs) = line.trim_end().strip_prefix("sleep\t") {
            tokio::time::sleep(std::time::Duration::from_secs_f64(secs.parse().unwrap_or(0.0))).await;
            continue;
        }
        let mut parts = line.trim_end().split('\t');
        let Some((service, method)) = parts.next().and_then(|m| m.split_once('.')) else { continue };
        let args: Vec<Value> = parts
            .filter_map(|a| a.split_once(':'))
            .map(|(t, v)| match t {
                "i" => Value::Int(v.parse().unwrap_or(0)),
                "f" => Value::Float(v.parse().unwrap_or(0.0)),
                "b" => Value::Bool(v == "true"),
                "l" => Value::List(v.split(',').filter(|x| !x.is_empty()).map(Value::from).collect()),
                _ => Value::from(v),
            })
            .collect();
        let t0 = std::time::Instant::now();
        let r = tokio::time::timeout(std::time::Duration::from_secs(120), client.call(service, method, args)).await;
        match r {
            Ok(Ok(v)) => println!("{line}\t{}\t{:.2}s", json(&v), t0.elapsed().as_secs_f64()),
            Ok(Err(e)) => println!("{line}\tERROR: {}", e.replace('\n', " ")),
            Err(_) => println!("{line}\tERROR: timeout"),
        }
    }
    Ok(())
}
