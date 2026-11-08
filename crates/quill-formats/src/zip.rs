//! Just enough ZIP for .docx: read stored and deflated entries, write
//! deflated ones. No encryption, no ZIP64, no multi-disk archives.

use std::{cell::Cell, fmt};

const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
/// More than a document within `quill_core::limits` writes: its pictures
/// (`MAX_PICTURES`), each header and footer (`MAX_HEADERS`) with its list
/// of pictures, and the dozen or so parts every document has.
const MAX_ENTRIES: usize = 16_384;
const MAX_ENTRY_BYTES: usize = 512 * 1024 * 1024;
const MAX_EXPANDED_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct ZipError(pub String);

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ZipError {}

fn err<T>(msg: &str) -> Result<T, ZipError> {
    Err(ZipError(msg.to_string()))
}

pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *v = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    d.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    offset: usize,
}

/// A ZIP archive held in memory.
pub struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
    expanded_read: Cell<usize>,
}

impl<'a> Archive<'a> {
    pub fn open(data: &'a [u8]) -> Result<Archive<'a>, ZipError> {
        if data.len() > MAX_ARCHIVE_BYTES {
            return err("zip file is too large");
        }
        // The end-of-central-directory record is in the last 64 KB.
        let start = data.len().saturating_sub(65_557);
        let eocd = (start..data.len().saturating_sub(21))
            .rev()
            .find(|&i| u32_at(data, i) == Some(0x0605_4b50));
        let Some(eocd) = eocd else {
            return err("not a zip file");
        };
        let field16 = |offset| {
            u16_at(data, eocd + offset).ok_or_else(|| ZipError("damaged zip directory".into()))
        };
        let field32 = |offset| {
            u32_at(data, eocd + offset).ok_or_else(|| ZipError("damaged zip directory".into()))
        };
        if field16(4)? != 0 || field16(6)? != 0 || field16(8)? != field16(10)? {
            return err("multi-disk zip archives are not supported");
        }
        let count = field16(10)? as usize;
        if count > MAX_ENTRIES {
            return err("zip file has too many entries");
        }
        let directory_size = field32(12)? as usize;
        let directory_offset = field32(16)? as usize;
        let directory_end = directory_offset
            .checked_add(directory_size)
            .ok_or_else(|| ZipError("damaged zip directory".into()))?;
        if directory_end > eocd {
            return err("damaged zip directory");
        }
        let comment_len = field16(20)? as usize;
        let eocd_end = eocd
            .checked_add(22)
            .and_then(|n| n.checked_add(comment_len))
            .ok_or_else(|| ZipError("damaged zip directory".into()))?;
        if eocd_end > data.len() {
            return err("damaged zip directory");
        }

        let mut pos = directory_offset;
        let mut expanded = 0usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(data, pos) != Some(0x0201_4b50) {
                return err("damaged zip directory");
            }
            let header_end = pos
                .checked_add(46)
                .filter(|&end| end <= directory_end)
                .ok_or_else(|| ZipError("damaged zip directory".into()))?;
            let at16 = |o| {
                pos.checked_add(o)
                    .and_then(|at| u16_at(data, at))
                    .ok_or_else(|| ZipError("damaged zip directory".into()))
            };
            let at32 = |o| {
                pos.checked_add(o)
                    .and_then(|at| u32_at(data, at))
                    .ok_or_else(|| ZipError("damaged zip directory".into()))
            };
            let flags = at16(8)?;
            if flags & 1 != 0 {
                return err("encrypted zip entries are not supported");
            }
            let method = at16(10)?;
            let crc = at32(16)?;
            let compressed = at32(20)? as usize;
            let size = at32(24)? as usize;
            let name_len = at16(28)? as usize;
            let extra_len = at16(30)? as usize;
            let comment_len = at16(32)? as usize;
            if at16(34)? != 0 {
                return err("multi-disk zip archives are not supported");
            }
            let offset = at32(42)? as usize;
            if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
                return err("ZIP64 archives are not supported");
            }
            if size > MAX_ENTRY_BYTES {
                return err("zip entry is too large");
            }
            expanded = expanded
                .checked_add(size)
                .filter(|&n| n <= MAX_EXPANDED_BYTES)
                .ok_or_else(|| ZipError("zip expands to too much data".into()))?;
            let name_end = header_end
                .checked_add(name_len)
                .ok_or_else(|| ZipError("damaged zip directory".into()))?;
            let entry_end = name_end
                .checked_add(extra_len)
                .and_then(|n| n.checked_add(comment_len))
                .filter(|&end| end <= directory_end)
                .ok_or_else(|| ZipError("damaged zip directory".into()))?;
            let Some(name) = data.get(header_end..name_end) else {
                return err("damaged zip directory");
            };
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                method,
                crc,
                compressed,
                size,
                offset,
            });
            pos = entry_end;
        }
        Ok(Archive {
            data,
            entries,
            expanded_read: Cell::new(0),
        })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The contents of `name`, or None if the archive has no such entry.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ZipError> {
        let Some(e) = self
            .entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
        else {
            return Ok(None);
        };
        let expanded = self
            .expanded_read
            .get()
            .checked_add(e.size)
            .filter(|&n| n <= MAX_EXPANDED_BYTES)
            .ok_or_else(|| ZipError("zip has expanded to too much data".into()))?;
        self.expanded_read.set(expanded);
        let d = self.data;
        if u32_at(d, e.offset) != Some(0x0403_4b50) {
            return err("damaged zip entry");
        }
        let local = |o| {
            e.offset
                .checked_add(o)
                .ok_or_else(|| ZipError("damaged zip entry".into()))
        };
        let flags = u16_at(d, local(6)?).ok_or_else(|| ZipError("damaged zip entry".into()))?;
        let method = u16_at(d, local(8)?).ok_or_else(|| ZipError("damaged zip entry".into()))?;
        if flags & 1 != 0 || method != e.method {
            return err("damaged zip entry");
        }
        let name_len =
            u16_at(d, local(26)?).ok_or_else(|| ZipError("damaged zip entry".into()))? as usize;
        let extra_len =
            u16_at(d, local(28)?).ok_or_else(|| ZipError("damaged zip entry".into()))? as usize;
        let start = e
            .offset
            .checked_add(30)
            .and_then(|n| n.checked_add(name_len))
            .and_then(|n| n.checked_add(extra_len))
            .ok_or_else(|| ZipError("damaged zip entry".into()))?;
        let end = start
            .checked_add(e.compressed)
            .ok_or_else(|| ZipError("damaged zip entry".into()))?;
        let Some(raw) = d.get(start..end) else {
            return err("truncated zip entry");
        };
        let out = match e.method {
            0 if raw.len() == e.size => raw.to_vec(),
            0 => return err("zip entry has the wrong size"),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, e.size.max(1))
                .map_err(|_| ZipError("damaged compressed data".into()))?,
            m => return Err(ZipError(format!("unsupported compression method {m}"))),
        };
        if out.len() != e.size {
            return err("zip entry has the wrong size");
        }
        if crc32(&out) != e.crc {
            return Err(ZipError(format!(
                "{} is damaged (its checksum is wrong)",
                e.name
            )));
        }
        Ok(Some(out))
    }
}

/// Builds a ZIP archive in memory.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    time: u16,
    date: u16,
}

impl Writer {
    /// A new archive whose entries carry `secs` (Unix time) as their date.
    pub fn new(secs: u64) -> Writer {
        let (time, date) = dos_time(secs);
        Writer {
            time,
            date,
            ..Default::default()
        }
    }

    pub fn add(&mut self, name: &str, data: &[u8]) {
        let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
        let (method, body): (u16, &[u8]) = if deflated.len() < data.len() {
            (8, &deflated)
        } else {
            (0, data)
        };
        self.add_body(name, data, method, body);
    }

    /// ODF requires its first mimetype member to be stored, without compression.
    pub fn add_stored(&mut self, name: &str, data: &[u8]) {
        self.add_body(name, data, 0, data);
    }

    fn add_body(&mut self, name: &str, data: &[u8], method: u16, body: &[u8]) {
        let crc = crc32(data);
        let offset = self.out.len() as u32;
        let name_b = name.as_bytes();
        let mut header = Vec::with_capacity(30 + name_b.len());
        header.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        header.extend_from_slice(&20u16.to_le_bytes()); // version needed
        header.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        header.extend_from_slice(&method.to_le_bytes());
        header.extend_from_slice(&self.time.to_le_bytes());
        header.extend_from_slice(&self.date.to_le_bytes());
        header.extend_from_slice(&crc.to_le_bytes());
        header.extend_from_slice(&(body.len() as u32).to_le_bytes());
        header.extend_from_slice(&(data.len() as u32).to_le_bytes());
        header.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(name_b);
        self.out.extend_from_slice(&header);
        self.out.extend_from_slice(body);

        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes()); // made by
        c.extend_from_slice(&header[4..30]); // same fields as the local header
        c.extend_from_slice(&0u16.to_le_bytes()); // comment length
        c.extend_from_slice(&0u16.to_le_bytes()); // disk
        c.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        c.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name_b);
        self.count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_offset = self.out.len() as u32;
        let cd_size = self.central.len() as u32;
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&self.count.to_le_bytes());
        self.out.extend_from_slice(&cd_size.to_le_bytes());
        self.out.extend_from_slice(&cd_offset.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

/// Unix seconds to MS-DOS time and date (UTC; ZIP has no time zones).
fn dos_time(secs: u64) -> (u16, u16) {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let s = secs % 86_400;
    let time = (((s / 3600) << 11) | ((s % 3600 / 60) << 5) | ((s % 60) / 2)) as u16;
    let y = (y - 1980).clamp(0, 127) as u16;
    (time, y << 9 | (m as u16) << 5 | d as u16)
}

pub use quill_core::revision::civil_from_days;

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(bytes: &[u8], sig: u32) -> usize {
        let sig = sig.to_le_bytes();
        bytes.windows(4).position(|w| w == sig).unwrap()
    }

    #[test]
    fn crc_of_known_string() {
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn round_trip() {
        let mut w = Writer::new(1_760_000_000);
        let big = "word ".repeat(1000);
        w.add("a.txt", b"hello");
        w.add("dir/b.xml", big.as_bytes());
        let bytes = w.finish();
        let a = Archive::open(&bytes).unwrap();
        assert_eq!(a.names().collect::<Vec<_>>(), ["a.txt", "dir/b.xml"]);
        assert_eq!(a.read("a.txt").unwrap().unwrap(), b"hello");
        assert_eq!(a.read("dir/b.xml").unwrap().unwrap(), big.as_bytes());
        assert!(a.read("missing").unwrap().is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Archive::open(b"not a zip at all, just some bytes").is_err());
    }

    #[test]
    fn rejects_too_many_entries_before_allocating_for_them() {
        let mut bytes = Writer::new(0).finish();
        let eocd = signature(&bytes, 0x0605_4b50);
        let count = (MAX_ENTRIES as u16 + 1).to_le_bytes();
        bytes[eocd + 8..eocd + 10].copy_from_slice(&count);
        bytes[eocd + 10..eocd + 12].copy_from_slice(&count);

        let error = Archive::open(&bytes).err().unwrap().to_string();

        assert!(error.contains("too many entries"), "{error}");
    }

    #[test]
    fn rejects_oversized_entries_and_archives() {
        let mut one = Writer::new(0);
        one.add("one", b"x");
        let mut bytes = one.finish();
        let central = signature(&bytes, 0x0201_4b50);
        bytes[central + 24..central + 28]
            .copy_from_slice(&((MAX_ENTRY_BYTES + 1) as u32).to_le_bytes());
        let error = Archive::open(&bytes).err().unwrap().to_string();
        assert!(error.contains("entry is too large"), "{error}");

        let mut many = Writer::new(0);
        for n in 0..5 {
            many.add(&format!("{n}"), b"x");
        }
        let mut bytes = many.finish();
        let mut pos = signature(&bytes, 0x0201_4b50);
        for _ in 0..5 {
            bytes[pos + 24..pos + 28].copy_from_slice(&(MAX_ENTRY_BYTES as u32).to_le_bytes());
            let name = u16_at(&bytes, pos + 28).unwrap() as usize;
            let extra = u16_at(&bytes, pos + 30).unwrap() as usize;
            let comment = u16_at(&bytes, pos + 32).unwrap() as usize;
            pos += 46 + name + extra + comment;
        }
        let error = Archive::open(&bytes).err().unwrap().to_string();
        assert!(error.contains("too much data"), "{error}");
    }

    #[test]
    fn rejects_declared_size_mismatches_and_bad_offsets() {
        let mut writer = Writer::new(0);
        writer.add("a.txt", b"hello");
        let original = writer.finish();
        let central = signature(&original, 0x0201_4b50);

        let mut wrong_size = original.clone();
        wrong_size[central + 24..central + 28].copy_from_slice(&4u32.to_le_bytes());
        let archive = Archive::open(&wrong_size).unwrap();
        let error = archive.read("a.txt").unwrap_err().to_string();
        assert!(error.contains("wrong size"), "{error}");

        let mut bad_offset = original;
        bad_offset[central + 42..central + 46].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        let archive = Archive::open(&bad_offset).unwrap();
        assert!(archive.read("a.txt").is_err());
    }

    #[test]
    fn limits_repeated_reads_as_well_as_declared_archive_size() {
        let mut writer = Writer::new(0);
        writer.add("a.txt", b"hello");
        let bytes = writer.finish();
        let archive = Archive::open(&bytes).unwrap();
        archive.expanded_read.set(MAX_EXPANDED_BYTES - 4);

        let error = archive.read("a.txt").unwrap_err().to_string();

        assert!(error.contains("too much data"), "{error}");
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_374), (2025, 10, 13));
    }
}
