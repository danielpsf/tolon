//! A minimal zip writer for the diagnostics file (L-072): deflate (flate2) and CRC-32 (crc32fast),
//! no zip64, no encryption, no extra fields. The `zip` crate stays out (L-019).
//!
//! Layout (PKWARE APPNOTE 4.3): for each entry a local file header and its compressed data, then
//! the central directory, then the end-of-central-directory record. Sizes are known before each
//! header is written, so no data descriptor is needed.

use std::io::{self, Write};

use chrono::{Datelike, NaiveDateTime, Timelike};
use flate2::write::DeflateEncoder;
use flate2::Compression;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
/// 2.0: deflate.
const VERSION: u16 = 20;
/// Bit 11: the names are UTF-8.
const FLAGS: u16 = 0x0800;
const DEFLATE: u16 = 8;

/// One file in the archive. `name` uses `/` between folders.
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

/// The archive as bytes. Fails only when an entry or the whole is too large for a zip without
/// zip64 (4 GB, 65,535 entries), far above what diagnostics hold.
pub fn write(entries: &[Entry], modified: NaiveDateTime) -> io::Result<Vec<u8>> {
    let (time, date) = dos_time(modified);
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let name = entry.name.as_bytes();
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&entry.data)?;
        let packed = encoder.finish()?;
        let crc = crc32fast::hash(&entry.data);
        let (size, packed_size, name_len) = (
            small(entry.data.len())?,
            small(packed.len())?,
            u16::try_from(name.len()).map_err(|_| too_large())?,
        );
        let offset = small(out.len())?;
        put32(&mut out, LOCAL_HEADER);
        for v in [VERSION, FLAGS, DEFLATE, time, date] {
            put16(&mut out, v);
        }
        for v in [crc, packed_size, size] {
            put32(&mut out, v);
        }
        put16(&mut out, name_len);
        put16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(&packed);

        put32(&mut central, CENTRAL_HEADER);
        for v in [VERSION, VERSION, FLAGS, DEFLATE, time, date] {
            put16(&mut central, v);
        }
        for v in [crc, packed_size, size] {
            put32(&mut central, v);
        }
        // Name length, extra, comment, disk, internal attributes.
        for v in [name_len, 0, 0, 0, 0] {
            put16(&mut central, v);
        }
        // External attributes, then where the local header starts.
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(name);
    }
    let count = u16::try_from(entries.len()).map_err(|_| too_large())?;
    let (central_size, central_at) = (small(central.len())?, small(out.len())?);
    out.extend_from_slice(&central);
    put32(&mut out, END_OF_CENTRAL);
    for v in [0, 0, count, count] {
        put16(&mut out, v);
    }
    put32(&mut out, central_size);
    put32(&mut out, central_at);
    put16(&mut out, 0);
    Ok(out)
}

/// MS-DOS time and date (2-second steps, years 1980 to 2107), as zip stores them.
fn dos_time(t: NaiveDateTime) -> (u16, u16) {
    let year = t.year().clamp(1980, 2107);
    let time = (t.hour() << 11) | (t.minute() << 5) | (t.second() / 2);
    let date = (u32::try_from(year - 1980).unwrap_or(0) << 9) | (t.month() << 5) | t.day();
    (
        u16::try_from(time).unwrap_or(0),
        u16::try_from(date).unwrap_or(0),
    )
}

fn small(n: usize) -> io::Result<u32> {
    u32::try_from(n).map_err(|_| too_large())
}

fn too_large() -> io::Error {
    io::Error::other("too large for a zip without zip64")
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A reader for tests only: walks the central directory, inflates each entry and checks its CRC.
#[cfg(test)]
pub(crate) fn read(bytes: &[u8]) -> anyhow::Result<Vec<Entry>> {
    use anyhow::{anyhow, ensure};
    use std::io::Read;

    let u16_at = |at: usize| -> anyhow::Result<usize> {
        let b = bytes
            .get(at..at + 2)
            .ok_or_else(|| anyhow!("short at {at}"))?;
        Ok(usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let u32_at = |at: usize| -> anyhow::Result<usize> {
        let b = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow!("short at {at}"))?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let end = bytes
        .len()
        .checked_sub(22)
        .ok_or_else(|| anyhow!("no end record"))?;
    ensure!(u32_at(end)? == END_OF_CENTRAL as usize, "no end record");
    let count = u16_at(end + 10)?;
    let mut at = u32_at(end + 16)?;
    let mut entries = Vec::new();
    for _ in 0..count {
        ensure!(u32_at(at)? == CENTRAL_HEADER as usize, "bad central header");
        let (crc, packed, size) = (u32_at(at + 16)?, u32_at(at + 20)?, u32_at(at + 24)?);
        let (name_len, extra, comment) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let local = u32_at(at + 42)?;
        let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec())?;
        ensure!(u32_at(local)? == LOCAL_HEADER as usize, "bad local header");
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let mut data = Vec::new();
        flate2::read::DeflateDecoder::new(&bytes[start..start + packed]).read_to_end(&mut data)?;
        ensure!(data.len() == size, "{name}: size");
        ensure!(crc32fast::hash(&data) as usize == crc, "{name}: crc");
        entries.push(Entry { name, data });
        at += 46 + name_len + extra + comment;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    type TestResult = anyhow::Result<()>;

    fn sample() -> anyhow::Result<Vec<u8>> {
        let at = NaiveDate::from_ymd_opt(2026, 10, 22)
            .and_then(|d| d.and_hms_opt(14, 5, 31))
            .ok_or_else(|| anyhow::anyhow!("date"))?;
        Ok(write(
            &[
                Entry {
                    name: "README.txt".into(),
                    data: b"What is in this file.\n".to_vec(),
                },
                Entry {
                    name: "logs/tolon.2026-10-22.log".into(),
                    data: "a line, \u{00e9}\n".repeat(5000).into_bytes(),
                },
                Entry {
                    name: "empty.json".into(),
                    data: Vec::new(),
                },
            ],
            at,
        )?)
    }

    #[test]
    fn l072_the_zip_reads_back_with_its_names_data_and_crc() -> TestResult {
        let entries = read(&sample()?)?;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["README.txt", "logs/tolon.2026-10-22.log", "empty.json"]
        );
        assert_eq!(entries[0].data, b"What is in this file.\n");
        assert_eq!(entries[1].data.len(), "a line, \u{00e9}\n".len() * 5000);
        assert!(entries[2].data.is_empty());
        Ok(())
    }

    #[test]
    fn l072_dos_time_packs_seconds_in_two_second_steps() -> TestResult {
        let at = NaiveDate::from_ymd_opt(2026, 10, 22)
            .and_then(|d| d.and_hms_opt(14, 5, 31))
            .ok_or_else(|| anyhow::anyhow!("date"))?;
        assert_eq!(
            dos_time(at),
            ((14 << 11) | (5 << 5) | 15, (46 << 9) | (10 << 5) | 22)
        );
        Ok(())
    }

    /// The system tools read it: `unzip -t` (macOS, Linux) and `ditto -x -k` (macOS). Skipped where
    /// neither exists (Windows CI); the Rust reader above runs everywhere.
    #[test]
    fn l072_unzip_and_ditto_read_the_zip() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sample.zip");
        std::fs::write(&path, sample()?)?;
        if let Ok(out) = std::process::Command::new("unzip")
            .arg("-t")
            .arg(&path)
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success(), "unzip -t: {text}");
            assert!(text.contains("No errors detected"), "{text}");
        }
        let into = dir.path().join("out");
        if let Ok(status) = std::process::Command::new("ditto")
            .args(["-x", "-k"])
            .arg(&path)
            .arg(&into)
            .status()
        {
            assert!(status.success(), "ditto -x -k");
            assert_eq!(
                std::fs::read(into.join("README.txt"))?,
                b"What is in this file.\n"
            );
            assert!(into.join("logs").join("tolon.2026-10-22.log").is_file());
        }
        Ok(())
    }
}
