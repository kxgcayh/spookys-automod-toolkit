//! BSShaderTextureSet: `u32 count` then `count` sized strings (u32 length + bytes).

use crate::nif::Nif;

pub const TEXTURE_SET: &[u8] = b"BSShaderTextureSet";

/// Texture slots of one block: (slot, path bytes), plus the bytes after the last slot.
pub struct TextureSet {
    pub slots: Vec<Vec<u8>>,
    pub tail: Vec<u8>,
}

pub fn parse_block(b: &[u8]) -> Option<TextureSet> {
    let n = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?) as usize;
    let mut p = 4;
    let mut slots = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let len = u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?) as usize;
        p += 4;
        slots.push(b.get(p..p.checked_add(len)?)?.to_vec());
        p += len;
    }
    Some(TextureSet { slots, tail: b[p..].to_vec() })
}

pub fn build_block(t: &TextureSet) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend((t.slots.len() as u32).to_le_bytes());
    for s in &t.slots {
        o.extend((s.len() as u32).to_le_bytes());
        o.extend(s);
    }
    o.extend(&t.tail);
    o
}

/// (block index, slot, path) for every non-empty texture path.
pub fn list(nif: &Nif) -> Vec<(usize, usize, Vec<u8>)> {
    let mut out = Vec::new();
    for i in 0..nif.blocks.len() {
        if nif.block_type(i) != TEXTURE_SET {
            continue;
        }
        if let Some(t) = parse_block(&nif.blocks[i]) {
            for (s, p) in t.slots.into_iter().enumerate() {
                if !p.is_empty() {
                    out.push((i, s, p));
                }
            }
        }
    }
    out
}
