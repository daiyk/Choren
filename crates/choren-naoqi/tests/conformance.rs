//! The virtual robot serves the same API as Choregraphe's own virtual NAO 6
//! (`api/naoqi-2.8.6-nao6.json`), so box scripts see the same methods, overloads and types.
use choren_naoqi::conformance::{api, MODULE_BASE};

/// Methods Choren serves on reference services in addition to NAOqi 2.8's: service, method,
/// parameters, and why.
const EXTRAS: &[(&str, &str, &str, &str)] = &[
    ("PackageManager", "_install", "(s)", "installs a project folder by path and returns its package id (the editor's Play)"),
];

#[tokio::test(flavor = "multi_thread")]
async fn every_reference_method_is_served_with_its_signature() {
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let node = &vr.robotd.node;
    let mut problems = vec![];
    for (service, reference) in api() {
        let Some(meta) = node.meta(service) else {
            problems.push(format!("missing service {service}"));
            continue;
        };
        for (name, params, ret) in &reference.methods {
            match meta.methods.values().find(|m| &m.name == name && &m.params == params) {
                None => problems.push(format!("{service}.{name}{params} missing")),
                Some(m) if &m.ret != ret => problems.push(format!("{service}.{name}{params} returns {} instead of {ret}", m.ret)),
                _ => {}
            }
        }
        for m in meta.methods.values().filter(|m| m.uid >= 100) {
            let known = reference.methods.iter().any(|r| r.0 == m.name && r.1 == m.params);
            if !known && !EXTRAS.iter().any(|e| e.0 == service && e.1 == m.name && e.2 == m.params) {
                problems.push(format!("{service}.{}{} -> {} is not in NAOqi 2.8", m.name, m.params, m.ret));
            }
        }
        for (name, sig) in &reference.signals {
            match meta.signals.values().find(|s| &s.name == name) {
                None => problems.push(format!("{service} signal {name} missing")),
                Some(s) if &s.sig != sig => problems.push(format!("{service} signal {name} is {} instead of {sig}", s.sig)),
                _ => {}
            }
        }
    }
    // Hand-written modules the reference could not record still serve ALModule's methods.
    for service in node.service_names().iter().filter(|s| s.starts_with("AL") && !api().contains_key(*s)) {
        let meta = node.meta(service).unwrap();
        for (name, params, _) in MODULE_BASE {
            if !meta.methods.values().any(|m| &m.name == name && &m.params == params) {
                problems.push(format!("{service}.{name}{params} (ALModule) missing"));
            }
        }
    }
    assert!(problems.is_empty(), "{} differences from NAOqi 2.8:\n{}", problems.len(), problems.join("\n"));
}

/// Lists whose order NAOqi does not define (hash order).
const UNORDERED: &[&str] = &["ALMemory.getDataListName", "ALMemory.getEventList", "ALMemory.getDataList"];

fn sorted(v: &serde_json::Value) -> serde_json::Value {
    let mut items = v.as_array().cloned().unwrap_or_default();
    items.sort_by_key(|x| x.to_string());
    serde_json::Value::Array(items)
}

/// Recorded answers Choren deliberately does not reproduce: method, reason.
const DIFFERENT_ANSWERS: &[(&str, &str)] = &[
    ("ALMemory.getDataList", "memory starts with the declared events and default keys only, not NAOqi's internal ones"),
    ("ALMemory.getDataListName", "memory starts with the declared events and default keys only, not NAOqi's internal ones"),
];

fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    // Maps come back as objects here and may be recorded as [key, value] pairs.
    let pairs = |o: &serde_json::Map<String, J>| J::Array(o.iter().map(|(k, v)| J::Array(vec![J::String(k.clone()), v.clone()])).collect());
    match (a, b) {
        (J::Object(x), J::Array(_)) => return same(&pairs(x), b),
        (J::Array(_), J::Object(y)) => return same(a, &pairs(y)),
        (J::Number(x), J::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 1e-3 * x.abs().max(1.0)
        }
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (J::Object(x), J::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

/// "Service.method" → (arguments, result), as in `api/nao6-defaults.json`'s `answers`.
type Answers = Vec<(String, serde_json::Value, serde_json::Value)>;

async fn check(vr: &choren_naoqi::VirtualRobot, answers: &Answers, problems: &mut Vec<String>) {
    for (method, args, expected) in answers {
        let (service, name) = method.split_once('.').unwrap();
        let args: Vec<_> = args.as_array().unwrap().iter().map(choren_naoqi::runtime::json_to_value).collect();
        match vr.robotd.node.call(service, name, args).await.map(|v| choren_naoqi::runtime::value_to_json(&v)) {
            Ok(v) if same(&v, expected) => {}
            Ok(v) if UNORDERED.contains(&method.as_str()) && same(&sorted(&v), &sorted(expected)) => {}
            Ok(v) => problems.push(format!("{method}: {} instead of {}", short(&v), short(expected))),
            Err(e) => problems.push(format!("{method}: error {e:?} instead of {}", short(expected))),
        }
    }
}

/// Stubbed getters answer as `api/nao6-defaults.json` says.
#[tokio::test(flavor = "multi_thread")]
async fn stubs_give_the_default_answers() {
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let answers: Answers = choren_naoqi::conformance::defaults().answers.iter().flat_map(|(m, r)| r.iter().map(|(a, v)| (m.clone(), a.clone(), v.clone()))).collect();
    let mut problems = vec![];
    check(&vr, &answers, &mut problems).await;
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// With a local recording of Choregraphe's virtual robot right after start-up (not part of the
/// repository; `NAOQI_REFERENCE`, default `docs/reference/naoqi-2.8.6-nao6-responses.json`), every
/// simulated read gets the same answer here. Stubs return type defaults, so they are skipped.
#[tokio::test(flavor = "multi_thread")]
async fn simulated_reads_answer_as_the_local_reference() {
    let path = std::env::var("NAOQI_REFERENCE").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/reference/naoqi-2.8.6-nao6-responses.json").into());
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no NAOqi reference at {path}");
        return;
    };
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let modules = &vr.robotd.modules;
    let mut answers: Answers = json["responses"].as_array().unwrap().iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].clone(), r[2].clone())).collect();
    answers.retain(|(m, a, _)| {
        let (service, name) = m.split_once('.').unwrap();
        let args: Vec<_> = a.as_array().unwrap().iter().map(choren_naoqi::runtime::json_to_value).collect();
        !DIFFERENT_ANSWERS.iter().any(|d| d.0 == m) && (!modules.is_stub(m) || choren_naoqi::conformance::answer(service, name, &args).is_some())
    });
    answers.sort_by(|a, b| a.0.cmp(&b.0));
    let mut problems = vec![];
    check(&vr, &answers, &mut problems).await;
    assert!(problems.is_empty(), "{} answers differ from the reference:\n{}", problems.len(), problems.join("\n"));
}

fn short(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 160 { format!("{}…", &s[..160]) } else { s }
}

/// Error texts and kept state measured live against Choregraphe's virtual robot (2026-10-18).
#[tokio::test(flavor = "multi_thread")]
async fn led_errors_and_speech_volume_match_the_reference() {
    use choren_qi::{Client, Value};
    let vr = choren_naoqi::start("nao6", "127.0.0.1:0", None).await.unwrap();
    let c = Client::connect(&vr.addr.to_string()).await.unwrap();
    let call = |m: &'static str, a: Vec<Value>| c.call(m.split_once('.').unwrap().0, m.split_once('.').unwrap().1, a);
    let quote = "Could not parse 'Bogus 'as a LED Device, RGB-LED or Group";
    for (method, args, expected) in [
        ("ALLeds.setIntensity", vec!["Bogus".into(), Value::Float(0.5)], format!("ALLeds: setIntensity: {quote}")),
        ("ALLeds.on", vec!["Bogus".into()], format!("ALLeds: setIntensity: {quote}")),
        ("ALLeds.fade", vec!["Bogus".into(), Value::Float(0.5), Value::Float(0.1)], format!("ALLeds: fade: {quote}")),
        ("ALLeds.reset", vec!["Bogus".into()], quote.to_string()),
        ("ALLeds.fadeRGB", vec!["Bogus".into(), "red".into(), Value::Float(0.1)], "ALLeds: fadeRGB: Could not parse 'Bogus' as a LED Device, RGB-LED or Group".into()),
        ("ALLeds.fadeRGB", vec!["FaceLeds".into(), "pink".into(), Value::Float(0.1)], "Color given as parameter pink is not supported. Supported colors are: white, red, green, blue, yellow, magenta, cyan".into()),
        ("ALLeds.listGroup", vec!["Bogus".into()], "ALLeds: listGroup: Could not parse pName as a group name".into()),
        ("ALTextToSpeech.setVolume", vec![Value::Float(-0.2)], "\tALTextToSpeechCore::setVolume\n\tWrong value for volume, must be between 0.0 and 1.0".into()),
    ] {
        assert_eq!(call(method, args).await.unwrap_err(), expected, "{method}");
    }
    call("ALTextToSpeech.setVolume", vec![Value::Float(1.5)]).await.unwrap();
    assert_eq!(call("ALTextToSpeech.getVolume", vec![]).await.unwrap().as_f64(), Some(1.5));
}
