//! Client side: one TCP connection, authenticated, calling services by name with metaObject-based
//! overload resolution and typed encoding, receiving signals, and serving the objects it sends.

use crate::message::{Kind, Message, HEADER};
use crate::meta::{self, MetaObject};
use crate::node::SERVICE_DIRECTORY;
use crate::object::Conn;
use crate::sig::Sig;
use crate::value::{decode, encode, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Service 0 (the server itself), action 8: `authenticate(CapabilityMap)`.
const AUTHENTICATE: u32 = 8;

pub struct Client {
    conn: Arc<Conn>,
    services: Mutex<HashMap<String, Route>>,
    /// `tcp://host:port` this client connected to, and the address that reached.
    url: String,
    peer: Option<SocketAddr>,
    /// Connections to other processes' endpoints, for services they host.
    peers: tokio::sync::Mutex<HashMap<String, Arc<Client>>>,
    reader: tokio::task::JoinHandle<()>,
}

/// Where a service lives: its id, meta object, and the connection that reaches it (`None`: this one).
type Route = (u32, Arc<MetaObject>, Option<Arc<Client>>);

impl Drop for Client {
    fn drop(&mut self) {
        self.reader.abort();
        self.conn.close();
    }
}

impl Client {
    /// Connects to `host:port` (a `tcp://` prefix is accepted) and authenticates.
    pub async fn connect(url: &str) -> Result<Client, String> {
        let addr = url.trim_start_matches("tcp://");
        let stream = TcpStream::connect(addr).await.map_err(|e| format!("Cannot connect to {addr}: {e}"))?;
        let _ = stream.set_nodelay(true);
        let peer = stream.peer_addr().ok();
        let (mut rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if wr.write_all(&m.to_bytes()).await.is_err() {
                    break;
                }
            }
        });
        let conn = Conn::new(tx);
        let c2 = conn.clone();
        let reader = tokio::spawn(async move {
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
                    Kind::Reply | Kind::Error | Kind::Canceled => c2.answer(msg),
                    Kind::Event => c2.event(&msg),
                    // The robot calls back the objects this client sent (e.g. a qi::File).
                    Kind::Call | Kind::Post => {
                        if !c2.serve(msg.clone()) {
                            c2.reply(&msg, Err(format!("No object {} on this client", msg.object)));
                        }
                    }
                    _ => {}
                }
            }
            // Fail every outstanding and future call when the connection drops.
            c2.close();
        });
        let url = format!("tcp://{addr}");
        let c = Client { conn, services: Default::default(), url, peer, peers: Default::default(), reader };
        let caps = Value::Map(vec![("ClientServerSocket".into(), Value::Dynamic(Box::new(Value::Bool(true))))]);
        let mut payload = vec![];
        encode(&Value::Tuple(vec![caps]), &Sig::parse("({sm})")?, &mut payload)?;
        let reply = c.conn.call(0, 0, AUTHENTICATE, payload).await?;
        let state = decode(&reply, &Sig::parse("{sm}")?)?;
        let done = match &state {
            Value::Map(pairs) => pairs.iter().find(|(k, _)| k.as_str() == Some("__qi_auth_state")).and_then(|(_, v)| v.as_i64()),
            _ => None,
        };
        if done != Some(3) {
            return Err("The robot requires authentication, which is not supported yet.".into());
        }
        Ok(c)
    }

    async fn meta_of(&self, service: u32) -> Result<MetaObject, String> {
        let mut payload = vec![];
        encode(&Value::Tuple(vec![Value::UInt(1)]), &Sig::parse("(I)")?, &mut payload)?;
        let bytes = self.conn.call(service, 1, meta::META_OBJECT, payload).await?;
        MetaObject::decode(&mut crate::value::Reader { buf: &bytes })
    }

    async fn cached(&self, name: &str, id: u32, via: Option<Arc<Client>>) -> Result<Route, String> {
        if let Some(s) = self.services.lock().unwrap().get(name) {
            return Ok(s.clone());
        }
        let meta = Arc::new(match &via {
            Some(peer) => peer.meta_of(id).await?,
            None => self.meta_of(id).await?,
        });
        let route = (id, meta, via);
        self.services.lock().unwrap().insert(name.into(), route.clone());
        Ok(route)
    }

    /// The connection for a service advertised on `endpoints`: another process's endpoint when
    /// one is reachable (see `plan`), else this connection, which also covers gateways that
    /// forward every service.
    async fn peer_for(&self, endpoints: &[String]) -> Option<Arc<Client>> {
        let Plan::Try(candidates) = plan(&self.url, self.peer, endpoints) else { return None };
        let mut peers = self.peers.lock().await;
        for e in candidates {
            if let Some(p) = peers.get(&e) {
                return Some(p.clone());
            }
            if let Ok(c) = Box::pin(Client::connect(&e)).await {
                let c = Arc::new(c);
                peers.insert(e, c.clone());
                return Some(c);
            }
        }
        None
    }

    /// Resolves a service id, meta object and connection (cached).
    async fn route(&self, name: &str) -> Result<Route, String> {
        if let Some(s) = self.services.lock().unwrap().get(name) {
            return Ok(s.clone());
        }
        let (sd, sd_meta, _) = self.cached("ServiceDirectory", SERVICE_DIRECTORY, None).await?;
        if name == "ServiceDirectory" {
            return Ok((sd, sd_meta, None));
        }
        // Decode ServiceInfo with the signature the directory itself declares.
        let info = self.invoke(sd, &sd_meta, "ServiceDirectory", "service", vec![name.into()]).await?;
        let fields = info.items();
        let id = fields.get(1).and_then(Value::as_i64).ok_or("malformed ServiceInfo")? as u32;
        let endpoints: Vec<String> = fields.get(4).map(|e| e.items().iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let via = self.peer_for(&endpoints).await;
        self.cached(name, id, via).await
    }

    /// Resolves a service id and meta object (cached).
    pub async fn service(&self, name: &str) -> Result<(u32, Arc<MetaObject>), String> {
        self.route(name).await.map(|(id, meta, _)| (id, meta))
    }

    async fn invoke(&self, id: u32, meta: &MetaObject, service: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let m = meta.resolve(method, &args).ok_or_else(|| format!("{service} has no method {method} taking {} arguments", args.len()))?.clone();
        let payload = self.conn.encode(&Value::Tuple(args), &m.params, id).map_err(|e| format!("{service}.{method}: {e}"))?;
        let reply = self.conn.call(id, 1, m.uid, payload).await?;
        Ok(self.conn.bind(decode(&reply, &Sig::parse(&m.ret)?)?))
    }

    /// Calls a service method. Objects in `args` (e.g. `object::file`) are served to the robot
    /// over this connection; objects in the result are callable (`Object::remote`).
    pub async fn call(&self, service: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
        let (id, meta, via) = self.route(service).await?;
        via.as_deref().unwrap_or(self).invoke(id, &meta, service, method, args).await
    }

    /// Subscribes to a signal; values arrive decoded as the signal's argument tuple.
    pub async fn subscribe(&self, service: &str, signal: &str) -> Result<mpsc::UnboundedReceiver<Value>, String> {
        let (id, meta, via) = self.route(service).await?;
        let c = via.as_deref().unwrap_or(self);
        c.conn.subscribe(id, 1, &meta, signal).await.map_err(|e| if e.starts_with("no signal") { format!("{service} has no signal {signal}") } else { e })
    }

    pub async fn services(&self) -> Result<Vec<String>, String> {
        let list = self.call("ServiceDirectory", "services", vec![]).await?;
        Ok(list.items().iter().filter_map(|i| i.items().first().and_then(|n| n.as_str().map(String::from))).collect())
    }
}

/// Where to reach a service advertised on `endpoints`, from a connection to `url` that reached
/// `peer`.
#[derive(Debug, PartialEq)]
enum Plan {
    /// The process this connection reaches serves it (or nothing better is advertised).
    Here,
    /// Other processes' endpoints, best first.
    Try(Vec<String>),
}

/// Endpoints naming the address this connection reached are this process. Loopback endpoints
/// are the robot's own machine, so they are tried only when the robot is on this machine.
/// Endpoints on the robot's address come first; `tcps://` and other schemes are not supported.
fn plan(url: &str, peer: Option<SocketAddr>, endpoints: &[String]) -> Plan {
    let parse = |e: &str| -> Option<(String, Option<SocketAddr>)> {
        let addr = e.strip_prefix("tcp://")?;
        let (host, port) = addr.rsplit_once(':')?;
        let ip = host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().ok();
        Some((host.to_string(), ip.zip(port.parse().ok()).map(|(ip, port)| SocketAddr::new(ip, port))))
    };
    let loopback = |host: &str, addr: Option<SocketAddr>| host == "localhost" || addr.is_some_and(|a| a.ip().is_loopback());
    let local = peer.map_or_else(|| parse(url).is_some_and(|(h, a)| loopback(&h, a)), |p| p.ip().is_loopback());
    let mut candidates = vec![];
    for e in endpoints {
        let Some((host, addr)) = parse(e) else { continue };
        if e == url || (addr.is_some() && addr == peer) {
            return Plan::Here;
        }
        if local || !loopback(&host, addr) {
            let on_robot = addr.is_some_and(|a| Some(a.ip()) == peer.map(|p| p.ip()));
            candidates.push((!on_robot, e.clone()));
        }
    }
    if candidates.is_empty() {
        return Plan::Here;
    }
    candidates.sort_by_key(|c| c.0);
    Plan::Try(candidates.into_iter().map(|c| c.1).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps(e: &[&str]) -> Vec<String> {
        e.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_remote_robot_never_routes_to_this_machine() {
        let robot = Some("192.168.1.10:9559".parse().unwrap());
        // Connected by name: the robot's own address and port is the process already reached.
        assert_eq!(plan("tcp://nao.local:9559", robot, &eps(&["tcp://192.168.1.10:9559", "tcp://127.0.0.1:9559"])), Plan::Here);
        // A service in another process on the robot: its LAN endpoint only, never loopback.
        assert_eq!(
            plan("tcp://nao.local:9559", robot, &eps(&["tcp://127.0.0.1:41000", "tcp://10.0.0.5:41000", "tcp://192.168.1.10:41000"])),
            Plan::Try(eps(&["tcp://192.168.1.10:41000", "tcp://10.0.0.5:41000"]))
        );
        assert_eq!(plan("tcp://192.168.1.10:9559", robot, &eps(&["tcp://127.0.0.1:41000", "tcp://[::1]:41000", "tcp://localhost:41000"])), Plan::Here);
    }

    #[test]
    fn a_local_robot_uses_its_loopback_endpoints() {
        let here = Some("127.0.0.1:19559".parse().unwrap());
        assert_eq!(plan("tcp://127.0.0.1:19559", here, &eps(&["tcp://192.168.50.208:56372", "tcp://127.0.0.1:56372"])), Plan::Try(eps(&["tcp://127.0.0.1:56372", "tcp://192.168.50.208:56372"])));
        assert_eq!(plan("tcp://localhost:19559", here, &eps(&["tcp://127.0.0.1:19559"])), Plan::Here);
    }

    #[test]
    fn unsupported_or_missing_endpoints_stay_on_this_connection() {
        let robot = Some("192.168.1.10:9503".parse().unwrap());
        assert_eq!(plan("tcp://pepper.local:9503", robot, &eps(&["tcps://192.168.1.10:9503"])), Plan::Here);
        assert_eq!(plan("tcp://pepper.local:9503", robot, &[]), Plan::Here);
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_falls_back_to_this_connection() {
        let node = crate::Node::new();
        let addr = node.listen("127.0.0.1:0").await.unwrap();
        let c = Client::connect(&addr.to_string()).await.unwrap();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap(); // freed on drop
        assert!(c.peer_for(&[format!("tcp://{closed}")]).await.is_none());
        assert_eq!(c.services().await.unwrap(), ["ServiceDirectory"]);
    }
}
