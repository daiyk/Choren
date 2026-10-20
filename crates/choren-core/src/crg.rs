//! Legacy `.crg`: a Debian `ar` archive whose `data.tar.gz` member holds `behavior.xar`.

use std::io::Read;

/// Returns `(path, bytes)` for every regular file in the archive's data member.
pub fn files(crg: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let data = ar_member(crg, "data.tar.gz")?;
    let tar = if data.starts_with(&[0x1f, 0x8b]) { gunzip_padded(data)? } else { data.to_vec() };
    tar_files(&tar)
}

// Choregraphe pads the gzip member with zeros to 10 KiB, so skip the header and inflate the raw
// deflate stream, ignoring whatever follows it.
fn gunzip_padded(d: &[u8]) -> Result<Vec<u8>, String> {
    let short = || "Truncated gzip header".to_string();
    let flags = *d.get(3).ok_or_else(short)?;
    let mut p = 10;
    if flags & 4 != 0 {
        p += 2 + (*d.get(10).ok_or_else(short)? as usize | (*d.get(11).ok_or_else(short)? as usize) << 8);
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            p += d.get(p..).ok_or_else(short)?.iter().position(|&b| b == 0).ok_or_else(short)? + 1;
        }
    }
    if flags & 2 != 0 {
        p += 2;
    }
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(d.get(p..).ok_or_else(short)?)
        .read_to_end(&mut out)
        .map_err(|e| format!("Corrupt archive data: {e}"))?;
    Ok(out)
}

fn ar_member<'a>(ar: &'a [u8], wanted: &str) -> Result<&'a [u8], String> {
    if !ar.starts_with(b"!<arch>\n") {
        return Err("Not an ar archive".into());
    }
    let mut off = 8;
    while off + 60 <= ar.len() {
        let name = String::from_utf8_lossy(&ar[off..off + 16]);
        let name = name.trim().trim_end_matches('/');
        let size: usize = String::from_utf8_lossy(&ar[off + 48..off + 58])
            .trim()
            .parse()
            .map_err(|_| "Invalid archive member size")?;
        let body = ar.get(off + 60..off + 60 + size).ok_or("Invalid archive member size")?;
        if name == wanted {
            return Ok(body);
        }
        off += 60 + size + size % 2;
    }
    Err(format!("{wanted} not found in archive"))
}

fn tar_files(tar: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + 512 <= tar.len() {
        let field = |a: usize, b: usize| {
            let s = &tar[off + a..off + b];
            String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]).trim().to_string()
        };
        let name = field(0, 100);
        if name.is_empty() {
            break;
        }
        let size_txt = field(124, 136);
        let size = if size_txt.is_empty() { 0 } else { usize::from_str_radix(&size_txt, 8).map_err(|_| "Invalid archive file size")? };
        let body = tar.get(off + 512..off + 512 + size).ok_or("Invalid archive file size")?;
        let prefix = if &tar[off + 257..off + 262] == b"ustar" { field(345, 500) } else { String::new() };
        let name = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if matches!(tar[off + 156], 0 | b'0') {
            out.push((name.trim_start_matches("./").to_string(), body.to_vec()));
        }
        off += 512 + size.div_ceil(512) * 512;
    }
    Ok(out)
}

/// The file in `control.tar.gz`: the archived folder's `.metadata` lines (`Type: …`) in older
/// archives (`control`); Choregraphe 2.8.8 writes an empty `content` there and keeps `.metadata`
/// in the data (observed with its `qi::project::exportProject`, 2026-10-19).
pub fn metadata(crg: &[u8]) -> Result<String, String> {
    let control = ar_member(crg, "control.tar.gz")?;
    let tar = if control.starts_with(&[0x1f, 0x8b]) { gunzip_padded(control)? } else { control.to_vec() };
    let (_, text) = tar_files(&tar)?.into_iter().next().ok_or("The archive has no control file")?;
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// Writes a legacy `.crg`: `debian-binary`, `control.tar.gz` and `data.tar.gz` members laid out
/// like Choregraphe's (GNU `name/` members, zero mtime/owner, gzip padded to 10 KiB blocks).
pub fn write(files: &[(String, Vec<u8>)], title: &str) -> Result<Vec<u8>, String> {
    archive(files, &format!("Type: ChoregrapheProject\nVersion: 2.0\ntitle: {title}\n"))
}

/// The `.crg` container, also used for box
/// libraries saved as `.cbl`, with `control` holding the folder's `.metadata`.
pub fn archive(files: &[(String, Vec<u8>)], control: &str) -> Result<Vec<u8>, String> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar.gz", gzip_padded(&tar(&[("control".into(), control.as_bytes().to_vec())])?)?),
        ("data.tar.gz", gzip_padded(&tar(files)?)?),
    ] {
        out.extend(format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", format!("{name}/"), 0, 0, 0, 100644, body.len()).as_bytes());
        out.extend(&body);
        if body.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    Ok(out)
}

fn gzip_padded(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(data).map_err(|e| e.to_string())?;
    let mut out = gz.finish().map_err(|e| e.to_string())?;
    out.resize(out.len().div_ceil(10240) * 10240, 0);
    Ok(out)
}

fn tar(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (path, body) in files {
        let mut h = [0u8; 512];
        // ustar: names over 100 bytes go into the 155-byte prefix, split at a '/'.
        let (prefix, name) = match path.len() {
            0..=100 => ("", path.as_str()),
            _ => path.char_indices().filter(|&(i, c)| c == '/' && i <= 155 && path.len() - i - 1 <= 100).map(|(i, _)| (&path[..i], &path[i + 1..])).next().ok_or_else(|| format!("Path too long for a .crg archive: {path}"))?,
        };
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"000644 \0");
        h[108..116].copy_from_slice(b"000000 \0");
        h[116..124].copy_from_slice(b"000000 \0");
        h[124..136].copy_from_slice(format!("{:011o} ", body.len()).as_bytes());
        h[136..148].copy_from_slice(b"00000000000 ");
        h[148..156].copy_from_slice(b"        ");
        h[156] = b'0';
        h[257..265].copy_from_slice(b"ustar\x0000");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend(h);
        out.extend(body);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize((out.len() + 1024).div_ceil(10240) * 10240, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn written_archives_read_back() {
        let long = format!("{}/sound.ogg", "d".repeat(120));
        let files = vec![("behavior.xar".to_string(), b"<ChoregrapheProject />".to_vec()), (long.clone(), vec![1, 2, 3])];
        let crg = super::write(&files, "demo").unwrap();
        assert!(crg.starts_with(b"!<arch>\ndebian-binary/  0           0     0     100644  4         `\n2.0\n"));
        assert_eq!(super::files(&crg).unwrap(), files);
        assert_eq!(super::metadata(&crg).unwrap(), "Type: ChoregrapheProject\nVersion: 2.0\ntitle: demo\n");
        assert_eq!(super::ar_member(&crg, "data.tar.gz").unwrap().len() % 10240, 0);
    }
}
