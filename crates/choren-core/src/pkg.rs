//! Application packages (`.pkg`): a ZIP of `manifest.xml`, the behavior folders and the project
//! resources, which NAOqi's PackageManager installs.

use std::collections::BTreeMap;
use std::io::{Read, Write};

/// Writes a deflated ZIP (UTF-8 names, 1980-01-01 timestamps, so equal input gives equal bytes).
pub fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let (mut out, mut central) = (vec![], vec![]);
    for (name, data) in files {
        let mut z = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
        z.write_all(data).unwrap();
        let packed = z.finish().unwrap();
        let mut crc = flate2::Crc::new();
        crc.update(data);
        // version 2.0, UTF-8 flag, deflate, time 0, date 1980-01-01, crc, sizes, name length
        let common = [&20u16.to_le_bytes()[..], &0x0800u16.to_le_bytes(), &8u16.to_le_bytes(), &0u16.to_le_bytes(), &0x21u16.to_le_bytes(), &crc.sum().to_le_bytes(), &(packed.len() as u32).to_le_bytes(), &(data.len() as u32).to_le_bytes(), &(name.len() as u16).to_le_bytes()].concat();
        central.extend([&0x0201_4b50u32.to_le_bytes()[..], &20u16.to_le_bytes(), &common, &[0; 12], &(out.len() as u32).to_le_bytes(), name.as_bytes()].concat());
        out.extend([&0x0403_4b50u32.to_le_bytes()[..], &common, &[0; 2], name.as_bytes(), &packed].concat());
    }
    let (at, size, n) = (out.len() as u32, central.len() as u32, files.len() as u16);
    out.extend(central);
    out.extend([&0x0605_4b50u32.to_le_bytes()[..], &[0; 4], &n.to_le_bytes(), &n.to_le_bytes(), &size.to_le_bytes(), &at.to_le_bytes(), &[0; 2]].concat());
    out
}

/// What an unpacked package may hold in all (packages can come from any client of a robot).
/// shortcut: apps with more media than this are refused; raise it if a real app needs more.
pub const MAX_UNPACKED: usize = 256 << 20;

/// Reads every file of a ZIP (stored or deflated); names must stay inside the package, each entry
/// must inflate to its declared size and CRC, and the total stays under `MAX_UNPACKED`.
pub fn unzip(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = || "Not a valid package (ZIP) file".to_string();
    let u16_at = |i: usize| zip.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(bad);
    let u32_at = |i: usize| zip.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(bad);
    let end = (0..zip.len().saturating_sub(21)).rev().find(|&i| zip[i..].starts_with(&0x0605_4b50u32.to_le_bytes())).ok_or_else(bad)?;
    let (count, mut p) = (u16_at(end + 10)?, u32_at(end + 16)?);
    let (mut files, mut total) = (vec![], 0usize);
    for _ in 0..count {
        if u32_at(p)? != 0x0201_4b50 {
            return Err(bad());
        }
        let (method, crc, size, raw_size, name_len) = (u16_at(p + 10)?, u32_at(p + 16)?, u32_at(p + 20)?, u32_at(p + 24)?, u16_at(p + 28)?);
        let name = String::from_utf8_lossy(zip.get(p + 46..p + 46 + name_len).ok_or_else(bad)?).into_owned();
        let local = u32_at(p + 42)?;
        p += 46 + name_len + u16_at(p + 30)? + u16_at(p + 32)?;
        if name.ends_with('/') {
            continue;
        }
        let name = crate::project::relative(&name)?;
        total = total.saturating_add(raw_size);
        if total > MAX_UNPACKED {
            return Err("The package is too large once unpacked.".into());
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let packed = zip.get(start..start.saturating_add(size)).ok_or_else(bad)?;
        let corrupt = || format!("Corrupt package entry {name}");
        let data = match method {
            0 => packed.to_vec(),
            8 => {
                let mut d = Vec::with_capacity(raw_size.min(1 << 26));
                flate2::read::DeflateDecoder::new(packed).take(raw_size as u64 + 1).read_to_end(&mut d).map_err(|e| format!("{}: {e}", corrupt()))?;
                d
            }
            m => return Err(format!("Unsupported compression method {m} for {name}")),
        };
        let mut sum = flate2::Crc::new();
        sum.update(&data);
        if data.len() != raw_size || sum.sum() as usize != crc {
            return Err(corrupt());
        }
        files.push((name, data));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_round_trip_and_path_checks() {
        let files = BTreeMap::from([("manifest.xml".to_string(), b"<package/>".to_vec()), ("behavior_1/behavior.xar".into(), vec![7; 5000]), ("empty.txt".into(), vec![])]);
        let z = zip(&files);
        assert_eq!(z, zip(&files), "deterministic");
        assert_eq!(unzip(&z).unwrap(), files.into_iter().collect::<Vec<_>>());
        let evil = zip(&BTreeMap::from([("../x".to_string(), vec![1])]));
        assert!(unzip(&evil).is_err());
        assert!(unzip(b"not a zip").is_err());
        // An entry that inflates past its declared size (a zip bomb) or fails its CRC is refused.
        let mut lying = zip(&BTreeMap::from([("big".to_string(), vec![0u8; 100_000])]));
        let central = lying.len() - 22 - 46 - 3;
        lying[central + 24..central + 28].copy_from_slice(&10u32.to_le_bytes());
        assert!(unzip(&lying).unwrap_err().contains("Corrupt"));
        let mut bad_crc = zip(&BTreeMap::from([("a".to_string(), vec![1, 2, 3])]));
        let central = bad_crc.len() - 22 - 46 - 1;
        bad_crc[central + 16] ^= 1;
        assert!(unzip(&bad_crc).is_err());
    }
}
