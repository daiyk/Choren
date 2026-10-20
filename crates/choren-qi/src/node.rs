//! Server side: a service registry with a ServiceDirectory (service 1), reachable in-process
//! (`Node::call`) and over TCP (`Node::listen`). Every remote call runs in its own task.

use crate::message::{Kind, Message, HEADER};
use crate::meta::{self, MetaObject, Method, Signal};
use crate::object::Conn;
use crate::sig::Sig;
use crate::value::{decode, encode, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub type Handler = Arc<dyn Fn(Vec<Value>) -> BoxFuture<Result<Value, String>> + Send + Sync>;
type Listener = Arc<dyn Fn(Vec<Value>) + Send + Sync>;
pub type Completer = Arc<dyn Fn(ServiceBuilder) -> ServiceBuilder + Send + Sync>;

pub const SERVICE_DIRECTORY: u32 = 1;
const MAIN_OBJECT: u32 = 1;
const METHOD_STATISTICS_SIG: &str = "{I(I(fff)<MinMaxSum,minValue,maxValue,cumulatedValue>(fff)<MinMaxSum,minValue,maxValue,cumulatedValue>(fff)<MinMaxSum,minValue,maxValue,cumulatedValue>)<MethodStatistics,count,wall,user,system>}";
const EVENT_TRACE_SIG: &str = "((IiIm(ll)<timeval,tv_sec,tv_usec>llII)<EventTrace,id,kind,slotId,arguments,timestamp,userUsTime,systemUsTime,callerContext,calleeContext>)";

const REMOTE_SERVICES: &str = "Services from other processes cannot be registered on Choren's virtual robot";

/// NAOqi 2.8's ServiceInfo (libqi added `objectUid` later; 2.8 clients reject the 7-field form).
pub const SERVICE_INFO_SIG: &str = "(sIsI[s]s)<ServiceInfo,name,serviceId,machineId,processId,endpoints,sessionId>";

/// Declares a service: methods get uids from 100, signals follow.
pub struct ServiceBuilder {
    name: String,
    pub(crate) meta: MetaObject,
    pub(crate) handlers: HashMap<u32, Handler>,
    next: u32,
}

impl ServiceBuilder {
    pub fn new(name: &str) -> ServiceBuilder {
        let mut b = ServiceBuilder { name: name.into(), meta: MetaObject::with_generic_methods(), handlers: HashMap::new(), next: 80 };
        // libqi's per-object statistics and tracing (uids 80–86 on every NAOqi object). Not recorded.
        b = b
            .method("isStatsEnabled", "()", "b", |_| Ok(Value::Bool(false)))
            .method("enableStats", "(b)", "v", |_| Ok(Value::Void))
            .method("stats", "()", METHOD_STATISTICS_SIG, |_| Ok(Value::Map(vec![])))
            .method("clearStats", "()", "v", |_| Ok(Value::Void))
            .method("isTraceEnabled", "()", "b", |_| Ok(Value::Bool(false)))
            .method("enableTrace", "(b)", "v", |_| Ok(Value::Void))
            .signal("traceObject", EVENT_TRACE_SIG);
        b.next = 100;
        b
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// True when a method with this name and parameter signature is already declared.
    pub fn has_method(&self, name: &str, params: &str) -> bool {
        self.meta.methods.values().any(|m| m.name == name && m.params == params)
    }

    /// Rewrites the errors every method of this service returns, given the method name.
    pub fn map_errors(mut self, f: impl Fn(&str, String) -> String + Send + Sync + 'static) -> Self {
        let f = Arc::new(f);
        for (uid, h) in self.handlers.iter_mut() {
            let (inner, f, name) = (h.clone(), f.clone(), self.meta.methods.get(uid).map(|m| m.name.clone()).unwrap_or_default());
            *h = Arc::new(move |a| {
                let (fut, f, name) = (inner(a), f.clone(), name.clone());
                Box::pin(async move { fut.await.map_err(|e| f(&name, e)) })
            });
        }
        self
    }

    pub fn has_signal(&self, name: &str) -> bool {
        self.meta.signals.values().any(|s| s.name == name)
    }

    /// Adds an async method. `params` is a tuple signature such as `(sf)`.
    pub fn method_async<F, Fut>(mut self, name: &str, params: &str, ret: &str, f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        Sig::params(params).expect("valid parameter signature");
        Sig::parse(ret).expect("valid return signature");
        let uid = self.next;
        self.next += 1;
        self.meta.methods.insert(uid, Method { uid, name: name.into(), params: params.into(), ret: ret.into() });
        self.handlers.insert(uid, Arc::new(move |a| Box::pin(f(a))));
        self
    }

    pub fn method<F>(self, name: &str, params: &str, ret: &str, f: F) -> Self
    where
        F: Fn(Vec<Value>) -> Result<Value, String> + Send + Sync + 'static,
    {
        let f = Arc::new(f);
        self.method_async(name, params, ret, move |a| {
            let f = f.clone();
            async move { f(a) }
        })
    }

    pub fn signal(mut self, name: &str, sig: &str) -> Self {
        Sig::parse(sig).expect("valid signal signature");
        let uid = self.next;
        self.next += 1;
        self.meta.signals.insert(uid, Signal { uid, name: name.into(), sig: sig.into() });
        self
    }
}

struct Service {
    id: u32,
    name: String,
    meta: MetaObject,
    handlers: HashMap<u32, Handler>,
}

struct Subscriber {
    link: u64,
    remote: Option<mpsc::UnboundedSender<Message>>,
    local: Option<Listener>,
}

#[derive(Default)]
pub struct Node {
    services: RwLock<Vec<Arc<Service>>>,
    subscribers: Mutex<HashMap<(u32, u32), Vec<Subscriber>>>,
    next_link: AtomicU64,
    pub machine_id: String,
    /// `tcp://host:port` of each listener, advertised in ServiceInfo so clients can reach services.
    endpoints: RwLock<Vec<String>>,
    /// Applied to every service but the ServiceDirectory before it is registered.
    completer: RwLock<Option<Completer>>,
    /// Accept loops and client connections, aborted by `close`.
    tasks: Mutex<Vec<tokio::task::AbortHandle>>,
}

fn arg_err(e: String) -> String {
    format!("invalid arguments: {e}")
}

impl Node {
    pub fn new() -> Arc<Node> {
        let node = Arc::new(Node { machine_id: "choren-virtual-robot".into(), ..Default::default() });
        let sd = Arc::downgrade(&node);
        let sd0 = sd.clone();
        let info = move |s: &Service| {
            let endpoints = sd0.upgrade().map(|n| n.endpoints.read().unwrap().iter().map(|e| e.as_str().into()).collect()).unwrap_or_default();
            Value::Tuple(vec![s.name.as_str().into(), Value::UInt(s.id as u64), "choren-virtual-robot".into(), Value::UInt(std::process::id() as u64), Value::List(endpoints), "".into()])
        };
        let (sd1, sd2, info2) = (sd.clone(), sd.clone(), info.clone());
        let directory = ServiceBuilder::new("ServiceDirectory")
            .method("service", "(s)", SERVICE_INFO_SIG, move |a| {
                let node = sd1.upgrade().ok_or("shutting down")?;
                let name = a.first().and_then(Value::as_str).ok_or("service name expected")?;
                let s = node.find(name).ok_or_else(|| format!("Cannot find service '{name}' in index"))?;
                Ok(info(&s))
            })
            .method("services", "()", &format!("[{SERVICE_INFO_SIG}]"), move |_| {
                let node = sd2.upgrade().ok_or("shutting down")?;
                Ok(Value::List(node.services.read().unwrap().iter().map(|s| info2(s)).collect()))
            })
            .method("machineId", "()", "s", |_| Ok("choren-virtual-robot".into()))
            .method("_socketOfService", "(I)", "o", |_| Err(REMOTE_SERVICES.into()))
            .method("registerService", &format!("({SERVICE_INFO_SIG})"), "I", |_| Err(REMOTE_SERVICES.into()))
            .method("unregisterService", "(I)", "v", |_| Err(REMOTE_SERVICES.into()))
            .method("serviceReady", "(I)", "v", |_| Err(REMOTE_SERVICES.into()))
            .method("updateServiceInfo", &format!("({SERVICE_INFO_SIG})"), "v", |_| Err(REMOTE_SERVICES.into()))
            .signal("serviceAdded", "(Is)")
            .signal("serviceRemoved", "(Is)");
        node.register(directory);
        node
    }

    /// Sets a hook that completes each service registered from now on (e.g. with the methods a
    /// reference API declares).
    pub fn set_completer(&self, f: Completer) {
        *self.completer.write().unwrap() = Some(f);
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Registers a service and returns its id (the ServiceDirectory is 1).
    pub fn register(&self, b: ServiceBuilder) -> u32 {
        let completer = self.completer.read().unwrap().clone();
        let b = match completer {
            Some(f) if b.name != "ServiceDirectory" => f(b),
            _ => b,
        };
        let mut services = self.services.write().unwrap();
        let id = services.len() as u32 + 1;
        let name = b.name.clone();
        services.push(Arc::new(Service { id, name: b.name, meta: b.meta, handlers: b.handlers }));
        drop(services);
        if id != SERVICE_DIRECTORY {
            self.emit("ServiceDirectory", "serviceAdded", vec![Value::UInt(id as u64), name.into()]);
        }
        id
    }

    fn find(&self, name: &str) -> Option<Arc<Service>> {
        self.services.read().unwrap().iter().find(|s| s.name == name).cloned()
    }

    fn by_id(&self, id: u32) -> Option<Arc<Service>> {
        self.services.read().unwrap().get(id.wrapping_sub(1) as usize).cloned()
    }

    pub fn service_names(&self) -> Vec<String> {
        self.services.read().unwrap().iter().map(|s| s.name.clone()).collect()
    }

    pub fn meta(&self, service: &str) -> Option<MetaObject> {
        self.find(service).map(|s| s.meta.clone())
    }

    /// In-process call by service and method name, with overload resolution and type conversion.
    pub async fn call(&self, service: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let s = self.find(service).ok_or_else(|| format!("Cannot find service '{service}'"))?;
        let m = s.meta.resolve(method, &args).ok_or_else(|| format!("Can't find method: {service}.{method} with {} arguments", args.len()))?;
        // Round-trip through the declared signature so handlers see the same values a remote call gets.
        let sig = Sig::parse(&m.params)?;
        let mut buf = vec![];
        encode(&Value::Tuple(args), &sig, &mut buf).map_err(arg_err)?;
        let args = decode(&buf, &sig)?.items();
        let h = s.handlers.get(&m.uid).cloned().ok_or("method has no handler")?;
        h(args).await
    }

    /// Raises a signal for every subscriber, remote or in-process.
    pub fn emit(&self, service: &str, signal: &str, args: Vec<Value>) {
        let Some(s) = self.find(service) else { return };
        let Some(sig) = s.meta.signal_named(signal) else { return };
        let mut payload = vec![];
        if encode(&Value::Tuple(args.clone()), &Sig::parse(&sig.sig).unwrap(), &mut payload).is_err() {
            return;
        }
        let mut locals: Vec<Listener> = vec![];
        if let Some(list) = self.subscribers.lock().unwrap().get_mut(&(s.id, sig.uid)) {
            list.retain(|sub| match (&sub.remote, &sub.local) {
                (Some(tx), _) => tx.send(Message::new(0, Kind::Event, s.id, MAIN_OBJECT, sig.uid, payload.clone())).is_ok(),
                (_, Some(f)) => {
                    locals.push(f.clone());
                    true
                }
                _ => false,
            });
        }
        // Called outside the lock so listeners may emit or subscribe themselves.
        for f in locals {
            f(args.clone());
        }
    }

    /// Subscribes an in-process listener; returns the link id for `disconnect`.
    pub fn connect(&self, service: &str, signal: &str, f: impl Fn(Vec<Value>) + Send + Sync + 'static) -> Result<u64, String> {
        let s = self.find(service).ok_or_else(|| format!("Cannot find service '{service}'"))?;
        let sig = s.meta.signal_named(signal).ok_or_else(|| format!("{service} has no signal {signal}"))?;
        let link = self.next_link.fetch_add(1, Ordering::Relaxed) + 1;
        self.subscribers.lock().unwrap().entry((s.id, sig.uid)).or_default().push(Subscriber { link, remote: None, local: Some(Arc::new(f)) });
        Ok(link)
    }

    pub fn disconnect(&self, link: u64) {
        for list in self.subscribers.lock().unwrap().values_mut() {
            list.retain(|s| s.link != link);
        }
    }

    /// Accepts qi clients on `addr` (e.g. `127.0.0.1:9559`, port 0 for any); returns the bound address.
    pub async fn listen(self: &Arc<Self>, addr: &str) -> std::io::Result<std::net::SocketAddr> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        self.endpoints.write().unwrap().push(format!("tcp://{local}"));
        let node = self.clone();
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let _ = stream.set_nodelay(true);
                let conn = tokio::spawn(node.clone().serve(stream)).abort_handle();
                let mut tasks = node.tasks.lock().unwrap();
                tasks.retain(|t| !t.is_finished());
                tasks.push(conn);
            }
        });
        self.tasks.lock().unwrap().push(accept.abort_handle());
        Ok(local)
    }

    /// Stops listening and drops every client connection; the port is free afterwards.
    pub fn close(&self) {
        for t in self.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
    }

    async fn serve(self: Arc<Self>, stream: TcpStream) {
        let (mut rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let writer = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if wr.write_all(&m.to_bytes()).await.is_err() {
                    break;
                }
            }
        });
        // Signal subscribers keep `tx` alive, so `close` must end the writer (and socket) itself.
        self.tasks.lock().unwrap().push(writer.abort_handle());
        let conn = Conn::new(tx.clone());
        let mut links = vec![];
        loop {
            let mut h = [0u8; HEADER];
            if rd.read_exact(&mut h).await.is_err() {
                break;
            }
            let Ok((mut msg, size)) = Message::parse_header(&h) else { break };
            msg.payload = vec![0; size];
            if rd.read_exact(&mut msg.payload).await.is_err() {
                break;
            }
            match msg.kind {
                Kind::Reply | Kind::Error | Kind::Canceled => conn.answer(msg),
                Kind::Event => conn.event(&msg),
                // Objects this connection was given (returned by a method) answer their own calls.
                // (Service 0 is the server itself: authentication.)
                Kind::Call | Kind::Post if msg.service != 0 && (msg.object != MAIN_OBJECT || msg.action == meta::TERMINATE) => {
                    if !conn.serve(msg.clone()) {
                        conn.reply(&msg, Err(format!("Can't find object {} of service {}", msg.object, msg.service)));
                    }
                }
                Kind::Call | Kind::Post => {
                    if msg.action == meta::REGISTER_EVENT || msg.action == meta::REGISTER_EVENT_WITH_SIGNATURE {
                        if let Ok(link) = self.subscribe_remote(&msg, &tx) {
                            links.push(link);
                        }
                    }
                    let (node, conn) = (self.clone(), conn.clone());
                    tokio::spawn(async move {
                        let reply = node.dispatch(&conn, &msg).await;
                        conn.reply(&msg, reply);
                    });
                }
                _ => {}
            }
        }
        conn.close();
        for l in links {
            self.disconnect(l);
        }
        writer.abort();
    }

    fn subscribe_remote(&self, msg: &Message, tx: &mpsc::UnboundedSender<Message>) -> Result<u64, String> {
        let a = decode(&msg.payload, &Sig::parse("(IIL)")?)?.items();
        let signal = a.get(1).and_then(Value::as_i64).ok_or("bad registerEvent")? as u32;
        let link = a.get(2).and_then(Value::as_i64).unwrap_or(0) as u64;
        self.subscribers.lock().unwrap().entry((msg.service, signal)).or_default().push(Subscriber { link, remote: Some(tx.clone()), local: None });
        Ok(link)
    }

    async fn dispatch(&self, conn: &Arc<Conn>, msg: &Message) -> Result<Vec<u8>, String> {
        if msg.service == 0 {
            // authenticate: no credentials are required by the virtual robot.
            let caps = Value::Map(vec![
                ("__qi_auth_state".into(), Value::Dynamic(Box::new(Value::UInt(3)))),
                ("ClientServerSocket".into(), Value::Dynamic(Box::new(Value::Bool(true)))),
            ]);
            let mut out = vec![];
            encode(&caps, &Sig::parse("{sm}")?, &mut out)?;
            return Ok(out);
        }
        let s = self.by_id(msg.service).ok_or_else(|| format!("unknown service {}", msg.service))?;
        if msg.action == meta::UNREGISTER_EVENT {
            let a = decode(&msg.payload, &Sig::parse("(IIL)")?)?.items();
            if let Some(link) = a.get(2).and_then(Value::as_i64) {
                self.disconnect(link as u64);
            }
        }
        if !s.meta.methods.contains_key(&msg.action) {
            return Err(format!("{} has no method {}", s.name, msg.action));
        }
        conn.invoke(&s.meta, &s.handlers, s.id, msg.action, &msg.payload).await
    }
}

/// Error replies carry the message as a dynamic string.
pub fn error_payload(e: &str) -> Vec<u8> {
    let mut out = vec![];
    let _ = encode(&Value::Dynamic(Box::new(e.into())), &Sig::Dynamic, &mut out);
    out
}
