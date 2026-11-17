//! NIF 20.2.0.7 (Skyrim LE/SE) container: header parsed, every block kept as raw bytes.
//!
//! Only the header tables are interpreted. Blocks, the footer and any bytes after it are kept
//! verbatim, so `serialize(parse(x)) == x` for every file that parses. Strings are raw bytes: the
//! exporter left uninitialised (invalid UTF-8) bytes in some vanilla string tables, and those must
//! survive a rewrite unchanged.

use std::fmt;
use std::io::{self, Cursor, Read};

pub const NIF_VERSION: u32 = 0x1402_0007;
pub const MIN_BS_VERSION: u32 = 83;

#[derive(Debug)]
pub enum NifError {
    Io(io::Error),
    InvalidHeader(String),
    UnsupportedVersion { version: u32, bs_version: u32 },
}

impl fmt::Display for NifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NifError::Io(e) => write!(f, "IO error: {e}"),
            NifError::InvalidHeader(got) => {
                write!(f, "Invalid NIF header: Expected 'Gamebryo File Format', got '{got}'")
            }
            NifError::UnsupportedVersion { version, bs_version } => {
                write!(f, "Unsupported NIF version: {version:#010x} (BS version {bs_version})")
            }
        }
    }
}

impl From<io::Error> for NifError {
    fn from(e: io::Error) -> Self {
        NifError::Io(e)
    }
}

#[derive(Clone, Debug)]
pub struct Nif {
    /// Header line including its trailing '\n'.
    pub header_line: Vec<u8>,
    pub version: u32,
    pub endian: u8,
    pub user_version: u32,
    pub bs_version: u32,
    /// Export info (author, process script, export script) as raw bytes, length prefixes included.
    pub export_info: Vec<u8>,
    pub block_types: Vec<Vec<u8>>,
    pub block_type_index: Vec<u16>,
    pub max_string_len: u32,
    pub strings: Vec<Vec<u8>>,
    pub groups: Vec<u32>,
    pub blocks: Vec<Vec<u8>>,
    /// Footer (root count and refs) plus anything after it, raw.
    pub footer: Vec<u8>,
}

struct Reader<'a> {
    c: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Self {
        Reader { c: Cursor::new(d) }
    }
    fn pos(&self) -> usize {
        self.c.position() as usize
    }
    fn remaining(&self) -> usize {
        self.c.get_ref().len().saturating_sub(self.pos())
    }
    fn bytes(&mut self, n: usize) -> io::Result<Vec<u8>> {
        if n > self.remaining() {
            // Same error (and message) read_exact gives, without allocating a huge buffer first.
            let mut sink = [0u8; 1];
            self.c.set_position(self.c.get_ref().len() as u64);
            self.c.read_exact(&mut sink)?;
        }
        let mut v = vec![0u8; n];
        self.c.read_exact(&mut v)?;
        Ok(v)
    }
    fn u8(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.c.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        let mut b = [0u8; 2];
        self.c.read_exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }
    fn u32(&mut self) -> io::Result<u32> {
        let mut b = [0u8; 4];
        self.c.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn sized_string(&mut self) -> io::Result<Vec<u8>> {
        let n = self.u32()? as usize;
        self.bytes(n)
    }
    fn short_string_raw(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        let n = self.u8()?;
        out.push(n);
        out.extend(self.bytes(n as usize)?);
        Ok(())
    }
}

impl Nif {
    pub fn parse(data: &[u8]) -> Result<Nif, NifError> {
        let mut r = Reader::new(data);

        let mut header_line = Vec::new();
        loop {
            let b = r.u8()?;
            header_line.push(b);
            if b == b'\n' {
                break;
            }
        }
        let line = String::from_utf8_lossy(&header_line[..header_line.len() - 1]).into_owned();
        if !line.starts_with("Gamebryo File Format") {
            return Err(NifError::InvalidHeader(line));
        }

        let version = r.u32()?;
        if version != NIF_VERSION {
            return Err(NifError::UnsupportedVersion { version, bs_version: 0 });
        }
        let endian = r.u8()?;
        let user_version = r.u32()?;
        let num_blocks = r.u32()? as usize;
        let bs_version = r.u32()?;
        if bs_version < MIN_BS_VERSION {
            return Err(NifError::UnsupportedVersion { version, bs_version });
        }

        let mut export_info = Vec::new();
        for _ in 0..3 {
            r.short_string_raw(&mut export_info)?;
        }

        let num_types = r.u16()? as usize;
        let mut block_types = Vec::new();
        for _ in 0..num_types {
            block_types.push(r.sized_string()?);
        }

        let mut block_type_index = Vec::new();
        for _ in 0..num_blocks {
            block_type_index.push(r.u16()?);
        }
        let mut sizes = Vec::new();
        for _ in 0..num_blocks {
            sizes.push(r.u32()? as usize);
        }

        let num_strings = r.u32()? as usize;
        let max_string_len = r.u32()?;
        let mut strings = Vec::new();
        for _ in 0..num_strings {
            strings.push(r.sized_string()?);
        }

        let num_groups = r.u32()? as usize;
        let mut groups = Vec::new();
        for _ in 0..num_groups {
            groups.push(r.u32()?);
        }

        let mut blocks = Vec::with_capacity(num_blocks.min(1 << 16));
        for &s in &sizes {
            blocks.push(r.bytes(s)?);
        }

        let footer_start = r.pos();
        let num_roots = r.u32()? as usize;
        for _ in 0..num_roots {
            r.u32()?;
        }
        let footer = data[footer_start..].to_vec();

        Ok(Nif {
            header_line,
            version,
            endian,
            user_version,
            bs_version,
            export_info,
            block_types,
            block_type_index,
            max_string_len,
            strings,
            groups,
            blocks,
            footer,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(self.blocks.iter().map(|b| b.len()).sum::<usize>() + 4096);
        o.extend(&self.header_line);
        o.extend(self.version.to_le_bytes());
        o.push(self.endian);
        o.extend(self.user_version.to_le_bytes());
        o.extend((self.blocks.len() as u32).to_le_bytes());
        o.extend(self.bs_version.to_le_bytes());
        o.extend(&self.export_info);
        o.extend((self.block_types.len() as u16).to_le_bytes());
        for t in &self.block_types {
            o.extend((t.len() as u32).to_le_bytes());
            o.extend(t);
        }
        for i in &self.block_type_index {
            o.extend(i.to_le_bytes());
        }
        for b in &self.blocks {
            o.extend((b.len() as u32).to_le_bytes());
        }
        o.extend((self.strings.len() as u32).to_le_bytes());
        o.extend(self.max_string_len.to_le_bytes());
        for s in &self.strings {
            o.extend((s.len() as u32).to_le_bytes());
            o.extend(s);
        }
        o.extend((self.groups.len() as u32).to_le_bytes());
        for g in &self.groups {
            o.extend(g.to_le_bytes());
        }
        for b in &self.blocks {
            o.extend(b);
        }
        o.extend(&self.footer);
        o
    }

    /// Block type name of block `i` (the high bit of the index is a flag, not part of it).
    pub fn block_type(&self, i: usize) -> &[u8] {
        let idx = (self.block_type_index[i] & 0x7FFF) as usize;
        self.block_types.get(idx).map(|v| v.as_slice()).unwrap_or(b"")
    }

    /// String table entry for a string index read from a block (-1 or out of range = empty).
    pub fn string_at(&self, idx: i32) -> &[u8] {
        if idx < 0 {
            return b"";
        }
        self.strings.get(idx as usize).map(|v| v.as_slice()).unwrap_or(b"")
    }

    pub fn recompute_max_string_len(&mut self) {
        self.max_string_len = self.strings.iter().map(|s| s.len() as u32).max().unwrap_or(0);
    }
}

/// Case-insensitive (ASCII) first-match substring replace. Empty `old` inserts `new` at the start.
pub fn replace_first_ci(s: &[u8], old: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    let pos = if old.is_empty() {
        0
    } else {
        if old.len() > s.len() {
            return None;
        }
        (0..=s.len() - old.len()).find(|&i| s[i..i + old.len()].eq_ignore_ascii_case(old))?
    };
    let mut out = Vec::with_capacity(s.len() + new.len());
    out.extend_from_slice(&s[..pos]);
    out.extend_from_slice(new);
    out.extend_from_slice(&s[pos + old.len()..]);
    Some(out)
}

pub fn lossy(b: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Vec<u8> {
        let mut o = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        o.extend(NIF_VERSION.to_le_bytes());
        o.push(1);
        o.extend(12u32.to_le_bytes());
        o.extend(1u32.to_le_bytes()); // blocks
        o.extend(100u32.to_le_bytes());
        o.extend([1, 0, 1, 0, 1, 0]); // export info: three empty short strings
        o.extend(1u16.to_le_bytes());
        o.extend(18u32.to_le_bytes());
        o.extend(b"BSShaderTextureSet");
        o.extend(0u16.to_le_bytes());
        let block: Vec<u8> = [1u32.to_le_bytes().to_vec(), 5u32.to_le_bytes().to_vec(), b"a.dds".to_vec()].concat();
        o.extend((block.len() as u32).to_le_bytes());
        o.extend(2u32.to_le_bytes()); // strings
        o.extend(4u32.to_le_bytes());
        o.extend(3u32.to_le_bytes());
        o.extend(b"Bow");
        o.extend(4u32.to_le_bytes());
        o.extend([0xAC, 0xD4, b'x', b'y']); // invalid UTF-8 must survive
        o.extend(0u32.to_le_bytes()); // groups
        o.extend(&block);
        o.extend(1u32.to_le_bytes());
        o.extend(0u32.to_le_bytes());
        o.extend(b"trailing");
        o
    }

    #[test]
    fn roundtrip_is_byte_perfect() {
        let d = tiny();
        assert_eq!(Nif::parse(&d).unwrap().serialize(), d);
    }

    #[test]
    fn truncated_is_eof() {
        let d = tiny();
        let e = Nif::parse(&d[..60]).unwrap_err().to_string();
        assert_eq!(e, "IO error: failed to fill whole buffer");
    }

    #[test]
    fn wrong_version_reports_bs_zero() {
        let mut d = tiny();
        let i = d.iter().position(|&b| b == b'\n').unwrap() + 1;
        d[i..i + 4].copy_from_slice(&0x1400_0005u32.to_le_bytes());
        assert_eq!(
            Nif::parse(&d).unwrap_err().to_string(),
            "Unsupported NIF version: 0x14000005 (BS version 0)"
        );
    }

    #[test]
    fn max_string_len_recomputed() {
        let mut n = Nif::parse(&tiny()).unwrap();
        n.strings[0] = b"LongerName".to_vec();
        n.recompute_max_string_len();
        assert_eq!(n.max_string_len, 10);
        let again = Nif::parse(&n.serialize()).unwrap();
        assert_eq!(again.strings[1], vec![0xAC, 0xD4, b'x', b'y']);
    }

    #[test]
    fn replace_first_only_case_insensitive() {
        assert_eq!(replace_first_ci(b"Bow_bow", b"BOW", b"x").unwrap(), b"x_bow");
        assert_eq!(replace_first_ci(b"abc", b"", b"Z").unwrap(), b"Zabc");
        assert!(replace_first_ci(b"abc", b"zz", b"Z").is_none());
    }
}
