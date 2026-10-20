//! qi objects inside values (`o`) and the per-connection state both sides share: calls this side
//! sent, and the objects it serves to the peer. libqi numbers objects a client sends from 2^31 and
//! the ones a server returns from 3 (`RemoteObject::_nextId`, `ServiceBoundObject::_nextId`).

use crate::message::{Kind, Message};
use crate::meta::{self, MetaObject};
use crate::node::{error_payload, Handler, ServiceBuilder};
use crate::sig::Sig;
use crate::value::{decode, encode_for, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{mpsc, oneshot};

static CLIENT_IDS: AtomicU32 = AtomicU32::new(0x8000_0000);
static SERVER_IDS: AtomicU32 = AtomicU32::new(3);

/// An object reference carried by a value.
#[derive(Clone)]
pub enum Object {
    /// Served by this side; the peer calls it back over the connection the value travels on.
    Local(Arc<Local>),
    /// Served by the peer.
    Remote(Remote),
}

impl Object {
    /// An object this side serves, built like a service (methods only). `client` selects libqi's
    /// id range for objects sent by a client.
    pub fn local(b: ServiceBuilder, client: bool) -> Object {
        let id = if client { CLIENT_IDS.fetch_add(1, Ordering::Relaxed) } else { SERVER_IDS.fetch_add(1, Ordering::Relaxed) };
        Object::Local(Arc::new(Local { id, meta: b.meta, handlers: b.handlers, subscribers: Default::default() }))
    }

    pub fn remote(&self) -> Option<&Remote> {
        match self {
            Object::Remote(r) => Some(r),
            Object::Local(_) => None,
        }
    }
}

impl std::fmt::Debug for Object {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Object::Local(l) => write!(f, "Local({})", l.id),
            Object::Remote(r) => write!(f, "Remote({}, {})", r.service, r.object),
        }
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Object) -> bool {
        match (self, other) {
            (Object::Local(a), Object::Local(b)) => a.id == b.id,
            (Object::Remote(a), Object::Remote(b)) => (a.service, a.object) == (b.service, b.object),
            _ => false,
        }
    }
}

pub struct Local {
    pub id: u32,
    pub meta: MetaObject,
    handlers: HashMap<u32, Handler>,
    /// Peers subscribed to a signal: (connection, host service, signal uid, link).
    subscribers: Mutex<Vec<(Weak<Conn>, u32, u32, u64)>>,
}

impl Local {
    /// Raises one of the object's signals for every peer that subscribed to it.
    pub fn emit(&self, signal: &str, args: Vec<Value>) {
        let Some(sig) = self.meta.signal_named(signal) else { return };
        let mut payload = vec![];
        if encode_for(&Value::Tuple(args), &Sig::parse(&sig.sig).unwrap_or(Sig::Void), &mut payload, 0).is_err() {
            return;
        }
        self.subscribers.lock().unwrap().retain(|(conn, service, uid, _)| match conn.upgrade() {
            Some(c) => *uid != sig.uid || c.send(Message::new(0, Kind::Event, *service, self.id, sig.uid, payload.clone())),
            None => false,
        });
    }
}

#[derive(Clone)]
pub struct Remote {
    pub service: u32,
    pub object: u32,
    pub meta: Arc<MetaObject>,
    /// The connection the reference arrived on; `None` until it is bound to one.
    pub(crate) conn: Option<Arc<Conn>>,
    /// Shared by the copies of a bound reference; the last one releases the peer's object.
    pub(crate) _release: Option<Arc<Release>>,
}

/// libqi's `onProxyLost`: when the last copy of a reference goes, `terminate::(I)` is posted to the
/// object itself with the host service id as argument, and the peer drops the object (ev).
pub(crate) struct Release {
    conn: Weak<Conn>,
    service: u32,
    object: u32,
}

impl Drop for Release {
    fn drop(&mut self) {
        if let Some(c) = self.conn.upgrade() {
            let mut payload = vec![];
            if crate::value::encode(&Value::Tuple(vec![Value::UInt(self.service as u64)]), &Sig::parse("(I)").unwrap(), &mut payload).is_ok() {
                c.send(Message::new(0, Kind::Post, self.service, self.object, meta::TERMINATE, payload));
            }
        }
    }
}

impl Remote {
    /// Calls a method of the peer's object by name (overloads resolved by the meta object).
    pub async fn call(&self, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let conn = self.conn.as_ref().ok_or("The object is not connected.")?;
        let m = self.meta.resolve(method, &args).ok_or_else(|| format!("The object has no method {method} taking {} arguments", args.len()))?.clone();
        let payload = conn.encode(&Value::Tuple(args), &m.params, self.service)?;
        let reply = conn.call(self.service, self.object, m.uid, payload).await?;
        Ok(conn.bind(decode(&reply, &Sig::parse(&m.ret)?)?))
    }

    /// Subscribes to one of the object's signals.
    pub async fn subscribe(&self, signal: &str) -> Result<mpsc::UnboundedReceiver<Value>, String> {
        let conn = self.conn.as_ref().ok_or("The object is not connected.")?;
        conn.subscribe(self.service, self.object, &self.meta, signal).await
    }
}

type Pending = Mutex<Option<HashMap<u32, oneshot::Sender<Message>>>>;
/// Signal subscribers by (service, object, signal uid), with their link ids.
type Signals = Mutex<HashMap<(u32, u32, u32), Vec<(u64, mpsc::UnboundedSender<Vec<u8>>)>>>;

/// One connection's calls in flight, signal subscriptions and hosted objects (client or server).
pub(crate) struct Conn {
    tx: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU32,
    signals: Signals,
    next_link: AtomicU64,
    objects: Mutex<HashMap<u32, Arc<Local>>>,
}

impl Conn {
    pub fn new(tx: mpsc::UnboundedSender<Message>) -> Arc<Conn> {
        Arc::new(Conn { tx, pending: Mutex::new(Some(HashMap::new())), next_id: AtomicU32::new(1), signals: Default::default(), next_link: AtomicU64::new(1), objects: Default::default() })
    }

    pub fn send(&self, m: Message) -> bool {
        self.tx.send(m).is_ok()
    }

    /// Sends a call and waits for its reply payload.
    pub async fn call(&self, service: u32, object: u32, action: u32, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().as_mut().ok_or("connection closed")?.insert(id, tx);
        if !self.send(Message::new(id, Kind::Call, service, object, action, payload)) {
            return Err("connection closed".into());
        }
        let reply = rx.await.map_err(|_| "connection closed".to_string())?;
        match reply.kind {
            Kind::Reply => Ok(reply.payload),
            Kind::Error => {
                let v = decode(&reply.payload, &Sig::Dynamic).unwrap_or(Value::Str("unknown error".into()));
                Err(v.plain().as_str().map(str::to_string).unwrap_or_else(|| format!("{v:?}")))
            }
            _ => Err("call canceled".into()),
        }
    }

    /// Hands a reply, error or cancellation to the call waiting for it.
    pub fn answer(&self, msg: Message) {
        if let Some(waiter) = self.pending.lock().unwrap().as_mut().and_then(|m| m.remove(&msg.id)) {
            let _ = waiter.send(msg);
        }
    }

    /// Subscribes to a signal of an object of the peer; values arrive decoded as the signal's
    /// argument tuple.
    pub async fn subscribe(self: &Arc<Self>, service: u32, object: u32, meta: &MetaObject, signal: &str) -> Result<mpsc::UnboundedReceiver<Value>, String> {
        let sig = meta.signal_named(signal).ok_or_else(|| format!("no signal {signal}"))?.clone();
        let link = self.next_link.fetch_add(1, Ordering::Relaxed);
        let (raw_tx, mut raw_rx) = mpsc::unbounded_channel();
        self.signals.lock().unwrap().entry((service, object, sig.uid)).or_default().push((link, raw_tx));
        let payload = self.encode(&Value::Tuple(vec![Value::UInt(object as u64), Value::UInt(sig.uid as u64), Value::UInt(link)]), "(IIL)", service)?;
        self.call(service, object, meta::REGISTER_EVENT, payload).await?;
        let (tx, rx) = mpsc::unbounded_channel();
        // Weak: a subscription must not keep a closed connection (and its socket) alive.
        let (s, conn, uid) = (Sig::parse(&sig.sig)?, Arc::downgrade(self), sig.uid);
        tokio::spawn(async move {
            while let Some(bytes) = raw_rx.recv().await {
                let (Ok(v), Some(c)) = (decode(&bytes, &s), conn.upgrade()) else { continue };
                if tx.send(c.bind(v)).is_err() {
                    // The receiver is gone: stop the peer's events too.
                    c.signals.lock().unwrap().entry((service, object, uid)).or_default().retain(|x| x.0 != link);
                    if let Ok(p) = c.encode(&Value::Tuple(vec![Value::UInt(object as u64), Value::UInt(uid as u64), Value::UInt(link)]), "(IIL)", service) {
                        c.send(Message::new(0, Kind::Post, service, object, meta::UNREGISTER_EVENT, p));
                    }
                    break;
                }
            }
        });
        Ok(rx)
    }

    /// Hands a signal to its subscribers.
    pub fn event(&self, msg: &Message) {
        if let Some(list) = self.signals.lock().unwrap().get_mut(&(msg.service, msg.object, msg.action)) {
            list.retain(|(_, s)| s.send(msg.payload.clone()).is_ok());
        }
    }

    /// Fails every outstanding and future call; hosted objects and subscriptions are dropped.
    pub fn close(&self) {
        self.pending.lock().unwrap().take();
        self.objects.lock().unwrap().clear();
        self.signals.lock().unwrap().clear();
    }

    /// Encodes `v` for a call to (or a reply from) `service`, hosting its local objects here.
    pub fn encode(&self, v: &Value, sig: &str, service: u32) -> Result<Vec<u8>, String> {
        let mut out = vec![];
        encode_for(v, &Sig::parse(sig)?, &mut out, service)?;
        visit(v, &mut |o| {
            if let Object::Local(l) = o {
                self.objects.lock().unwrap().insert(l.id, l.clone());
            }
        });
        Ok(out)
    }

    /// Binds the peer's objects in a decoded value to this connection.
    pub fn bind(self: &Arc<Self>, v: Value) -> Value {
        match v {
            Value::Object(Object::Remote(r)) => {
                let release = Some(Arc::new(Release { conn: Arc::downgrade(self), service: r.service, object: r.object }));
                Value::Object(Object::Remote(Remote { conn: Some(self.clone()), _release: release, ..r }))
            }
            Value::List(l) => Value::List(l.into_iter().map(|x| self.bind(x)).collect()),
            Value::Tuple(l) => Value::Tuple(l.into_iter().map(|x| self.bind(x)).collect()),
            Value::Map(m) => Value::Map(m.into_iter().map(|(k, x)| (self.bind(k), self.bind(x))).collect()),
            Value::Dynamic(x) => Value::Dynamic(Box::new(self.bind(*x))),
            v => v,
        }
    }

    /// Serves a call to an object hosted here (or `terminate`, which releases it: libqi's
    /// `ServiceBoundObject::terminate` drops its own object and ignores the argument). Returns
    /// false when the message is not for this connection's objects.
    pub fn serve(self: &Arc<Self>, msg: Message) -> bool {
        if msg.action == meta::TERMINATE {
            self.objects.lock().unwrap().remove(&msg.object);
            self.reply(&msg, Ok(vec![]));
            return true;
        }
        let Some(obj) = self.objects.lock().unwrap().get(&msg.object).cloned() else { return false };
        let subscription = matches!(msg.action, meta::REGISTER_EVENT | meta::REGISTER_EVENT_WITH_SIGNATURE | meta::UNREGISTER_EVENT);
        if let Some(a) = subscription.then(|| decode(&msg.payload, &Sig::parse("(IIL)").unwrap()).ok()).flatten().map(|v| v.items()) {
            let (uid, link) = (a[1].as_i64().unwrap_or(0) as u32, a[2].as_i64().unwrap_or(0) as u64);
            if msg.action == meta::UNREGISTER_EVENT {
                obj.subscribers.lock().unwrap().retain(|s| s.3 != link);
            } else {
                obj.subscribers.lock().unwrap().push((Arc::downgrade(self), msg.service, uid, link));
            }
        }
        let conn = self.clone();
        tokio::spawn(async move {
            let r = conn.invoke(&obj.meta, &obj.handlers, msg.service, msg.action, &msg.payload).await;
            conn.reply(&msg, r);
        });
        true
    }

    pub fn reply(&self, msg: &Message, r: Result<Vec<u8>, String>) {
        if msg.kind == Kind::Call {
            let (kind, payload) = match r {
                Ok(p) => (Kind::Reply, p),
                Err(e) => (Kind::Error, error_payload(&e)),
            };
            self.send(Message::new(msg.id, kind, msg.service, msg.object, msg.action, payload));
        }
    }

    /// Runs a method of an object or service: generic actions, else its handler.
    pub async fn invoke(self: &Arc<Self>, meta: &MetaObject, handlers: &HashMap<u32, Handler>, service: u32, action: u32, payload: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = vec![];
        match action {
            meta::META_OBJECT => meta.encode(&mut out)?,
            meta::REGISTER_EVENT | meta::REGISTER_EVENT_WITH_SIGNATURE => {
                let a = decode(payload, &Sig::parse(if action == meta::REGISTER_EVENT { "(IIL)" } else { "(IILs)" })?)?.items();
                crate::value::encode(a.get(2).unwrap_or(&Value::UInt(0)), &Sig::Int(8, false), &mut out)?;
            }
            meta::UNREGISTER_EVENT => {}
            meta::PROPERTIES => crate::value::encode(&Value::List(vec![]), &Sig::parse("[s]")?, &mut out)?,
            _ => {
                let m = meta.methods.get(&action).ok_or_else(|| format!("no method {action}"))?;
                let args = decode(payload, &Sig::parse(&m.params)?)?.items().into_iter().map(|a| self.bind(a)).collect();
                let h = handlers.get(&action).cloned().ok_or("no handler")?;
                let v = h(args).await?;
                out = self.encode(&v, &m.ret, service)?;
            }
        }
        Ok(out)
    }
}

/// Calls `f` on every object in a value.
fn visit(v: &Value, f: &mut impl FnMut(&Object)) {
    match v {
        Value::Object(o) => f(o),
        Value::List(l) | Value::Tuple(l) => l.iter().for_each(|x| visit(x, f)),
        Value::Map(m) => m.iter().for_each(|(k, x)| {
            visit(k, f);
            visit(x, f);
        }),
        Value::Dynamic(x) => visit(x, f),
        _ => {}
    }
}

/// libqi's `qi::File` over bytes held in memory, with the `ProgressNotifier` its
/// `operationProgress()` returns: what `PackageManager.install(o)`/`_install(oss)` copy from.
pub fn file(bytes: Vec<u8>) -> Object {
    let bytes = Arc::new(bytes);
    let pos = Arc::new(Mutex::new(0usize));
    let read_at = {
        let bytes = bytes.clone();
        move |offset: i64, count: i64| -> Value {
            let start = (offset.max(0) as usize).min(bytes.len());
            let end = start.saturating_add(count.max(0) as usize).min(bytes.len());
            Value::Raw(bytes[start..end].to_vec())
        }
    };
    let notifier = Object::local(
        ["notifyRunning", "notifyFinished", "notifyCanceled", "notifyFailed", "waitForFinished", "reset", "_reset", "_notifyRunning", "_notifyFinished", "_notifyCanceled", "_notifyFailed"]
            .into_iter()
            .fold(ServiceBuilder::new("ProgressNotifier"), |b, name| b.method(name, "()", "v", |_| Ok(Value::Void)))
            .method("notifyProgressed", "(d)", "v", |_| Ok(Value::Void))
            .method("_notifyProgressed", "(d)", "v", |_| Ok(Value::Void))
            .method("isRunning", "()", "b", |_| Ok(Value::Bool(false))),
        true,
    );
    let size = bytes.len() as i64;
    let mut b = ServiceBuilder::new("File")
        .method("size", "()", "l", move |_| Ok(Value::Int(size)))
        .method("isOpen", "()", "b", |_| Ok(Value::Bool(true)))
        .method("isRemote", "()", "b", |_| Ok(Value::Bool(false)))
        .method("operationProgress", "()", "o", move |_| Ok(Value::Object(notifier.clone())));
    // libqi registers `read`, `seek` and `close` twice, the second time with a `_` prefix.
    for prefix in ["", "_"] {
        let (r1, r2, p1, p2) = (read_at.clone(), read_at.clone(), pos.clone(), pos.clone());
        b = b
            .method(&format!("{prefix}read"), "(ll)", "r", move |a| Ok(r1(a[0].as_i64().unwrap_or(0), a[1].as_i64().unwrap_or(0))))
            .method(&format!("{prefix}read"), "(l)", "r", move |a| {
                let mut p = p1.lock().unwrap();
                let v = r2(*p as i64, a[0].as_i64().unwrap_or(0));
                if let Value::Raw(chunk) = &v {
                    *p += chunk.len();
                }
                Ok(v)
            })
            .method(&format!("{prefix}seek"), "(l)", "b", move |a| {
                let to = a[0].as_i64().unwrap_or(-1);
                let ok = (0..=size).contains(&to);
                if ok {
                    *p2.lock().unwrap() = to as usize;
                }
                Ok(Value::Bool(ok))
            })
            .method(&format!("{prefix}close"), "()", "v", |_| Ok(Value::Void));
    }
    Object::local(b, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two connections wired back to back through their outgoing channels.
    fn pair() -> (Arc<Conn>, mpsc::UnboundedReceiver<Message>, Arc<Conn>, mpsc::UnboundedReceiver<Message>) {
        let (ta, ra) = mpsc::unbounded_channel();
        let (tb, rb) = mpsc::unbounded_channel();
        (Conn::new(ta), ra, Conn::new(tb), rb)
    }

    #[tokio::test]
    async fn a_dropped_reference_terminates_the_hosted_object() {
        let (a, _ra, b, mut rb) = pair();
        let obj = Object::local(ServiceBuilder::new("File"), true);
        let Object::Local(l) = &obj else { unreachable!() };
        let bytes = a.encode(&Value::Object(obj.clone()), "o", 7).unwrap();
        assert!(a.objects.lock().unwrap().contains_key(&l.id));
        let remote = b.bind(decode(&bytes, &Sig::Object).unwrap());
        let copy = remote.clone();
        drop(remote);
        assert!(rb.try_recv().is_err(), "a copy still holds the reference");
        drop(copy);
        let msg = rb.try_recv().unwrap();
        assert_eq!((msg.kind, msg.service, msg.object, msg.action), (Kind::Post, 7, l.id, meta::TERMINATE));
        assert!(a.serve(msg));
        assert!(a.objects.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn subscriptions_do_not_keep_the_connection_and_unregister_when_dropped() {
        let (a, mut ra, _b, _rb) = pair();
        let mut meta = MetaObject::with_generic_methods();
        meta.signals.insert(100, crate::meta::Signal { uid: 100, name: "changed".into(), sig: "(i)".into() });
        let a2 = a.clone();
        let answer = tokio::spawn(async move {
            let m = ra.recv().await.unwrap();
            a2.answer(Message::new(m.id, Kind::Reply, m.service, m.object, m.action, 1u64.to_le_bytes().to_vec()));
            ra
        });
        let rx = a.subscribe(5, 1, &meta, "changed").await.unwrap();
        let mut ra = answer.await.unwrap();
        assert_eq!(Arc::strong_count(&a), 1, "the forwarder holds no strong reference");
        drop(rx);
        let mut payload = vec![];
        crate::value::encode(&Value::Tuple(vec![Value::Int(3)]), &Sig::parse("(i)").unwrap(), &mut payload).unwrap();
        a.event(&Message::new(0, Kind::Event, 5, 1, 100, payload));
        let m = tokio::time::timeout(std::time::Duration::from_secs(1), ra.recv()).await.unwrap().unwrap();
        assert_eq!((m.kind, m.service, m.object, m.action), (Kind::Post, 5, 1, meta::UNREGISTER_EVENT));
        a.close();
        assert!(a.signals.lock().unwrap().is_empty());
    }
}
