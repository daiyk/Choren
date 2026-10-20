//! 28-byte message header: magic (big-endian) then id, size, version, type, flags, service,
//! object, action (little-endian).

pub const MAGIC: u32 = 0x42de_ad42;
pub const HEADER: usize = 28;
/// libqi refuses larger payloads by default.
pub const MAX_PAYLOAD: usize = 50 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Call = 1,
    Reply = 2,
    Error = 3,
    Post = 4,
    Event = 5,
    Capability = 6,
    Cancel = 7,
    Canceled = 8,
}

impl Kind {
    pub fn from_u8(b: u8) -> Option<Kind> {
        Some(match b {
            1 => Kind::Call,
            2 => Kind::Reply,
            3 => Kind::Error,
            4 => Kind::Post,
            5 => Kind::Event,
            6 => Kind::Capability,
            7 => Kind::Cancel,
            8 => Kind::Canceled,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: u32,
    pub kind: Kind,
    pub flags: u8,
    pub service: u32,
    pub object: u32,
    pub action: u32,
    pub payload: Vec<u8>,
}

/// Payload carries a dynamic value instead of the declared type.
pub const FLAG_DYNAMIC: u8 = 1;

impl Message {
    pub fn new(id: u32, kind: Kind, service: u32, object: u32, action: u32, payload: Vec<u8>) -> Message {
        Message { id, kind, flags: 0, service, object, action, payload }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + self.payload.len());
        b.extend_from_slice(&MAGIC.to_be_bytes());
        b.extend_from_slice(&self.id.to_le_bytes());
        b.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.push(self.kind as u8);
        b.push(self.flags);
        b.extend_from_slice(&self.service.to_le_bytes());
        b.extend_from_slice(&self.object.to_le_bytes());
        b.extend_from_slice(&self.action.to_le_bytes());
        b.extend_from_slice(&self.payload);
        b
    }

    /// Parses a header; returns the message without payload and the payload size.
    pub fn parse_header(h: &[u8; HEADER]) -> Result<(Message, usize), String> {
        let u = |i: usize| u32::from_le_bytes(h[i..i + 4].try_into().unwrap());
        if u32::from_be_bytes(h[0..4].try_into().unwrap()) != MAGIC {
            return Err("bad magic: not a qi message".into());
        }
        let size = u(8) as usize;
        if size > MAX_PAYLOAD {
            return Err(format!("payload of {size} bytes is too large"));
        }
        let kind = Kind::from_u8(h[14]).ok_or_else(|| format!("unknown message type {}", h[14]))?;
        Ok((Message { id: u(4), kind, flags: h[15], service: u(16), object: u(20), action: u(24), payload: vec![] }, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_round_trip() {
        let m = Message::new(9, Kind::Call, 1, 1, 100, vec![1, 2, 3]);
        let b = m.to_bytes();
        assert_eq!(&b[..4], &[0x42, 0xde, 0xad, 0x42]);
        let (h, n) = Message::parse_header(b[..HEADER].try_into().unwrap()).unwrap();
        assert_eq!(n, 3);
        assert_eq!(Message { payload: b[HEADER..].to_vec(), ..h }, m);
        assert!(Message::parse_header(&[0; HEADER]).is_err());
    }
}
