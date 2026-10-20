//! Ogre binary `.mesh` reader (MeshSerializer v1.41 and v1.8, little endian), enough for drawing:
//! positions, normals and triangle indices per submesh, plus the material name. The meshes are
//! read from the user's Choregraphe install at runtime and never shipped.
//!
//! Format: a `0x1000` header followed by a version string, then chunks of `u16 id, u32 length`.
//! As in Ogre's own reader, chunk lengths are only trusted for skipping chunks we don't read.

const HEADER: u16 = 0x1000;
const MESH: u16 = 0x3000;
const SUBMESH: u16 = 0x4000;
const GEOMETRY: u16 = 0x5000;
const VERTEX_DECLARATION: u16 = 0x5100;
const VERTEX_ELEMENT: u16 = 0x5110;
const VERTEX_BUFFER: u16 = 0x5200;
const VERTEX_BUFFER_DATA: u16 = 0x5210;
const SUBMESH_CHUNKS: &[u16] = &[0x4010, 0x4100, 0x4200]; // operation, bone assignment, texture alias

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct SubMesh {
    pub material: String,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub version: String,
    pub submeshes: Vec<SubMesh>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("truncated mesh")?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn line(&mut self) -> Result<String, String> {
        let end = self.b[self.at..].iter().position(|&c| c == b'\n').ok_or("unterminated string")?;
        let s = String::from_utf8_lossy(&self.b[self.at..self.at + end]).into_owned();
        self.at += end + 1;
        Ok(s)
    }
    fn eof(&self) -> bool {
        self.at >= self.b.len()
    }
    /// Next chunk id and length, without consuming it.
    fn peek(&self) -> Option<(u16, u32)> {
        let h = self.b.get(self.at..self.at + 6)?;
        Some((u16::from_le_bytes([h[0], h[1]]), u32::from_le_bytes([h[2], h[3], h[4], h[5]])))
    }
    fn skip_chunk(&mut self) -> Result<(), String> {
        let (_, len) = self.peek().ok_or("truncated chunk")?;
        if len < 6 {
            return Err("invalid chunk length".into());
        }
        self.take(len as usize).map(|_| ())
    }
}

fn geometry(r: &mut Reader) -> Result<Geometry, String> {
    r.take(6)?;
    let count = r.u32()? as usize;
    // (source, type, semantic, offset)
    let mut elements = vec![];
    let mut g = Geometry::default();
    while let Some((id, _)) = r.peek() {
        match id {
            VERTEX_DECLARATION => {
                r.take(6)?;
                while r.peek().is_some_and(|(id, _)| id == VERTEX_ELEMENT) {
                    r.take(6)?;
                    let (source, ty, semantic, offset, _index) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?, r.u16()?);
                    elements.push((source, ty, semantic, offset as usize));
                }
            }
            VERTEX_BUFFER => {
                r.take(6)?;
                let (bind, size) = (r.u16()?, r.u16()? as usize);
                if r.peek().map(|c| c.0) != Some(VERTEX_BUFFER_DATA) {
                    return Err("vertex buffer without data".into());
                }
                r.take(6)?;
                let data = r.take(count * size)?;
                for &(source, ty, semantic, offset) in &elements {
                    // Only float3 positions (1) and normals (4) are needed for drawing.
                    let out = match (source == bind, ty, semantic) {
                        (true, 2, 1) => &mut g.positions,
                        (true, 2, 4) => &mut g.normals,
                        _ => continue,
                    };
                    for v in 0..count {
                        for c in 0..3 {
                            let at = v * size + offset + c * 4;
                            out.push(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()));
                        }
                    }
                }
            }
            _ => break,
        }
    }
    if g.positions.len() != count * 3 {
        return Err("mesh has no float3 positions".into());
    }
    Ok(g)
}

pub fn parse(bytes: &[u8]) -> Result<Mesh, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.u16()? != HEADER {
        return Err("not an Ogre mesh (or big endian)".into());
    }
    let version = r.line()?;
    if !matches!(version.as_str(), "[MeshSerializer_v1.41]" | "[MeshSerializer_v1.8]") {
        return Err(format!("unsupported mesh serializer {version}"));
    }
    if r.peek().map(|c| c.0) != Some(MESH) {
        return Err("mesh chunk missing".into());
    }
    r.take(6)?;
    r.u8()?; // skeletally animated
    let mut shared: Option<Geometry> = None;
    let mut subs = vec![];
    while let Some((id, _)) = r.peek() {
        match id {
            GEOMETRY => shared = Some(geometry(&mut r)?),
            SUBMESH => {
                r.take(6)?;
                let material = r.line()?;
                let use_shared = r.u8()? != 0;
                let n = r.u32()? as usize;
                let wide = r.u8()? != 0;
                let mut indices = Vec::with_capacity(n);
                for _ in 0..n {
                    indices.push(if wide { r.u32()? } else { r.u16()? as u32 });
                }
                let mut own = None;
                while let Some((id, _)) = r.peek() {
                    match id {
                        GEOMETRY if !use_shared && own.is_none() => own = Some(geometry(&mut r)?),
                        id if SUBMESH_CHUNKS.contains(&id) => r.skip_chunk()?,
                        _ => break,
                    }
                }
                subs.push((material, use_shared, indices, own));
            }
            _ => r.skip_chunk()?, // bounds, name table, edge lists, LOD, poses, animations…
        }
        if r.eof() {
            break;
        }
    }
    let submeshes = subs
        .into_iter()
        .map(|(material, use_shared, indices, own)| {
            let g = if use_shared { shared.clone() } else { own }.ok_or("submesh without geometry")?;
            let count = g.positions.len() as u32 / 3;
            if indices.iter().any(|&i| i >= count) {
                return Err("index out of range".to_string());
            }
            Ok(SubMesh { material, positions: g.positions, normals: g.normals, indices })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Mesh { version, submeshes })
}

/// `diffuse r g b` of each material in an Ogre `.material` script, as 0xRRGGBB.
pub fn material_colors(script: &str) -> Vec<(String, u32)> {
    let mut out = vec![];
    let mut current: Option<String> = None;
    for line in script.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("material ") {
            current = Some(name.trim_end_matches('{').trim().to_string());
        } else if let (Some(rgb), Some(name)) = (line.strip_prefix("diffuse "), &current) {
            let c: Vec<f64> = rgb.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            if c.len() >= 3 {
                let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
                out.push((name.clone(), byte(c[0]) << 16 | byte(c[1]) << 8 | byte(c[2])));
                current = None; // the first pass's diffuse is the colour
            }
        }
    }
    out
}

/// Flat binary form for the UI: per submesh `u32 rgb, u32 vertices, u32 indices`, then the
/// positions and normals (f32 × 3 each) and the indices (u32), all little endian.
pub fn to_bytes(mesh: &Mesh, colors: &[(String, u32)]) -> Vec<u8> {
    let mut out = vec![];
    let mut put = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    put(mesh.submeshes.len() as u32);
    for s in &mesh.submeshes {
        put(colors.iter().find(|c| c.0 == s.material).map_or(0xd4d4d4, |c| c.1));
        put(s.positions.len() as u32 / 3);
        put(s.indices.len() as u32);
        let normals = if s.normals.len() == s.positions.len() { s.normals.clone() } else { vec![0.0; s.positions.len()] };
        for v in s.positions.iter().chain(&normals) {
            put(v.to_bits());
        }
        for &i in &s.indices {
            put(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle mesh in the v1.41 layout, built here (no install needed).
    fn triangle() -> Vec<u8> {
        let mut b = vec![];
        let chunk = |b: &mut Vec<u8>, id: u16, len: u32| {
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&len.to_le_bytes());
        };
        b.extend_from_slice(&HEADER.to_le_bytes());
        b.extend_from_slice(b"[MeshSerializer_v1.41]\n");
        chunk(&mut b, MESH, 0);
        b.push(0);
        chunk(&mut b, SUBMESH, 0);
        b.extend_from_slice(b"NaoMat_MainSkin\n");
        b.push(0);
        b.extend_from_slice(&3u32.to_le_bytes());
        b.push(0);
        for i in [0u16, 1, 2] {
            b.extend_from_slice(&i.to_le_bytes());
        }
        chunk(&mut b, GEOMETRY, 0);
        b.extend_from_slice(&3u32.to_le_bytes());
        chunk(&mut b, VERTEX_DECLARATION, 0);
        for (sem, off) in [(1u16, 0u16), (4, 12)] {
            chunk(&mut b, VERTEX_ELEMENT, 16);
            for v in [0u16, 2, sem, off, 0] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        chunk(&mut b, VERTEX_BUFFER, 0);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        chunk(&mut b, VERTEX_BUFFER_DATA, 6 + 72);
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v.iter().chain(&[0.0, 0.0, 1.0]) {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        chunk(&mut b, 0x4010, 8); // operation type
        b.extend_from_slice(&4u16.to_le_bytes());
        chunk(&mut b, 0x9000, 6 + 28); // bounds
        b.extend_from_slice(&[0; 28]);
        b
    }

    #[test]
    fn reads_a_triangle() {
        let m = parse(&triangle()).unwrap();
        assert_eq!(m.submeshes.len(), 1);
        let s = &m.submeshes[0];
        assert_eq!((s.material.as_str(), s.indices.as_slice()), ("NaoMat_MainSkin", &[0, 1, 2][..]));
        assert_eq!(s.positions[3..6], [1.0, 0.0, 0.0]);
        assert_eq!(s.normals[0..3], [0.0, 0.0, 1.0]);
        let colors = material_colors("material NaoMat_MainSkin\n{\n technique\n {\n  pass\n  {\n   ambient 1 1 1 1\n   diffuse 1.0 0.5 0 1\n");
        assert_eq!(colors, [("NaoMat_MainSkin".to_string(), 0xff8000)]);
        let bytes = to_bytes(&m, &colors);
        assert_eq!(bytes.len(), 4 * (1 + 3 + 18 + 3));
        assert!(parse(b"\x10\x00[MeshSerializer_v9]\n").is_err());
    }
}
