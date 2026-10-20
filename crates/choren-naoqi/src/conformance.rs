//! The service surface of Choregraphe's own virtual NAO 6 (NAOqi 2.8.6.14), recorded in
//! `api/naoqi-2.8.6-nao6.json` with `qicli info --raw-signature`. Every service and method listed
//! there exists on Choren's virtual robot with the same signature:
//! - the simulated ones from `services.rs` and `runtime.rs`;
//! - the ALModule base methods (`pCall`, `wait`, `stop`, `isRunning`, `ping`, …) on every module;
//! - the rest as stubs that return their answer in `api/nao6-defaults.json`, or else a default of
//!   the declared type, and log once that they are not simulated. Box scripts then behave as on Choregraphe's virtual robot, where most perception
//!   and hardware services also exist without doing anything.
//!
//! Services hosted in NAOqi's helper processes (ALAnimatedSpeech, ALDialog, ALAutonomousLife, …)
//! could not be read; the hand-written ones get the ALModule base methods only.

use crate::Robotd;
use choren_qi::{ServiceBuilder, Sig, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::watch;

pub const NAOQI_VERSION: &str = "2.8.6.14";
/// What `ALModule.version()` returns on NAOqi 2.8.
pub const MODULE_VERSION: &str = "2.8";

pub struct ServiceApi {
    /// (name, parameter signature, return signature), overloads repeated.
    pub methods: Vec<(String, String, String)>,
    pub signals: Vec<(String, String)>,
}

/// The reference surface, by service name.
pub fn api() -> &'static BTreeMap<String, ServiceApi> {
    static API: OnceLock<BTreeMap<String, ServiceApi>> = OnceLock::new();
    API.get_or_init(|| {
        let json: serde_json::Value = serde_json::from_str(include_str!("../api/naoqi-2.8.6-nao6.json")).expect("valid API table");
        let strs = |v: &serde_json::Value| v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>();
        json["services"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, s)| {
                let methods = s["methods"].as_array().unwrap().iter().map(|m| { let m = strs(m); (m[0].clone(), m[1].clone(), m[2].clone()) }).collect();
                let signals = s["signals"].as_array().unwrap().iter().map(|m| { let m = strs(m); (m[0].clone(), m[1].clone()) }).collect();
                (name.clone(), ServiceApi { methods, signals })
            })
            .collect()
    })
}

/// The virtual robot's start-up state beyond type defaults (`api/nao6-defaults.json`):
/// "Service.method" → (arguments, result) for stubbed getters, ALMemory keys and event names.
pub struct Defaults {
    pub answers: HashMap<String, Vec<(serde_json::Value, serde_json::Value)>>,
    pub memory: Vec<(String, serde_json::Value)>,
    pub events: Vec<String>,
}

pub fn defaults() -> &'static Defaults {
    static D: OnceLock<Defaults> = OnceLock::new();
    D.get_or_init(|| {
        let json: serde_json::Value = serde_json::from_str(include_str!("../api/nao6-defaults.json")).expect("valid defaults");
        let mut answers: HashMap<String, Vec<_>> = HashMap::new();
        for r in json["answers"].as_array().unwrap() {
            answers.entry(r[0].as_str().unwrap().to_string()).or_default().push((r[1].clone(), r[2].clone()));
        }
        let memory = json["memory"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let events = json["events"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect();
        Defaults { answers, memory, events }
    })
}

/// The default answer to `service.method(args)`, if `api/nao6-defaults.json` has one.
pub fn answer(service: &str, method: &str, args: &[Value]) -> Option<&'static serde_json::Value> {
    let args = serde_json::Value::Array(args.iter().map(crate::runtime::value_to_json).collect());
    defaults().answers.get(&format!("{service}.{method}"))?.iter().find(|(a, _)| *a == args).map(|(_, r)| r)
}

/// A JSON value as a value of signature `sig`.
pub fn typed(j: &serde_json::Value, sig: &Sig) -> Result<Value, String> {
    use serde_json::Value as J;
    let bad = || format!("default value {j} does not fit {sig}");
    Ok(match sig {
        Sig::Void => Value::Void,
        Sig::Bool => Value::Bool(j.as_bool().ok_or_else(bad)?),
        Sig::Int(_, true) => Value::Int(j.as_i64().ok_or_else(bad)?),
        Sig::Int(_, false) => Value::UInt(j.as_u64().ok_or_else(bad)?),
        Sig::Float => Value::Float(j.as_f64().ok_or_else(bad)? as f32),
        Sig::Double => Value::Double(j.as_f64().ok_or_else(bad)?),
        Sig::Str => Value::Str(j.as_str().ok_or_else(bad)?.to_string()),
        Sig::List(t) | Sig::VarArgs(t) => Value::List(j.as_array().ok_or_else(bad)?.iter().map(|x| typed(x, t)).collect::<Result<_, _>>()?),
        Sig::Map(k, v) => match j {
            J::Object(o) => Value::Map(o.iter().map(|(a, b)| Ok((typed(&J::String(a.clone()), k)?, typed(b, v)?))).collect::<Result<_, String>>()?),
            J::Array(pairs) => Value::Map(pairs.iter().map(|p| Ok((typed(&p[0], k)?, typed(&p[1], v)?))).collect::<Result<_, String>>()?),
            _ => return Err(bad()),
        },
        Sig::Tuple(fields, _) => {
            let items = j.as_array().ok_or_else(bad)?;
            Value::Tuple(fields.iter().zip(items).map(|(f, x)| typed(x, f)).collect::<Result<_, _>>()?)
        }
        Sig::Dynamic | Sig::Unknown | Sig::Raw => crate::runtime::json_to_value(j),
        Sig::Object => return Err("returns an object".into()),
    })
}

/// ALModule's methods, served by every legacy module (as NAOqi 2.8 declares them on ALLeds).
pub const MODULE_BASE: &[(&str, &str, &str)] = &[
    ("exit", "()", "v"),
    ("pCall", "m", "m"),
    ("version", "()", "s"),
    ("ping", "()", "b"),
    ("getMethodList", "()", "[s]"),
    ("getMethodHelp", "(s)", "m"),
    ("getModuleHelp", "()", "m"),
    ("wait", "(ii)", "b"),
    ("wait", "(i)", "v"),
    ("isRunning", "(i)", "b"),
    ("stop", "(i)", "v"),
    ("getBrokerName", "()", "s"),
    ("getUsage", "(s)", "s"),
];

/// Calls started with `pCall`: `done` turns true when the call returns and closes when stopped.
struct Task {
    abort: tokio::task::AbortHandle,
    done: watch::Receiver<bool>,
}

#[derive(Default)]
pub struct Modules {
    tasks: Mutex<HashMap<i64, Task>>,
    next: AtomicI64,
    logged: Mutex<HashSet<String>>,
    stubs: Mutex<HashSet<String>>,
}

impl Modules {
    /// Whether "Service.method" is a stub rather than simulated.
    pub fn is_stub(&self, method: &str) -> bool {
        self.stubs.lock().unwrap().contains(method)
    }
}

/// A value of signature `sig`, for methods the virtual robot does not simulate.
pub fn default_value(sig: &Sig) -> Result<Value, String> {
    Ok(match sig {
        Sig::Void | Sig::Dynamic | Sig::Unknown => Value::Void,
        Sig::Bool => Value::Bool(false),
        Sig::Int(_, true) => Value::Int(0),
        Sig::Int(_, false) => Value::UInt(0),
        Sig::Float => Value::Float(0.0),
        Sig::Double => Value::Double(0.0),
        Sig::Str => Value::Str(String::new()),
        Sig::Raw => Value::Raw(vec![]),
        Sig::List(_) | Sig::VarArgs(_) => Value::List(vec![]),
        Sig::Map(..) => Value::Map(vec![]),
        Sig::Tuple(fields, _) => Value::Tuple(fields.iter().map(default_value).collect::<Result<_, _>>()?),
        Sig::Object => return Err("returns an object".into()),
    })
}

fn wants_base(service: &str) -> bool {
    match api().get(service) {
        Some(s) => s.methods.iter().any(|m| m.0 == "pCall"),
        None => service.starts_with("AL"),
    }
}

/// Adds what the reference declares and `b` does not.
pub fn complete(mut b: ServiceBuilder, d: &Weak<Robotd>) -> ServiceBuilder {
    let service = b.name().to_string();
    let reference = api().get(&service);
    if wants_base(&service) {
        for &(name, params, ret) in MODULE_BASE {
            if !b.has_method(name, params) {
                b = base_method(b, &service, name, params, ret, d.clone());
            }
        }
    }
    let naoqi_errors = reference.is_some() || service.starts_with("AL");
    if naoqi_errors {
        let svc = service.clone();
        b = b.map_errors(move |method, e| naoqi_error(&svc, method, e));
    }
    let Some(reference) = reference else { return b };
    for (name, params, ret) in &reference.methods {
        if b.has_method(name, params) {
            continue;
        }
        let (Ok(_), Ok(ret_sig)) = (Sig::params(params), Sig::parse(ret)) else { continue };
        let (d, label, svc, method) = (d.clone(), format!("{service}.{name}"), service.clone(), name.clone());
        if let Some(d) = d.upgrade() {
            d.modules.stubs.lock().unwrap().insert(label.clone());
        }
        b = b.method(name, params, ret, move |a| {
            if let Some(r) = answer(&svc, &method, &a) {
                return typed(r, &ret_sig);
            }
            if let Some(d) = d.upgrade() {
                if d.modules.logged.lock().unwrap().insert(label.clone()) {
                    d.log("warning", "naoqi.virtual", &format!("{label} is not simulated on the virtual robot"));
                }
            }
            default_value(&ret_sig).map_err(|e| format!("{label} {e} and is not simulated on the virtual robot"))
        });
    }
    for (name, sig) in &reference.signals {
        if !b.has_signal(name) && Sig::parse(sig).is_ok() {
            b = b.signal(name, sig);
        }
    }
    b
}

/// NAOqi reports errors as `\t<Module>::<method>\n\t<message>`; messages that already name their
/// origin (`ALLeds: fade: …`, `ALTextToSpeechCore::…`) and the few NAOqi raises as plain
/// exceptions (measured) are kept.
pub fn naoqi_error(service: &str, method: &str, e: String) -> String {
    let plain = ["Could not parse '", "Color given as parameter "];
    if e.starts_with('\t') || e.contains("::") || e.starts_with(&format!("{service}:")) || plain.iter().any(|p| e.starts_with(p)) {
        e
    } else {
        format!("\t{service}::{method}\n\t{e}")
    }
}

fn base_method(b: ServiceBuilder, service: &str, name: &str, params: &str, ret: &str, d: Weak<Robotd>) -> ServiceBuilder {
    let service = service.to_string();
    let int = |a: &[Value], i: usize| a.get(i).and_then(Value::as_i64).ok_or_else(|| format!("argument {} must be an integer", i + 1));
    match (name, params) {
        ("pCall", _) => b.method(name, params, ret, move |a| {
            // libqi packs pCall's arguments into one dynamic value: (method, args…).
            let items = match a.as_slice() {
                [one] if !one.plain().items().is_empty() && one.plain().items()[0].as_str().is_some() => one.plain().items().to_vec(),
                _ => a.iter().map(|v| v.plain().clone()).collect(),
            };
            let method = items.first().and_then(Value::as_str).ok_or("pCall expects a method name")?.to_string();
            let d = d.upgrade().ok_or("shutting down")?;
            let id = d.modules.next.fetch_add(1, Ordering::Relaxed) + 1;
            let (tx, done) = watch::channel(false);
            let (node, service, args) = (d.node.clone(), service.clone(), items[1..].to_vec());
            let task = tokio::spawn(async move {
                let _ = node.call(&service, &method, args).await;
                let _ = tx.send(true);
            });
            d.modules.tasks.lock().unwrap().insert(id, Task { abort: task.abort_handle(), done });
            Ok(Value::Int(id))
        }),
        ("wait", _) => {
            let timed = params == "(ii)";
            b.method_async(name, params, ret, move |a| {
                let d = d.clone();
                async move {
                    let id = int(&a, 0)?;
                    let done = d.upgrade().and_then(|d| d.modules.tasks.lock().unwrap().get(&id).map(|t| t.done.clone()));
                    let finished = async move {
                        if let Some(mut done) = done {
                            let _ = done.wait_for(|&x| x).await;
                        }
                    };
                    let ms = if timed { int(&a, 1)? } else { 0 };
                    let ok = if ms > 0 { tokio::time::timeout(std::time::Duration::from_millis(ms as u64), finished).await.is_ok() } else { finished.await; true };
                    Ok(if timed { Value::Bool(ok) } else { Value::Void })
                }
            })
        }
        ("isRunning", _) => b.method(name, params, ret, move |a| {
            let id = int(&a, 0)?;
            let d = d.upgrade().ok_or("shutting down")?;
            let running = d.modules.tasks.lock().unwrap().get(&id).is_some_and(|t| !*t.done.borrow() && t.done.has_changed().is_ok());
            Ok(Value::Bool(running))
        }),
        ("stop", _) => b.method(name, params, ret, move |a| {
            let id = int(&a, 0)?;
            if let Some(t) = d.upgrade().and_then(|d| d.modules.tasks.lock().unwrap().remove(&id)) {
                t.abort.abort();
            }
            Ok(Value::Void)
        }),
        ("getMethodList", _) => b.method(name, params, ret, move |_| {
            let d = d.upgrade().ok_or("shutting down")?;
            let mut names: Vec<String> = d.node.meta(&service).map(|m| m.methods.values().filter(|m| m.uid >= 100).map(|m| m.name.clone()).collect()).unwrap_or_default();
            names.dedup();
            Ok(Value::List(names.into_iter().map(Value::Str).collect()))
        }),
        ("version", _) => b.method(name, params, ret, move |_| Ok(answer(&service, "version", &[]).and_then(|v| v.as_str()).unwrap_or(MODULE_VERSION).into())),
        ("ping", _) => b.method(name, params, ret, |_| Ok(Value::Bool(true))),
        ("getBrokerName", _) => b.method(name, params, ret, |_| Ok("ALModuleCore::getBrokerName not implemented".into())),
        ("getUsage", _) => b.method(name, params, ret, |_| Ok("".into())),
        _ => b.method(name, params, ret, |_| Ok(Value::Void)), // exit, getMethodHelp, getModuleHelp
    }
}

/// Registers the reference services the virtual robot does not simulate at all.
pub fn register_missing(d: &Arc<Robotd>) {
    for name in api().keys() {
        if !d.node.has_service(name) {
            d.node.register(ServiceBuilder::new(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reference_signature_parses() {
        let mut bad = vec![];
        for (service, s) in api() {
            for (name, params, ret) in &s.methods {
                if Sig::params(params).is_err() || Sig::parse(ret).is_err() {
                    bad.push(format!("{service}.{name} {params} -> {ret}"));
                }
            }
            for (name, sig) in &s.signals {
                if Sig::parse(sig).is_err() {
                    bad.push(format!("{service}.{name} signal {sig}"));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }
}
