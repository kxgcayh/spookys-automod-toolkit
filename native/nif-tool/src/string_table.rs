//! Header string table (node names, block names, paths stored as strings).

use crate::nif::{replace_first_ci, Nif};

/// (index, string) for every non-empty entry.
pub fn list(nif: &Nif) -> Vec<(usize, &[u8])> {
    nif.strings.iter().enumerate().filter(|(_, s)| !s.is_empty()).map(|(i, s)| (i, s.as_slice())).collect()
}

/// Planned renames: (index, old, new). Does not modify `nif`.
pub fn plan(nif: &Nif, old: &[u8], new: &[u8]) -> Vec<(usize, Vec<u8>, Vec<u8>)> {
    nif.strings
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .filter_map(|(i, s)| replace_first_ci(s, old, new).map(|n| (i, s.clone(), n)))
        .collect()
}
