//! Robots on the local network: NAOqi announces `_naoqi._tcp` over mDNS (Choregraphe's
//! `libmdnsmanager`). A one-shot query from an ephemeral port gets unicast answers (RFC 6762 §6.7).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::net::UdpSocket;

const SERVICE: &str = "_naoqi._tcp.local";
/// Anyone on the LAN can answer: keep what one search gathers small.
const MAX_RECORDS: usize = 256;
const MAX_NAME: usize = 255;

/// Robots that answered within `wait`: (instance name, `tcp://address:port`), sorted by name.
pub async fn discover(wait: Duration) -> Result<Vec<(String, String)>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| format!("Cannot search the network: {e}"))?;
    socket.send_to(&query(SERVICE), "224.0.0.251:5353").await.map_err(|e| format!("Cannot search the network: {e}"))?;
    let mut records = Records::default();
    let mut buf = vec![0u8; 9000];
    let _ = tokio::time::timeout(wait, async {
        while let Ok((n, _)) = socket.recv_from(&mut buf).await {
            records.add(&buf[..n]);
        }
    })
    .await;
    Ok(records.robots())
}

/// A DNS query for the PTR records of `name`.
fn query(name: &str) -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 12, 0, 1]); // root, type PTR, class IN
    q
}

/// The PTR, SRV and A records gathered from every answer.
#[derive(Default)]
struct Records {
    instances: Vec<String>,
    srv: BTreeMap<String, (String, u16)>,
    a: BTreeMap<String, Ipv4Addr>,
}

impl Records {
    fn add(&mut self, msg: &[u8]) {
        if self.instances.len() + self.srv.len() + self.a.len() >= MAX_RECORDS {
            return;
        }
        let Some(count) = msg.get(4..12).map(|c| (1..4).map(|i| u16::from_be_bytes([c[2 * i], c[2 * i + 1]]) as usize).sum::<usize>()) else { return };
        let mut p = 12;
        // Skip the questions.
        for _ in 0..msg.get(4..6).map_or(0, |c| u16::from_be_bytes([c[0], c[1]])) {
            let Some((_, next)) = name(msg, p) else { return };
            p = next + 4;
        }
        for _ in 0..count {
            let Some((owner, next)) = name(msg, p) else { return };
            let Some(h) = msg.get(next..next + 10) else { return };
            let (kind, len) = (u16::from_be_bytes([h[0], h[1]]), u16::from_be_bytes([h[8], h[9]]) as usize);
            let data = next + 10;
            let Some(rdata) = msg.get(data..data + len) else { return };
            match kind {
                12 if owner.eq_ignore_ascii_case(SERVICE) => {
                    if let Some((instance, _)) = name(msg, data) {
                        if !self.instances.contains(&instance) {
                            self.instances.push(instance);
                        }
                    }
                }
                33 if len >= 7 => {
                    if let Some((target, _)) = name(msg, data + 6) {
                        self.srv.insert(owner.to_ascii_lowercase(), (target.to_ascii_lowercase(), u16::from_be_bytes([rdata[4], rdata[5]])));
                    }
                }
                1 if len == 4 => {
                    self.a.insert(owner.to_ascii_lowercase(), Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]));
                }
                _ => {}
            }
            p = data + len;
        }
    }

    fn robots(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .instances
            .iter()
            .filter_map(|i| {
                let (host, port) = self.srv.get(&i.to_ascii_lowercase())?;
                let ip = self.a.get(host)?;
                let label = i.strip_suffix(&format!(".{SERVICE}")).unwrap_or(i);
                Some((label.to_string(), format!("tcp://{ip}:{port}")))
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Reads a (possibly compressed) domain name at `p`: the dotted name and the offset after it.
fn name(msg: &[u8], mut p: usize) -> Option<(String, usize)> {
    let (mut labels, mut end, mut jumps) = (vec![], None, 0);
    loop {
        let len = *msg.get(p)? as usize;
        if len == 0 {
            return Some((labels.join("."), end.unwrap_or(p + 1)));
        }
        if len & 0xc0 == 0xc0 {
            jumps += 1;
            if jumps > 16 {
                return None;
            }
            end.get_or_insert(p + 2);
            p = ((len & 0x3f) << 8) | *msg.get(p + 1)? as usize;
            continue;
        }
        labels.push(String::from_utf8_lossy(msg.get(p + 1..p + 1 + len)?).into_owned());
        if labels.iter().map(|l| l.len() + 1).sum::<usize>() > MAX_NAME {
            return None;
        }
        p += 1 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Avahi-style answer: PTR in answers, SRV and A in additional records, compressed names.
    #[test]
    fn reads_robots_from_an_answer() {
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 2];
        let service_at = m.len();
        m.extend(&query(SERVICE)[12..12 + SERVICE.len() + 2]);
        m.extend([0, 12, 0, 1, 0, 0, 0, 120]);
        let instance = [&[3u8][..], b"Nao", &[0xc0, service_at as u8]].concat();
        m.extend((instance.len() as u16).to_be_bytes());
        let instance_at = m.len();
        m.extend(&instance);
        // SRV: priority, weight, port 9559, target nao.local
        m.extend([0xc0, instance_at as u8, 0, 33, 0x80, 1, 0, 0, 0, 120]);
        let target = [&[3u8][..], b"nao", &[5], b"local", &[0]].concat();
        m.extend(((6 + target.len()) as u16).to_be_bytes());
        m.extend([0, 0, 0, 0, 0x25, 0x57]);
        let target_at = m.len();
        m.extend(&target);
        m.extend([0xc0, target_at as u8, 0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 192, 168, 1, 12]);
        let mut r = Records::default();
        r.add(&m);
        assert_eq!(r.robots(), [("Nao".to_string(), "tcp://192.168.1.12:9559".to_string())]);
        r.add(&m[..m.len() - 3]); // truncated packets are ignored
        assert_eq!(r.robots().len(), 1);
        // A flood of answers stops growing at the cap, and over-long names are dropped.
        let mut flood = Records::default();
        for i in 0..1000 {
            let mut p = m.clone();
            p[instance_at + 1..instance_at + 4].copy_from_slice(format!("{i:03}").as_bytes());
            flood.add(&p);
        }
        assert!(flood.instances.len() + flood.srv.len() + flood.a.len() <= MAX_RECORDS + 3, "one packet adds at most 3 records past the cap");
        let long = [&[63u8][..], &[b'a'; 63], &[63], &[b'a'; 63], &[63], &[b'a'; 63], &[63], &[b'a'; 63], &[0]].concat();
        assert!(name(&long, 0).is_none());
    }
}
