//! nif-tool: native rewrite of the nif-tool CLI bundled with Spooky's AutoMod Toolkit.
//!
//! Command names, flags and output text follow the original Windows binary (checked by a
//! differential test against it), so AutoMod's `NifToolWrapper` and its users see the same
//! results. Deliberate difference: strings are handled as raw bytes, so a file with invalid UTF-8
//! in its string table passes `verify` and keeps those bytes on every write; the original decodes
//! them lossily and writes U+FFFD in their place.

mod nif;
mod shader_flags;
mod string_table;
mod texture_paths;

use clap::{ArgAction, Parser, Subcommand};
use nif::{lossy, replace_first_ci, Nif};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "nif-tool", version, about = "SSE NIF texture path rewriting tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all texture paths in NIF file(s)
    List {
        /// NIF file or folder to scan
        path: PathBuf,
    },
    /// Replace texture path substrings in NIF file(s)
    Replace {
        /// NIF file or folder to process
        path: PathBuf,
        /// Old path substring to find (case-insensitive)
        #[arg(long)]
        old: String,
        /// New path substring to replace with
        #[arg(long)]
        new: String,
        /// Preview changes without writing files
        #[arg(long)]
        dry_run: bool,
        /// Create .nif.bak backup before overwriting (default: true)
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        backup: bool,
    },
    /// Verify a NIF can be parsed and re-serialized without data loss
    Verify {
        /// NIF file or folder to verify
        path: PathBuf,
    },
    /// Restore NIF files from .nif.bak backups
    Restore {
        /// File or folder containing .nif.bak files
        path: PathBuf,
    },
    /// List all string table entries in NIF file(s)
    Strings {
        /// NIF file or folder to scan
        path: PathBuf,
    },
    /// Show shader flags for BSLightingShaderProperty blocks
    ShaderInfo {
        /// NIF file or folder to inspect
        path: PathBuf,
    },
    /// Fix eye shader flags (Eye_Environment_Mapping → Environment_Mapping)
    FixEyes {
        /// NIF file or folder to process
        path: PathBuf,
        /// Preview changes without writing files
        #[arg(long)]
        dry_run: bool,
        /// Create .nif.bak backup before overwriting (default: true)
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        backup: bool,
    },
    /// Rename string table entries in NIF file(s) (block names, etc.)
    Rename {
        /// NIF file or folder to process
        path: PathBuf,
        /// Old substring to find (case-insensitive)
        #[arg(long)]
        old: String,
        /// New substring to replace with
        #[arg(long)]
        new: String,
        /// Preview changes without writing files
        #[arg(long)]
        dry_run: bool,
        /// Create .nif.bak backup before overwriting (default: true)
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        backup: bool,
    },
}

fn has_ext_ci(p: &Path, ext: &str) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn is_nif(p: &Path) -> bool {
    has_ext_ci(p, "nif")
}

fn is_nif_bak(p: &Path) -> bool {
    has_ext_ci(p, "bak") && p.file_stem().is_some_and(|s| is_nif(Path::new(s)))
}

fn walk(dir: &Path, pred: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, pred, out);
        } else if p.is_file() && pred(&p) {
            out.push(p);
        }
    }
}

fn collect(path: &Path, pred: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if path.is_file() {
        if pred(path) {
            out.push(path.to_path_buf());
        }
    } else if path.is_dir() {
        walk(path, pred, &mut out);
    }
    out
}

/// NIF files under `path`, with the "Found" header printed; None when there are none.
fn nif_files(path: &Path, verb: &str, dry_run: bool) -> Option<Vec<PathBuf>> {
    let files = collect(path, &is_nif);
    if files.is_empty() {
        eprintln!("No NIF files found in: {}", path.display());
        return None;
    }
    println!("{verb} {} NIF file(s)", files.len());
    if dry_run {
        println!("DRY RUN — no files will be modified");
    }
    println!();
    Some(files)
}

fn load(p: &Path) -> Option<(Vec<u8>, Nif)> {
    let data = match fs::read(p) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Error parsing {}: IO error: {e}", p.display());
            return None;
        }
    };
    match Nif::parse(&data) {
        Ok(n) => Some((data, n)),
        Err(e) => {
            eprintln!("  Error parsing {}: {e}", p.display());
            None
        }
    }
}

/// Validate and write a modified NIF. Returns false (file untouched) if anything fails.
fn save(p: &Path, original: &[u8], nif: &Nif, backup: bool) -> bool {
    let out = nif.serialize();
    match Nif::parse(&out) {
        Ok(check) if check.serialize() == out => {}
        Ok(_) => {
            eprintln!("  Validation failed for {}: re-serialized output differs; file not written", p.display());
            return false;
        }
        Err(e) => {
            eprintln!("  Validation failed for {}: {e}; file not written", p.display());
            return false;
        }
    }
    if backup {
        // Same name the original tool uses: the extension becomes "nif.bak" whatever its case.
        let bak = p.with_extension("nif.bak");
        if let Err(e) = fs::write(&bak, original) {
            eprintln!("  Error writing backup for {}: {e}", p.display());
            return false;
        }
    }
    if let Err(e) = fs::write(p, &out) {
        eprintln!("  Error writing {}: {e}", p.display());
        return false;
    }
    true
}

fn cmd_list(path: &Path) {
    let Some(files) = nif_files(path, "Found", false) else { return };
    for f in files {
        let Some((_, nif)) = load(&f) else { continue };
        let tex = texture_paths::list(&nif);
        if tex.is_empty() {
            println!("{}: no texture paths", f.display());
            continue;
        }
        println!("{}:", f.display());
        for (b, s, p) in tex {
            println!("  [block {b} BSShaderTextureSet slot {s}] {}", lossy(&p));
        }
        println!();
    }
}

fn cmd_strings(path: &Path) {
    let Some(files) = nif_files(path, "Found", false) else { return };
    for f in files {
        let Some((_, nif)) = load(&f) else { continue };
        let list = string_table::list(&nif);
        if list.is_empty() {
            println!("{}: no string table entries", f.display());
            continue;
        }
        println!("{}:", f.display());
        for (i, s) in list {
            println!("  [{i}] {}", lossy(s));
        }
        println!();
    }
}

fn cmd_shader_info(path: &Path) {
    let Some(files) = nif_files(path, "Found", false) else { return };
    let mut eye_files = 0;
    for f in files {
        let Some((_, nif)) = load(&f) else { continue };
        let shaders = shader_flags::list(&nif);
        if shaders.is_empty() {
            continue;
        }
        println!("{}:", f.display());
        let mut eye = false;
        for s in &shaders {
            let mark = if s.sf1 & shader_flags::SF1_EYE_ENV_MAP != 0 {
                eye = true;
                " *** EYE_ENV_MAP ***"
            } else {
                ""
            };
            println!("  [block {}] {}{mark}", s.block, lossy(&s.name));
            println!("    SF1: 0x{:08X} {}", s.sf1, shader_flags::names(s.sf1, shader_flags::SF1_NAMES));
            println!("    SF2: 0x{:08X} {}", s.sf2, shader_flags::names(s.sf2, shader_flags::SF2_NAMES));
        }
        println!();
        if eye {
            eye_files += 1;
        }
    }
    println!("{eye_files} file(s) with Eye_Environment_Mapping flag");
}

fn cmd_verify(path: &Path) {
    let Some(files) = nif_files(path, "Verifying", false) else { return };
    let (mut ok, mut bad) = (0, 0);
    for f in files {
        let data = match fs::read(&f) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  FAIL {}: parse error: IO error: {e}", f.display());
                bad += 1;
                continue;
            }
        };
        match Nif::parse(&data) {
            Err(e) => {
                eprintln!("  FAIL {}: parse error: {e}", f.display());
                bad += 1;
            }
            Ok(n) => {
                let out = n.serialize();
                if out == data {
                    println!("  OK   {} ({} bytes, byte-perfect roundtrip)", f.display(), data.len());
                    ok += 1;
                } else {
                    let at = data.iter().zip(&out).position(|(a, b)| a != b);
                    if out.len() != data.len() {
                        eprintln!(
                            "  FAIL {}: size mismatch (original {} bytes, roundtrip {} bytes, diff at {:?})",
                            f.display(),
                            data.len(),
                            out.len(),
                            at
                        );
                    } else {
                        eprintln!("  FAIL {}: content mismatch at byte {}", f.display(), at.unwrap_or(0));
                    }
                    bad += 1;
                }
            }
        }
    }
    println!();
    println!("{ok} passed, {bad} failed");
}

fn finish_file(dry_run: bool, saved: bool) {
    if dry_run {
        println!("  (dry run — not saved)");
    } else if saved {
        println!("  Saved.");
    }
    println!();
}

fn cmd_replace(path: &Path, old: &str, new: &str, dry_run: bool, backup: bool) {
    let Some(files) = nif_files(path, "Found", dry_run) else { return };
    let (mut count, mut nfiles, mut failed) = (0, 0, 0);
    for f in files {
        let Some((data, mut nif)) = load(&f) else {
            failed += 1;
            continue;
        };
        let mut changes = Vec::new();
        for i in 0..nif.blocks.len() {
            if nif.block_type(i) != texture_paths::TEXTURE_SET {
                continue;
            }
            let Some(mut t) = texture_paths::parse_block(&nif.blocks[i]) else { continue };
            let mut touched = false;
            for (s, p) in t.slots.iter_mut().enumerate() {
                if p.is_empty() {
                    continue;
                }
                if let Some(n) = replace_first_ci(p, old.as_bytes(), new.as_bytes()) {
                    changes.push((i, s, p.clone(), n.clone()));
                    *p = n;
                    touched = true;
                }
            }
            if touched {
                nif.blocks[i] = texture_paths::build_block(&t);
            }
        }
        if changes.is_empty() {
            continue;
        }
        println!("{}:", f.display());
        for (b, s, o, n) in &changes {
            println!("  [block {b} slot {s}]");
            println!("    - {}", lossy(o));
            println!("    + {}", lossy(n));
        }
        let saved = !dry_run && save(&f, &data, &nif, backup);
        if !dry_run && !saved {
            failed += 1;
            println!();
            continue;
        }
        finish_file(dry_run, saved);
        count += changes.len();
        nfiles += 1;
    }
    println!("Done: {count} replacement(s) across {nfiles} file(s), {failed} failed");
}

fn cmd_rename(path: &Path, old: &str, new: &str, dry_run: bool, backup: bool) {
    let Some(files) = nif_files(path, "Found", dry_run) else { return };
    let (mut count, mut nfiles, mut failed) = (0, 0, 0);
    for f in files {
        let Some((data, mut nif)) = load(&f) else {
            failed += 1;
            continue;
        };
        let changes = string_table::plan(&nif, old.as_bytes(), new.as_bytes());
        if changes.is_empty() {
            continue;
        }
        println!("{}:", f.display());
        for (i, o, n) in &changes {
            println!("  [string {i}]");
            println!("    - {}", lossy(o));
            println!("    + {}", lossy(n));
            nif.strings[*i] = n.clone();
        }
        nif.recompute_max_string_len();
        let saved = !dry_run && save(&f, &data, &nif, backup);
        if !dry_run && !saved {
            failed += 1;
            println!();
            continue;
        }
        finish_file(dry_run, saved);
        count += changes.len();
        nfiles += 1;
    }
    println!("Done: {count} rename(s) across {nfiles} file(s), {failed} failed");
}

fn cmd_fix_eyes(path: &Path, dry_run: bool, backup: bool) {
    use shader_flags::{names, SF1_EYE_ENV_MAP, SF1_NAMES, SF2_NAMES};
    let Some(files) = nif_files(path, "Found", dry_run) else { return };
    let (mut count, mut nfiles, mut failed) = (0, 0, 0);
    for f in files {
        let Some((data, mut nif)) = load(&f) else {
            failed += 1;
            continue;
        };
        let targets: Vec<_> = shader_flags::list(&nif).into_iter().filter(|s| s.sf1 & SF1_EYE_ENV_MAP != 0).collect();
        if targets.is_empty() {
            continue;
        }
        println!("{}:", f.display());
        for s in &targets {
            let (n1, n2) = shader_flags::fixed(s.sf1, s.sf2);
            println!("  [block {}] {}", s.block, lossy(&s.name));
            println!("    SF1: 0x{:08X} -> 0x{:08X}", s.sf1, n1);
            println!("      - {}", names(s.sf1, SF1_NAMES));
            println!("      + {}", names(n1, SF1_NAMES));
            println!("    SF2: 0x{:08X} -> 0x{:08X}", s.sf2, n2);
            println!("      - {}", names(s.sf2, SF2_NAMES));
            println!("      + {}", names(n2, SF2_NAMES));
            shader_flags::write_flags(&mut nif, s, n1, n2);
        }
        let saved = !dry_run && save(&f, &data, &nif, backup);
        if !dry_run && !saved {
            failed += 1;
            println!();
            continue;
        }
        finish_file(dry_run, saved);
        count += targets.len();
        nfiles += 1;
    }
    println!("Done: {count} fix(es) across {nfiles} file(s), {failed} failed");
}

/// `x.nif.bak` restores to `x.nif`. Backups of `x.NIF` are also named `x.nif.bak`, so on a
/// case-sensitive filesystem restore onto the existing file whose name differs only in case.
fn restore_target(bak: &Path) -> PathBuf {
    let target = bak.with_extension("");
    if target.exists() {
        return target;
    }
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().and_then(|n| n.to_str())) else {
        return target;
    };
    let found = fs::read_dir(dir).ok().and_then(|rd| {
        rd.filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name)))
    });
    found.unwrap_or(target)
}

fn cmd_restore(path: &Path) {
    let baks = collect(path, &is_nif_bak);
    if baks.is_empty() {
        eprintln!("No .nif.bak files found in: {}", path.display());
        return;
    }
    println!("Found {} backup file(s)", baks.len());
    println!();
    let (mut ok, mut bad) = (0, 0);
    for b in baks {
        let target = restore_target(&b);
        match fs::copy(&b, &target).and_then(|_| fs::remove_file(&b)) {
            Ok(()) => {
                println!("  Restored {}", target.display());
                ok += 1;
            }
            Err(e) => {
                eprintln!("  Failed to restore {}: {e}", target.display());
                bad += 1;
            }
        }
    }
    println!();
    println!("{ok} restored, {bad} failed");
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::List { path } => cmd_list(&path),
        Command::Strings { path } => cmd_strings(&path),
        Command::ShaderInfo { path } => cmd_shader_info(&path),
        Command::Verify { path } => cmd_verify(&path),
        Command::Restore { path } => cmd_restore(&path),
        Command::Replace { path, old, new, dry_run, backup } => cmd_replace(&path, &old, &new, dry_run, backup),
        Command::Rename { path, old, new, dry_run, backup } => cmd_rename(&path, &old, &new, dry_run, backup),
        Command::FixEyes { path, dry_run, backup } => cmd_fix_eyes(&path, dry_run, backup),
    }
}
