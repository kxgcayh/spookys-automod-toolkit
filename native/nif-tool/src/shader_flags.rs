//! BSLightingShaderProperty shader flags (Skyrim layout):
//! u32 shader type, NiObjectNET (i32 name, u32 extra count, i32 refs, i32 controller), u32 SF1, u32 SF2.

use crate::nif::Nif;

pub const LIGHTING_SHADER: &[u8] = b"BSLightingShaderProperty";

pub const SF1_NAMES: &[(u32, &str)] = &[
    (0, "Specular"),
    (1, "Skinned"),
    (7, "Environment_Mapping"),
    (8, "Receive_Shadows"),
    (9, "Cast_Shadows"),
    (17, "Eye_Environment_Mapping"),
    (22, "Own_Emit"),
    (25, "Remappable_Textures"),
    (31, "ZBuffer_Test"),
];

pub const SF2_NAMES: &[(u32, &str)] = &[
    (0, "ZBuffer_Write"),
    (4, "Double_Sided"),
    (7, "Assume_Shadowmask"),
    (15, "EnvMap_Light_Fade"),
    (25, "Soft_Lighting"),
];

pub const SF1_EYE_ENV_MAP: u32 = 1 << 17;
pub const SF1_ENV_MAP: u32 = 1 << 7;
/// SF1 bits fix-eyes makes sure are set.
pub const SF1_ENSURE: u32 = (1 << 0) | (1 << 1) | (1 << 8) | (1 << 9) | (1 << 22) | (1 << 25) | (1 << 31);
/// SF2 bits fix-eyes makes sure are set.
pub const SF2_ENSURE: u32 = (1 << 0) | (1 << 4) | (1 << 7) | (1 << 15) | (1 << 25);

pub fn names(flags: u32, table: &[(u32, &str)]) -> String {
    let v: Vec<&str> = table.iter().filter(|(b, _)| flags & (1 << b) != 0).map(|(_, n)| *n).collect();
    format!("[{}]", v.join(", "))
}

pub struct Shader {
    pub block: usize,
    pub name: Vec<u8>,
    /// Byte offset of SF1 inside the block (SF2 follows it).
    pub offset: usize,
    pub sf1: u32,
    pub sf2: u32,
}

fn rd_u32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

pub fn parse_block(nif: &Nif, i: usize) -> Option<Shader> {
    let b = &nif.blocks[i];
    let name_idx = rd_u32(b, 4)? as i32;
    let extra = rd_u32(b, 8)? as usize;
    let offset = 12usize.checked_add(extra.checked_mul(4)?)?.checked_add(4)?;
    let sf1 = rd_u32(b, offset)?;
    let sf2 = rd_u32(b, offset + 4)?;
    Some(Shader { block: i, name: nif.string_at(name_idx).to_vec(), offset, sf1, sf2 })
}

pub fn list(nif: &Nif) -> Vec<Shader> {
    (0..nif.blocks.len())
        .filter(|&i| nif.block_type(i) == LIGHTING_SHADER)
        .filter_map(|i| parse_block(nif, i))
        .collect()
}

pub fn fixed(sf1: u32, sf2: u32) -> (u32, u32) {
    ((sf1 & !SF1_EYE_ENV_MAP) | SF1_ENV_MAP | SF1_ENSURE, sf2 | SF2_ENSURE)
}

pub fn write_flags(nif: &mut Nif, s: &Shader, sf1: u32, sf2: u32) {
    let b = &mut nif.blocks[s.block];
    b[s.offset..s.offset + 4].copy_from_slice(&sf1.to_le_bytes());
    b[s.offset + 4..s.offset + 8].copy_from_slice(&sf2.to_le_bytes());
}
