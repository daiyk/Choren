//! Dynamic values and their binary encoding (little-endian, u32 length prefixes; a dynamic `m`
//! value is its signature string followed by the value).

use crate::object::{Object, Remote};
use crate::sig::Sig;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Void,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f32),
    Double(f64),
    Str(String),
    Raw(Vec<u8>),
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Tuple(Vec<Value>),
    /// A value transmitted with its own signature (`m`).
    Dynamic(Box<Value>),
    /// An object reference (`o`).
    Object(Object),
}

impl Value {
    /// The natural signature of a value, used when it travels as `m`.
    pub fn signature(&self) -> Sig {
        match self {
            Value::Void => Sig::Void,
            Value::Bool(_) => Sig::Bool,
            Value::Int(_) => Sig::Int(4, true),
            Value::UInt(_) => Sig::Int(4, false),
            Value::Float(_) => Sig::Float,
            Value::Double(_) => Sig::Double,
            Value::Str(_) => Sig::Str,
            Value::Raw(_) => Sig::Raw,
            Value::Dynamic(_) => Sig::Dynamic,
            Value::Object(..) => Sig::Object,
            Value::Tuple(v) => Sig::tuple(v.iter().map(Value::signature).collect()),
            Value::List(v) => Sig::List(Box::new(common(v.iter().map(Value::signature)))),
            Value::Map(v) => Sig::Map(Box::new(common(v.iter().map(|p| p.0.signature()))), Box::new(common(v.iter().map(|p| p.1.signature())))),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self.plain() {
            Value::Int(i) => Some(*i as f64),
            Value::UInt(i) => Some(*i as f64),
            Value::Float(f) => Some(*f as f64),
            Value::Double(f) => Some(*f),
            Value::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self.plain() {
            Value::Int(i) => Some(*i),
            Value::UInt(i) => Some(*i as i64),
            Value::Bool(b) => Some(*b as i64),
            Value::Float(_) | Value::Double(_) => self.as_f64().map(|f| f as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self.plain() {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.plain() {
            Value::Bool(b) => Some(*b),
            v => v.as_i64().map(|i| i != 0),
        }
    }

    /// Elements of a list or tuple; a scalar counts as a one-element list (NAOqi accepts both).
    pub fn items(&self) -> Vec<Value> {
        match self.plain() {
            Value::List(v) | Value::Tuple(v) => v.clone(),
            Value::Void => vec![],
            v => vec![v.clone()],
        }
    }

    /// Strips `Dynamic` wrappers.
    pub fn plain(&self) -> &Value {
        match self {
            Value::Dynamic(v) => v.plain(),
            v => v,
        }
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    pub fn floats(v: impl IntoIterator<Item = f64>) -> Value {
        Value::List(v.into_iter().map(|f| Value::Float(f as f32)).collect())
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.into())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}
impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f as f32)
    }
}
impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i as i64)
    }
}

/// Signature shared by all items, or `m` when they differ (or the list is empty).
fn common(mut sigs: impl Iterator<Item = Sig>) -> Sig {
    let Some(first) = sigs.next() else { return Sig::Dynamic };
    if sigs.all(|s| s == first) { first } else { Sig::Dynamic }
}

pub fn encode(v: &Value, sig: &Sig, out: &mut Vec<u8>) -> Result<(), String> {
    encode_for(v, sig, out, 0)
}

/// Encodes for a message to or from `service`, the host id libqi writes for objects sent along.
pub fn encode_for(v: &Value, sig: &Sig, out: &mut Vec<u8>, service: u32) -> Result<(), String> {
    let encode = |v: &Value, sig: &Sig, out: &mut Vec<u8>| encode_for(v, sig, out, service);
    let v = if matches!(sig, Sig::Dynamic) { v } else { v.plain() };
    let mismatch = || format!("cannot encode {v:?} as {sig}");
    match sig {
        Sig::Void => {}
        Sig::Bool => out.push(v.as_bool().ok_or_else(mismatch)? as u8),
        Sig::Int(w, _) => {
            let i = v.as_i64().ok_or_else(mismatch)?;
            out.extend_from_slice(&i.to_le_bytes()[..*w as usize]);
        }
        Sig::Float => out.extend_from_slice(&(v.as_f64().ok_or_else(mismatch)? as f32).to_le_bytes()),
        Sig::Double => out.extend_from_slice(&v.as_f64().ok_or_else(mismatch)?.to_le_bytes()),
        Sig::Str => put_bytes(out, v.as_str().ok_or_else(mismatch)?.as_bytes()),
        Sig::Raw => match v {
            Value::Raw(b) => put_bytes(out, b),
            Value::Str(s) => put_bytes(out, s.as_bytes()),
            _ => return Err(mismatch()),
        },
        Sig::Dynamic => {
            let inner = match v {
                Value::Dynamic(i) => i.as_ref(),
                other => other,
            };
            let s = inner.signature();
            put_bytes(out, s.to_string().as_bytes());
            encode(inner, &s, out)?;
        }
        // MetaObject, host service and object id (no MetaObjectCache/ObjectPtrUID capability).
        Sig::Object => match v {
            Value::Object(Object::Local(l)) => {
                l.meta.encode(out)?;
                out.extend_from_slice(&service.to_le_bytes());
                out.extend_from_slice(&l.id.to_le_bytes());
            }
            _ => return Err(mismatch()),
        },
        Sig::Unknown | Sig::VarArgs(_) => return Err(mismatch()),
        Sig::List(t) => {
            let items = v.items();
            out.extend_from_slice(&(items.len() as u32).to_le_bytes());
            for i in &items {
                encode(i, t, out)?;
            }
        }
        Sig::Map(k, vs) => {
            let Value::Map(pairs) = v else { return Err(mismatch()) };
            out.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
            for (a, b) in pairs {
                encode(a, k, out)?;
                encode(b, vs, out)?;
            }
        }
        Sig::Tuple(fields, _) => {
            let items = v.items();
            if items.len() != fields.len() {
                return Err(mismatch());
            }
            for (i, f) in items.iter().zip(fields) {
                encode(i, f, out)?;
            }
        }
    }
    Ok(())
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

pub struct Reader<'a> {
    pub buf: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.buf.len() < n {
            return Err("truncated payload".into());
        }
        let (a, b) = self.buf.split_at(n);
        self.buf = b;
        Ok(a)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }

    pub fn decode(&mut self, sig: &Sig) -> Result<Value, String> {
        Ok(match sig {
            Sig::Void => Value::Void,
            Sig::Bool => Value::Bool(self.take(1)?[0] != 0),
            Sig::Int(w, signed) => {
                let b = self.take(*w as usize)?;
                let mut full = [0u8; 8];
                full[..b.len()].copy_from_slice(b);
                let raw = u64::from_le_bytes(full);
                if *signed {
                    let shift = 64 - 8 * *w as u32;
                    Value::Int(((raw << shift) as i64) >> shift)
                } else {
                    Value::UInt(raw)
                }
            }
            Sig::Float => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            Sig::Double => Value::Double(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            Sig::Str => Value::Str(String::from_utf8_lossy(&self.bytes()?).into_owned()),
            Sig::Raw => Value::Raw(self.bytes()?),
            Sig::Dynamic => {
                let s = Sig::parse(&String::from_utf8_lossy(&self.bytes()?))?;
                Value::Dynamic(Box::new(self.decode(&s)?))
            }
            Sig::Object => {
                // MetaObject, then service and object ids (no capabilities negotiated).
                let meta = Arc::new(crate::meta::MetaObject::decode(self)?);
                let (service, object) = (self.u32()?, self.u32()?);
                Value::Object(Object::Remote(Remote { service, object, meta, conn: None, _release: None }))
            }
            Sig::Unknown | Sig::VarArgs(_) => return Err(format!("cannot decode {sig}")),
            Sig::List(t) => {
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.decode(t)?);
                }
                Value::List(v)
            }
            Sig::Map(k, vs) => {
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push((self.decode(k)?, self.decode(vs)?));
                }
                Value::Map(v)
            }
            Sig::Tuple(fields, _) => Value::Tuple(fields.iter().map(|f| self.decode(f)).collect::<Result<_, _>>()?),
        })
    }
}

pub fn decode(buf: &[u8], sig: &Sig) -> Result<Value, String> {
    Reader { buf }.decode(sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(v: Value, s: &str) -> Value {
        let sig = Sig::parse(s).unwrap();
        let mut out = vec![];
        encode(&v, &sig, &mut out).unwrap();
        decode(&out, &sig).unwrap()
    }

    #[test]
    fn round_trips() {
        assert_eq!(rt(Value::Int(-5), "c"), Value::Int(-5));
        assert_eq!(rt(Value::Int(70000), "i"), Value::Int(70000));
        assert_eq!(rt(Value::UInt(7), "L"), Value::UInt(7));
        assert_eq!(rt("héllo".into(), "s"), "héllo".into());
        let v = Value::List(vec![Value::Float(1.5), Value::Float(-2.0)]);
        assert_eq!(rt(v.clone(), "[f]"), v);
        let m = Value::Map(vec![("a".into(), Value::Dynamic(Box::new(Value::Int(3))))]);
        assert_eq!(rt(m.clone(), "{sm}"), m);
        assert_eq!(rt(Value::Tuple(vec!["x".into(), Value::Bool(true)]), "(sb)"), Value::Tuple(vec!["x".into(), Value::Bool(true)]));
        // dynamic carries its signature: list of mixed values becomes [m]
        let mixed = Value::List(vec!["HeadYaw".into(), Value::Int(1)]);
        let back = rt(Value::Dynamic(Box::new(mixed)), "m");
        assert_eq!(back.plain().items().len(), 2);
        // ints convert to floats where the signature asks for it
        assert_eq!(rt(Value::Int(2), "f"), Value::Float(2.0));
        assert!(decode(&[1, 0], &Sig::parse("I").unwrap()).is_err());
    }

    #[test]
    fn known_bytes() {
        let mut out = vec![];
        encode(&"ab".into(), &Sig::Str, &mut out).unwrap();
        assert_eq!(out, [2, 0, 0, 0, b'a', b'b']);
        let mut out = vec![];
        encode(&Value::Int(1), &Sig::Dynamic, &mut out).unwrap();
        assert_eq!(out, [1, 0, 0, 0, b'i', 1, 0, 0, 0]);
        // An object sent by a client: meta object, host service, then an id from 2^31 (libqi).
        let o = Object::local(crate::ServiceBuilder::new("File"), true);
        let Object::Local(l) = &o else { unreachable!() };
        let mut out = vec![];
        encode_for(&Value::Object(o.clone()), &Sig::Object, &mut out, 7).unwrap();
        assert!(l.id >= 0x8000_0000);
        assert_eq!(out[out.len() - 8..], [&7u32.to_le_bytes()[..], &l.id.to_le_bytes()[..]].concat());
        let Value::Object(Object::Remote(r)) = decode(&out, &Sig::Object).unwrap() else { panic!() };
        assert_eq!((r.service, r.object, r.meta.as_ref()), (7, l.id, &l.meta));
    }
}
