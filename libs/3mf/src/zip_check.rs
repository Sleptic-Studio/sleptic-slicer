// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Checks a ZIP archive's own structure before anything in it is unpacked.
//!
//! A ZIP archive describes every entry twice: in a local header just before
//! the entry's data, and again in the central directory at the end. Nothing
//! in the format forces the two to agree, and programs differ in which one
//! they read, so a crafted archive can show one program different entries
//! than another. Bytes the directory does not list (a hidden entry, a second
//! archive glued on) and archives split over several disks are the same kind
//! of problem. A package is only read when every reading agrees:
//!
//! - one end-of-directory record, closing the file, on a single disk (with
//!   its ZIP64 record and locator, when sizes need them, right before it);
//! - the directory ends exactly where those records begin;
//! - each listed entry has a local header that says the same as the
//!   directory (name, flags, method, CRC and sizes, the last two possibly
//!   in a data descriptor after the data);
//! - the listed entries, header to end of data, follow one another from the
//!   first byte of the file to the start of the directory, with no gap and
//!   no overlap;
//! - no entry is encrypted, no two share a name, and no Unicode Path extra
//!   field names an entry differently from its header.
//!
//! The format is ZIP as described in PKWARE's APPNOTE.TXT.

use std::fmt;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const DIRECTORY_ENTRY: u32 = 0x0201_4b50;
const END_RECORD: u32 = 0x0605_4b50;
const ZIP64_END_RECORD: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const DATA_DESCRIPTOR: u32 = 0x0807_4b50;

/// Extra field ids: ZIP64 sizes and offsets, and Info-ZIP's Unicode Path.
const ZIP64_EXTRA: u16 = 0x0001;
const UNICODE_PATH_EXTRA: u16 = 0x7075;

/// General purpose flags: encrypted, sizes in a data descriptor, strong
/// encryption.
const FLAG_ENCRYPTED: u16 = 1;
const FLAG_DESCRIPTOR: u16 = 1 << 3;
const FLAG_STRONG_ENCRYPTION: u16 = 1 << 6;

/// More entries than any real package holds; refusing beyond it keeps a
/// crafted directory from costing memory.
pub const MAX_ENTRIES: usize = 100_000;

/// One listed entry, as both its headers agree it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

/// Why an archive's structure was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipStructureError {
    /// No end-of-directory record closes the file.
    NoEndRecord,
    /// Two end-of-directory records both fit, so programs could disagree
    /// about which archive the file is.
    SeveralEndRecords,
    /// The archive says it spans several disks.
    MultiDisk,
    /// A record or header runs past the end of the file or cannot be read.
    Truncated { what: &'static str },
    /// The directory does not end where the end records begin.
    DirectoryMisplaced,
    /// The directory lists a different number of entries than it holds.
    EntryCountMismatch { declared: u64, found: u64 },
    /// More than `MAX_ENTRIES` entries.
    TooManyEntries,
    /// An entry's name is not UTF-8.
    NameNotUtf8 { index: usize },
    /// Two entries share a name.
    DuplicateName { name: String },
    /// An entry is encrypted, which no package uses.
    Encrypted { name: String },
    /// An entry's local header is missing where the directory points.
    LocalHeaderMissing { name: String },
    /// An entry's two descriptions disagree about `field`.
    HeadersDisagree { name: String, field: &'static str },
    /// An entry's Unicode Path names it differently from its header.
    UnicodePathDisagrees { name: String },
    /// Bytes no listed entry accounts for (a hidden entry, or another
    /// archive glued on), starting at `offset`.
    UnlistedBytes { offset: u64, length: u64 },
    /// Two entries' bytes overlap, the second starting at `offset`.
    OverlappingEntries { offset: u64 },
}

impl fmt::Display for ZipStructureError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEndRecord => out.write_str(
                "the file does not end like a ZIP archive (no end-of-directory record); it is not a ZIP file or it was cut short",
            ),
            Self::SeveralEndRecords => out.write_str(
                "the file holds two ZIP end records, so programs could read it as different archives",
            ),
            Self::MultiDisk => out.write_str("the archive says it is split over several disks, which packages never are"),
            Self::Truncated { what } => write!(out, "the archive is damaged: its {what} runs past the end of the file"),
            Self::DirectoryMisplaced => {
                out.write_str("the archive is damaged: its file index does not end where its end record begins")
            }
            Self::EntryCountMismatch { declared, found } => {
                write!(out, "the archive is damaged: its end record counts {declared} entries but its index holds {found}")
            }
            Self::TooManyEntries => write!(out, "the archive lists more than {MAX_ENTRIES} entries"),
            Self::NameNotUtf8 { index } => write!(out, "entry {index} of the archive has a name that is not valid UTF-8"),
            Self::DuplicateName { name } => write!(out, "the archive holds two entries named \"{name}\""),
            Self::Encrypted { name } => write!(out, "entry \"{name}\" is encrypted, which a package never is"),
            Self::LocalHeaderMissing { name } => {
                write!(out, "the archive is damaged: entry \"{name}\" is listed but its data is not where the index says")
            }
            Self::HeadersDisagree { name, field } => write!(
                out,
                "entry \"{name}\" is described twice with a different {field}, so programs could read different contents"
            ),
            Self::UnicodePathDisagrees { name } => {
                write!(out, "entry \"{name}\" carries a second, different name, so programs could read it under either")
            }
            Self::UnlistedBytes { offset, length } => write!(
                out,
                "the archive holds {length} bytes at offset {offset} that its index does not list (a hidden entry or a second archive glued on)"
            ),
            Self::OverlappingEntries { offset } => {
                write!(out, "the archive is damaged: two entries share the bytes at offset {offset}")
            }
        }
    }
}

impl std::error::Error for ZipStructureError {}

/// Little-endian reads that fail instead of panicking past the end.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn slice(&self, at: u64, len: u64, what: &'static str) -> Result<&[u8], ZipStructureError> {
        let start = usize::try_from(at).ok();
        let end = at.checked_add(len).and_then(|e| usize::try_from(e).ok());
        match (start, end) {
            (Some(s), Some(e)) if e <= self.0.len() => Ok(&self.0[s..e]),
            _ => Err(ZipStructureError::Truncated { what }),
        }
    }
    fn u16(&self, at: u64, what: &'static str) -> Result<u16, ZipStructureError> {
        let b = self.slice(at, 2, what)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&self, at: u64, what: &'static str) -> Result<u32, ZipStructureError> {
        let b = self.slice(at, 4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&self, at: u64, what: &'static str) -> Result<u64, ZipStructureError> {
        let b = self.slice(at, 8, what)?;
        Ok(u64::from_le_bytes(b.try_into().expect("eight bytes")))
    }
}

/// The extra field `id` among `extra`'s (id, length, data) records.
fn extra_field(extra: &[u8], id: u16) -> Option<&[u8]> {
    let mut rest = extra;
    while rest.len() >= 4 {
        let field = u16::from_le_bytes([rest[0], rest[1]]);
        let len = usize::from(u16::from_le_bytes([rest[2], rest[3]]));
        let body = rest.get(4..4 + len)?;
        if field == id {
            return Some(body);
        }
        rest = &rest[4 + len..];
    }
    None
}

/// The sizes (and directory offset) from a ZIP64 extra field: each value
/// whose 32-bit field is saturated is read from it, in the order
/// uncompressed size, compressed size, offset.
fn widen(
    extra: &[u8],
    (uncompressed, compressed, offset): (u32, u32, Option<u32>),
) -> Result<(u64, u64, u64), &'static str> {
    let saturated = |v: u32| v == u32::MAX;
    let needs = saturated(uncompressed) || saturated(compressed) || offset.is_some_and(saturated);
    if !needs {
        return Ok((u64::from(uncompressed), u64::from(compressed), u64::from(offset.unwrap_or(0))));
    }
    let body = extra_field(extra, ZIP64_EXTRA).ok_or("ZIP64 sizes")?;
    let mut values = body.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().expect("eight bytes")));
    let mut take = |v: u32| if saturated(v) { values.next().ok_or("ZIP64 sizes") } else { Ok(u64::from(v)) };
    let u = take(uncompressed)?;
    let c = take(compressed)?;
    let o = match offset {
        Some(o) => take(o)?,
        None => 0,
    };
    Ok((u, c, o))
}

struct Listed {
    name: String,
    name_bytes: Vec<u8>,
    flags: u16,
    method: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    offset: u64,
    zip64: bool,
}

/// Checks `archive`'s structure (see the module doc) and lists its entries
/// in directory order.
pub fn check_zip_structure(archive: &[u8]) -> Result<Vec<CheckedEntry>, ZipStructureError> {
    use ZipStructureError::*;
    let file = Bytes(archive);
    let size = archive.len() as u64;

    // The end record is 22 bytes plus a comment of at most 65535; it must
    // close the file exactly, its comment running to the last byte.
    // A second record that would also fit (one hidden in the other's
    // comment) makes the archive ambiguous: programs pick either.
    let earliest = size.saturating_sub(22 + 0xFFFF);
    let mut found = Vec::new();
    let mut at = size.checked_sub(22).ok_or(NoEndRecord)?;
    loop {
        if file.u32(at, "end record")? == END_RECORD && at + 22 + u64::from(file.u16(at + 20, "end record")?) == size {
            found.push(at);
        }
        if at == earliest {
            break;
        }
        at -= 1;
    }
    let end = match found.as_slice() {
        [] => return Err(NoEndRecord),
        [only] => *only,
        _ => return Err(SeveralEndRecords),
    };
    let disk = file.u16(end + 4, "end record")?;
    let directory_disk = file.u16(end + 6, "end record")?;
    let on_disk = file.u16(end + 8, "end record")?;
    let total16 = file.u16(end + 10, "end record")?;
    let size32 = file.u32(end + 12, "end record")?;
    let offset32 = file.u32(end + 16, "end record")?;
    let wide = [on_disk, total16].contains(&u16::MAX) || [size32, offset32].contains(&u32::MAX);
    let (total, directory_size, directory_offset, directory_end) = if wide {
        let locator = end.checked_sub(20).ok_or(Truncated { what: "ZIP64 locator" })?;
        if file.u32(locator, "ZIP64 locator")? != ZIP64_LOCATOR {
            return Err(Truncated { what: "ZIP64 locator" });
        }
        let record = file.u64(locator + 8, "ZIP64 locator")?;
        if file.u32(locator + 4, "ZIP64 locator")? != 0 || file.u32(locator + 16, "ZIP64 locator")? > 1 {
            return Err(MultiDisk);
        }
        if file.u32(record, "ZIP64 end record")? != ZIP64_END_RECORD {
            return Err(Truncated { what: "ZIP64 end record" });
        }
        let length = file.u64(record + 4, "ZIP64 end record")?;
        if record.checked_add(12).and_then(|r| r.checked_add(length)) != Some(locator) {
            return Err(DirectoryMisplaced);
        }
        if file.u32(record + 16, "ZIP64 end record")? != 0 || file.u32(record + 20, "ZIP64 end record")? != 0 {
            return Err(MultiDisk);
        }
        let here = file.u64(record + 24, "ZIP64 end record")?;
        let total = file.u64(record + 32, "ZIP64 end record")?;
        if here != total {
            return Err(MultiDisk);
        }
        (total, file.u64(record + 40, "ZIP64 end record")?, file.u64(record + 48, "ZIP64 end record")?, record)
    } else {
        if on_disk != total16 {
            return Err(MultiDisk);
        }
        (u64::from(total16), u64::from(size32), u64::from(offset32), end)
    };
    if disk != 0 || directory_disk != 0 {
        return Err(MultiDisk);
    }
    if directory_offset.checked_add(directory_size) != Some(directory_end) {
        return Err(DirectoryMisplaced);
    }
    if total > MAX_ENTRIES as u64 {
        return Err(TooManyEntries);
    }

    // The directory, entry by entry.
    let mut listed = Vec::with_capacity(total as usize);
    let mut at = directory_offset;
    while at < directory_end {
        if listed.len() >= MAX_ENTRIES {
            return Err(TooManyEntries);
        }
        if file.u32(at, "file index")? != DIRECTORY_ENTRY {
            return Err(DirectoryMisplaced);
        }
        let flags = file.u16(at + 8, "file index")?;
        let method = file.u16(at + 10, "file index")?;
        let crc = file.u32(at + 16, "file index")?;
        let compressed32 = file.u32(at + 20, "file index")?;
        let uncompressed32 = file.u32(at + 24, "file index")?;
        let name_len = u64::from(file.u16(at + 28, "file index")?);
        let extra_len = u64::from(file.u16(at + 30, "file index")?);
        let comment_len = u64::from(file.u16(at + 32, "file index")?);
        let start_disk = file.u16(at + 34, "file index")?;
        let offset32 = file.u32(at + 42, "file index")?;
        let name_bytes = file.slice(at + 46, name_len, "file index")?.to_vec();
        let extra = file.slice(at + 46 + name_len, extra_len, "file index")?;
        let index = listed.len();
        let name = String::from_utf8(name_bytes.clone()).map_err(|_| NameNotUtf8 { index })?;
        let (uncompressed, compressed, offset) =
            widen(extra, (uncompressed32, compressed32, Some(offset32))).map_err(|what| Truncated { what })?;
        let zip64 = [uncompressed32, compressed32, offset32].contains(&u32::MAX);
        if start_disk != 0 && start_disk != u16::MAX {
            return Err(MultiDisk);
        }
        if flags & (FLAG_ENCRYPTED | FLAG_STRONG_ENCRYPTION) != 0 {
            return Err(Encrypted { name });
        }
        if let Some(unicode) = extra_field(extra, UNICODE_PATH_EXTRA)
            && unicode.get(5..) != Some(name_bytes.as_slice())
        {
            return Err(UnicodePathDisagrees { name });
        }
        listed.push(Listed { name, name_bytes, flags, method, crc, compressed, uncompressed, offset, zip64 });
        at = at + 46 + name_len + extra_len + comment_len;
    }
    if at != directory_end {
        return Err(DirectoryMisplaced);
    }
    if listed.len() as u64 != total {
        return Err(EntryCountMismatch { declared: total, found: listed.len() as u64 });
    }
    let mut names = std::collections::HashSet::with_capacity(listed.len());
    for entry in &listed {
        if !names.insert(entry.name.as_str()) {
            return Err(DuplicateName { name: entry.name.clone() });
        }
    }

    // Each entry's local header and the bytes it covers.
    let mut spans = Vec::with_capacity(listed.len());
    for entry in &listed {
        let at = entry.offset;
        let missing = || LocalHeaderMissing { name: entry.name.clone() };
        if file.u32(at, "entry header").map_err(|_| missing())? != LOCAL_HEADER {
            return Err(missing());
        }
        let disagree = |field| HeadersDisagree { name: entry.name.clone(), field };
        let flags = file.u16(at + 6, "entry header")?;
        let method = file.u16(at + 8, "entry header")?;
        let crc = file.u32(at + 14, "entry header")?;
        let compressed32 = file.u32(at + 18, "entry header")?;
        let uncompressed32 = file.u32(at + 22, "entry header")?;
        let name_len = u64::from(file.u16(at + 26, "entry header")?);
        let extra_len = u64::from(file.u16(at + 28, "entry header")?);
        if file.slice(at + 30, name_len, "entry header")? != entry.name_bytes.as_slice() {
            return Err(disagree("name"));
        }
        let extra = file.slice(at + 30 + name_len, extra_len, "entry header")?;
        if let Some(unicode) = extra_field(extra, UNICODE_PATH_EXTRA)
            && unicode.get(5..) != Some(entry.name_bytes.as_slice())
        {
            return Err(UnicodePathDisagrees { name: entry.name.clone() });
        }
        if flags != entry.flags {
            return Err(disagree("set of flags"));
        }
        if method != entry.method {
            return Err(disagree("compression method"));
        }
        let data_start = at + 30 + name_len + extra_len;
        let data_end = data_start.checked_add(entry.compressed).ok_or(Truncated { what: "entry data" })?;
        let mut span_end = data_end;
        let (local_u, local_c, _) = widen(extra, (uncompressed32, compressed32, None)).map_err(|what| Truncated { what })?;
        let header_says = (crc, local_c, local_u);
        let index_says = (entry.crc, entry.compressed, entry.uncompressed);
        if flags & FLAG_DESCRIPTOR != 0 {
            // Sizes and CRC follow the data, after an optional signature;
            // the header itself holds zeros or the same values.
            if header_says != (0, 0, 0) && header_says != index_says {
                return Err(disagree("CRC or size"));
            }
            let mut d = data_end;
            if file.u32(d, "data descriptor")? == DATA_DESCRIPTOR {
                d += 4;
            }
            let wide = entry.zip64 || extra_field(extra, ZIP64_EXTRA).is_some();
            let (c, u, len) = if wide {
                (file.u64(d + 4, "data descriptor")?, file.u64(d + 12, "data descriptor")?, 20)
            } else {
                (u64::from(file.u32(d + 4, "data descriptor")?), u64::from(file.u32(d + 8, "data descriptor")?), 12)
            };
            if file.u32(d, "data descriptor")? != entry.crc {
                return Err(disagree("CRC"));
            }
            if (c, u) != (entry.compressed, entry.uncompressed) {
                return Err(disagree("size"));
            }
            span_end = d + len;
        } else if crc != entry.crc {
            return Err(disagree("CRC"));
        } else if (local_c, local_u) != (entry.compressed, entry.uncompressed) {
            return Err(disagree("size"));
        }
        if span_end > directory_offset {
            return Err(OverlappingEntries { offset: at });
        }
        spans.push((at, span_end));
    }

    // Listed bytes tile the file from its start to the directory.
    spans.sort_unstable();
    let mut covered = 0u64;
    for (start, stop) in spans {
        if start > covered {
            return Err(UnlistedBytes { offset: covered, length: start - covered });
        }
        if start < covered {
            return Err(OverlappingEntries { offset: start });
        }
        covered = stop;
    }
    if covered != directory_offset {
        return Err(UnlistedBytes { offset: covered, length: directory_offset - covered });
    }
    Ok(listed
        .into_iter()
        .map(|e| CheckedEntry { name: e.name, compressed_size: e.compressed, uncompressed_size: e.uncompressed })
        .collect())
}
