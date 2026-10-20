//! MetaObject: the description of an object's methods, signals and properties.

use crate::sig::Sig;
use crate::value::{encode, Reader, Value};
use std::collections::BTreeMap;

pub const META_OBJECT_SIG: &str = "({I(Issss[(ss)<MetaMethodParameter,name,description>]s)<MetaMethod,uid,returnSignature,name,parametersSignature,description,parameters,returnDescription>}{I(Iss)<MetaSignal,uid,name,signature>}{I(Iss)<MetaProperty,uid,name,signature>}s)<MetaObject,methods,signals,properties,description>";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Method {
    pub uid: u32,
    pub name: String,
    /// Parameters as a tuple signature, e.g. `(sI)`.
    pub params: String,
    pub ret: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signal {
    pub uid: u32,
    pub name: String,
    pub sig: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaObject {
    pub methods: BTreeMap<u32, Method>,
    pub signals: BTreeMap<u32, Signal>,
    pub properties: BTreeMap<u32, Signal>,
    pub description: String,
}

/// Generic actions every bound object answers (libqi `BoundObject`).
pub const REGISTER_EVENT: u32 = 0;
pub const UNREGISTER_EVENT: u32 = 1;
pub const META_OBJECT: u32 = 2;
pub const TERMINATE: u32 = 3;
pub const PROPERTY: u32 = 5;
pub const SET_PROPERTY: u32 = 6;
pub const PROPERTIES: u32 = 7;
pub const REGISTER_EVENT_WITH_SIGNATURE: u32 = 8;

impl MetaObject {
    /// The generic methods present on every object (ids 0–8).
    pub fn with_generic_methods() -> MetaObject {
        let mut m = MetaObject::default();
        for (uid, name, params, ret) in [
            (REGISTER_EVENT, "registerEvent", "(IIL)", "L"),
            (UNREGISTER_EVENT, "unregisterEvent", "(IIL)", "v"),
            (META_OBJECT, "metaObject", "(I)", META_OBJECT_SIG),
            (TERMINATE, "terminate", "(I)", "v"),
            (PROPERTY, "property", "(m)", "m"),
            (SET_PROPERTY, "setProperty", "(mm)", "v"),
            (PROPERTIES, "properties", "()", "[s]"),
            (REGISTER_EVENT_WITH_SIGNATURE, "registerEventWithSignature", "(IILs)", "L"),
        ] {
            m.methods.insert(uid, Method { uid, name: name.into(), params: params.into(), ret: ret.into() });
        }
        m
    }

    pub fn to_value(&self) -> Value {
        let s = |x: &str| Value::Str(x.into());
        Value::Tuple(vec![
            Value::Map(
                self.methods
                    .values()
                    .map(|m| {
                        (
                            Value::UInt(m.uid as u64),
                            Value::Tuple(vec![Value::UInt(m.uid as u64), s(&m.ret), s(&m.name), s(&m.params), s(""), Value::List(vec![]), s("")]),
                        )
                    })
                    .collect(),
            ),
            Value::Map(self.signals.values().map(|g| (Value::UInt(g.uid as u64), Value::Tuple(vec![Value::UInt(g.uid as u64), s(&g.name), s(&g.sig)]))).collect()),
            Value::Map(self.properties.values().map(|g| (Value::UInt(g.uid as u64), Value::Tuple(vec![Value::UInt(g.uid as u64), s(&g.name), s(&g.sig)]))).collect()),
            s(&self.description),
        ])
    }

    pub fn from_value(v: &Value) -> Result<MetaObject, String> {
        let bad = || "malformed MetaObject".to_string();
        let f = v.items();
        let [methods, signals, props, desc] = f.as_slice() else { return Err(bad()) };
        let mut m = MetaObject { description: desc.as_str().unwrap_or("").into(), ..Default::default() };
        let pairs = |x: &Value| match x {
            Value::Map(p) => Ok(p.clone()),
            _ => Err(bad()),
        };
        for (_, t) in pairs(methods)? {
            let t = t.items();
            let get = |i: usize| t.get(i).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let uid = t.first().and_then(Value::as_i64).ok_or_else(bad)? as u32;
            m.methods.insert(uid, Method { uid, ret: get(1), name: get(2), params: get(3) });
        }
        for (target, src) in [(&mut m.signals, signals), (&mut m.properties, props)] {
            for (_, t) in pairs(src)? {
                let t = t.items();
                let uid = t.first().and_then(Value::as_i64).ok_or_else(bad)? as u32;
                let get = |i: usize| t.get(i).and_then(|x| x.as_str()).unwrap_or("").to_string();
                target.insert(uid, Signal { uid, name: get(1), sig: get(2) });
            }
        }
        Ok(m)
    }

    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), String> {
        encode(&self.to_value(), &Sig::parse(META_OBJECT_SIG)?, out)
    }

    pub fn decode(r: &mut Reader) -> Result<MetaObject, String> {
        MetaObject::from_value(&r.decode(&Sig::parse(META_OBJECT_SIG)?)?)
    }

    /// Methods with this name, in uid order (overloads).
    pub fn methods_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Method> + 'a {
        self.methods.values().filter(move |m| m.name == name)
    }

    pub fn signal_named(&self, name: &str) -> Option<&Signal> {
        self.signals.values().find(|s| s.name == name)
    }

    /// Picks the overload whose parameter count matches (NAOqi overloads differ by arity),
    /// preferring one whose parameter types accept the values.
    pub fn resolve<'a>(&'a self, name: &'a str, args: &[Value]) -> Option<&'a Method> {
        let arity: Vec<_> = self.methods_named(name).filter(|m| Sig::params(&m.params).is_ok_and(|p| p.len() == args.len())).collect();
        arity.iter().copied().find(|m| Sig::params(&m.params).is_ok_and(|p| p.iter().zip(args).all(|(s, v)| accepts(s, v)))).or(arity.first().copied())
    }
}

/// Whether a value can be encoded with a signature without loss of meaning.
pub fn accepts(sig: &Sig, v: &Value) -> bool {
    let v = v.plain();
    match sig {
        Sig::Dynamic => true,
        Sig::Str | Sig::Raw => matches!(v, Value::Str(_) | Value::Raw(_)),
        Sig::Bool | Sig::Int(..) | Sig::Float | Sig::Double => v.as_f64().is_some(),
        Sig::List(t) => matches!(v, Value::List(_) | Value::Tuple(_)) && v.items().iter().all(|i| accepts(t, i)),
        Sig::Map(..) => matches!(v, Value::Map(_)),
        Sig::Tuple(f, _) => matches!(v, Value::Tuple(_) | Value::List(_)) && v.items().len() == f.len(),
        Sig::Object => matches!(v, Value::Object(_)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meta_object_round_trip_and_overloads() {
        let mut m = MetaObject::with_generic_methods();
        m.methods.insert(100, Method { uid: 100, name: "fadeRGB".into(), params: "(sIf)".into(), ret: "v".into() });
        m.methods.insert(101, Method { uid: 101, name: "fadeRGB".into(), params: "(ssf)".into(), ret: "v".into() });
        m.methods.insert(102, Method { uid: 102, name: "fadeRGB".into(), params: "(sfffd)".into(), ret: "v".into() });
        m.signals.insert(103, Signal { uid: 103, name: "changed".into(), sig: "(m)".into() });
        let mut out = vec![];
        m.encode(&mut out).unwrap();
        let back = MetaObject::decode(&mut Reader { buf: &out }).unwrap();
        assert_eq!(back, m);
        assert_eq!(m.resolve("fadeRGB", &["FaceLeds".into(), "red".into(), 1.0.into()]).unwrap().uid, 101);
        assert_eq!(m.resolve("fadeRGB", &["FaceLeds".into(), Value::Int(255), 1.0.into()]).unwrap().uid, 100);
    }
}
