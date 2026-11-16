// SPDX-License-Identifier: 0BSD
//! Deterministic "many small files" corpus (class 1): two versions of a synthetic game tree.
//!
//! Layout of version 1 (default 100 000 regular files):
//! * boundary group: 8 files at each of size t-1, t, t+1 for t in {64 KiB, 256 KiB, 1 MiB, 4 MiB, 16 MiB}
//!   (all candidate min/max values of the grid) plus three 64 MiB-boundary files;
//! * 2 % exact content duplicates of other files (same chunk ids under several paths);
//! * the rest drawn from `corpus_model::SIZE_TABLE`;
//! * 50 symlinks, 3 % of the files executable.
//!
//! Version 2 is derived from version 1 by a fixed change pattern (shares are of the non-boundary version-1 files):
//! 3.0 % deleted, 3.0 % moved (same content, new path), 8.0 % modified, the rest unchanged, plus new files worth
//! 4 % of the version-1 file count. Modified files get 1 edit (1-3 edits if >= 64 KiB), drawn from in-place overwrite
//! 35 %, insert 25 %, delete 10 %, append 15 %, full rewrite 15 %. Boundary files cross or touch the min/max
//! boundaries: k<3 append 1 byte, k<5 truncate 1 byte, k=5 flip one byte in place, k>5 unchanged.
//!
//! All randomness comes from `corpus_model::Rng` (integer-only), so output is identical for equal parameters.

use crate::corpus_model::{derive_seed, sample_size, synth_path, Rng};
use crate::treewalk::walk;
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const GEN_VERSION: &str = "chunkbench-gen/1";
const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct GenParams {
    pub seed: u64,
    pub files: usize,
    pub symlinks: usize,
    /// Include the boundary group (sizes around all min/max values). Off for the diagnostic corpus without it.
    pub boundary: bool,
}

struct V1File {
    uid: u64,
    size: u64,
    cseed: u64,
    mode: u32,
    boundary: bool,
}

pub fn boundary_sizes(files: usize, enabled: bool) -> Vec<u64> {
    let mut v = Vec::new();
    if files < 1000 || !enabled {
        return v;
    }
    for t in [64 * KIB, 256 * KIB, MIB, 4 * MIB, 16 * MIB] {
        for d in [-1i64, 0, 1] {
            for _ in 0..8 {
                v.push((t as i64 + d) as u64);
            }
        }
    }
    if files >= 20_000 {
        for d in [-1i64, 0, 1] {
            v.push((64 * MIB as i64 + d) as u64);
        }
    }
    v
}

fn layout_v1(p: &GenParams) -> Vec<V1File> {
    let mut r = Rng::new(derive_seed(p.seed, 1, 0));
    let bsz = boundary_sizes(p.files, p.boundary);
    let mut out: Vec<V1File> = Vec::with_capacity(p.files);
    for s in &bsz {
        let uid = out.len() as u64;
        out.push(V1File {
            uid,
            size: *s,
            cseed: derive_seed(p.seed, uid, 0xC0),
            mode: 0o644,
            boundary: true,
        });
    }
    let rest = p.files - bsz.len();
    let dups = rest / 50;
    let sampled = rest - dups;
    let first_sampled = out.len();
    for _ in 0..sampled {
        let uid = out.len() as u64;
        let size = sample_size(&mut r);
        let mode = if r.below(100) < 3 { 0o755 } else { 0o644 };
        out.push(V1File {
            uid,
            size,
            cseed: derive_seed(p.seed, uid, 0xC0),
            mode,
            boundary: false,
        });
    }
    for _ in 0..dups {
        let uid = out.len() as u64;
        let src = first_sampled + r.below(sampled as u64) as usize;
        let (size, cseed) = (out[src].size, out[src].cseed);
        out.push(V1File {
            uid,
            size,
            cseed,
            mode: 0o644,
            boundary: false,
        });
    }
    out
}

fn content(cseed: u64, size: u64) -> Vec<u8> {
    let mut v = vec![0u8; size as usize];
    Rng::new(cseed).fill(&mut v);
    v
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum Fate {
    Unchanged,
    Deleted,
    Moved,
    Modified,
}

fn fate(r: &mut Rng) -> Fate {
    match r.below(1000) {
        0..=29 => Fate::Deleted,
        30..=59 => Fate::Moved,
        60..=139 => Fate::Modified,
        _ => Fate::Unchanged,
    }
}

fn apply_edits(data: &mut Vec<u8>, r: &mut Rng) {
    let edits = if data.len() >= 64 * 1024 {
        1 + r.below(3)
    } else {
        1
    };
    for _ in 0..edits {
        let n = data.len() as u64;
        if n < 2 {
            let add = r.range(1, 64) as usize;
            let mut b = vec![0u8; add];
            r.fill(&mut b);
            data.extend_from_slice(&b);
            continue;
        }
        match r.below(100) {
            0..=34 => {
                let len = r.range(4096.min(n / 2).max(1), 65_536.min(n / 2).max(1));
                let off = r.below(n - len + 1) as usize;
                r.fill(&mut data[off..off + len as usize]);
            }
            35..=59 => {
                let len = r.range(1, 65_536) as usize;
                let off = r.below(n + 1) as usize;
                let mut b = vec![0u8; len];
                r.fill(&mut b);
                data.splice(off..off, b);
            }
            60..=69 => {
                let len = r.range(1, 65_536.min(n / 2).max(1));
                let off = r.below(n - len + 1) as usize;
                data.drain(off..off + len as usize);
            }
            70..=84 => {
                let len = r.range(1, 65_536) as usize;
                let mut b = vec![0u8; len];
                r.fill(&mut b);
                data.extend_from_slice(&b);
            }
            _ => {
                let mut b = vec![0u8; data.len()];
                r.fill(&mut b);
                *data = b;
            }
        }
    }
}

#[derive(Serialize, Default, Clone)]
pub struct Counts {
    pub files_v1: u64,
    pub bytes_v1: u64,
    pub files_v2: u64,
    pub bytes_v2: u64,
    pub unchanged: u64,
    pub deleted: u64,
    pub moved: u64,
    pub modified: u64,
    pub new: u64,
    pub boundary_files: u64,
    pub duplicate_files_v1: u64,
}

#[derive(Serialize)]
pub struct GenReport {
    pub generator: &'static str,
    pub model: &'static str,
    pub params: GenParams,
    pub counts: Counts,
    pub version_digests: Option<(String, String)>,
}

fn write_file(root: &Path, rel: &str, data: &[u8], mode: u32) -> io::Result<()> {
    let p = root.join(rel);
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(&p, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> io::Result<()> {
    if let Some(d) = link.parent() {
        fs::create_dir_all(d)?;
    }
    std::os::unix::fs::symlink(target, link)
}
#[cfg(not(unix))]
fn symlink(_t: &str, _l: &Path) -> io::Result<()> {
    Ok(())
}

pub fn generate(out: &Path, p: &GenParams) -> io::Result<GenReport> {
    let v1 = layout_v1(p);
    let (d1, d2): (PathBuf, PathBuf) = (out.join("v1"), out.join("v2"));
    fs::create_dir_all(&d1)?;
    fs::create_dir_all(&d2)?;
    let path_of = |uid: u64| synth_path(p.seed, uid);
    // Decide fates sequentially (one stream), then write in parallel.
    let mut fr = Rng::new(derive_seed(p.seed, 2, 0));
    let fates: Vec<Fate> = v1
        .iter()
        .map(|f| {
            if f.boundary {
                if f.uid % 8 >= 6 {
                    Fate::Unchanged
                } else {
                    Fate::Modified
                }
            } else {
                fate(&mut fr)
            }
        })
        .collect();
    let n_new = p.files / 25;
    let mut nr = Rng::new(derive_seed(p.seed, 3, 0));
    let new_files: Vec<(u64, u64)> = (0..n_new)
        .map(|i| ((p.files + i) as u64, sample_size(&mut nr)))
        .collect();

    // Version 1 and the modified/moved part of version 2 in one pass over version-1 files.
    v1.par_iter()
        .zip(fates.par_iter())
        .try_for_each(|(f, fate)| -> io::Result<()> {
            let data = content(f.cseed, f.size);
            let rel = path_of(f.uid);
            write_file(&d1, &rel, &data, f.mode)?;
            let mut er = Rng::new(derive_seed(p.seed, f.uid, 0xED));
            match fate {
                Fate::Deleted => {}
                Fate::Unchanged => write_file(&d2, &rel, &data, f.mode)?,
                Fate::Moved => {
                    write_file(&d2, &synth_path(p.seed, f.uid + 1_000_000), &data, f.mode)?
                }
                Fate::Modified => {
                    let mut d = data;
                    if f.boundary {
                        match f.uid % 8 {
                            0..=2 => d.push(0xA5),
                            3..=4 => {
                                d.pop();
                            }
                            5 => {
                                let i = (er.below(d.len() as u64)) as usize;
                                d[i] ^= 0xFF;
                            }
                            _ => {}
                        }
                    } else {
                        apply_edits(&mut d, &mut er);
                    }
                    write_file(&d2, &rel, &d, f.mode)?;
                }
            }
            Ok(())
        })?;
    new_files
        .par_iter()
        .try_for_each(|(uid, size)| -> io::Result<()> {
            write_file(
                &d2,
                &path_of(*uid),
                &content(derive_seed(p.seed, *uid, 0xC0), *size),
                0o644,
            )
        })?;

    // Symlinks: targets are existing small files of version 1 (relative links), a few are re-pointed in version 2.
    let mut sr = Rng::new(derive_seed(p.seed, 4, 0));
    for i in 0..p.symlinks {
        let t = &v1[sr.below(v1.len() as u64) as usize];
        let target = format!("../{}", path_of(t.uid));
        let rel = format!("links/l{i:04}");
        symlink(&target, &d1.join(&rel))?;
        let t2 = if i.is_multiple_of(5) {
            format!("../{}", path_of(v1[sr.below(v1.len() as u64) as usize].uid))
        } else {
            target
        };
        symlink(&t2, &d2.join(&rel))?;
    }

    let mut c = Counts::default();
    for (f, fa) in v1.iter().zip(fates.iter()) {
        c.files_v1 += 1;
        c.bytes_v1 += f.size;
        if f.boundary {
            c.boundary_files += 1;
        }
        match fa {
            Fate::Unchanged => c.unchanged += 1,
            Fate::Deleted => c.deleted += 1,
            Fate::Moved => c.moved += 1,
            Fate::Modified => c.modified += 1,
        }
    }
    c.new = n_new as u64;
    c.duplicate_files_v1 = ((p.files - boundary_sizes(p.files, p.boundary).len()) / 50) as u64;
    // Real totals of version 2 from the written tree (modified files change size).
    let t2 = walk(&d2)?;
    c.files_v2 = t2.files.len() as u64;
    c.bytes_v2 = t2.files.iter().map(|f| f.size).sum();
    Ok(GenReport {
        generator: GEN_VERSION,
        model: crate::corpus_model::MODEL_VERSION,
        params: p.clone(),
        counts: c,
        version_digests: None,
    })
}

#[derive(Serialize)]
pub struct FilterReport {
    pub boundary_files_removed_v1: u64,
    pub boundary_files_removed_v2: u64,
    pub files_kept_v1: u64,
    pub files_kept_v2: u64,
}

/// Derives a corpus without the boundary group from an existing generated corpus: exactly the boundary files are
/// left out of both versions, every other file is the same file (hard link, copy as fallback), so all remaining
/// bytes, edits and duplicates are identical to the original. Isolates the influence of the boundary group.
pub fn filter_boundary(src: &Path, out: &Path, p: &GenParams) -> io::Result<FilterReport> {
    let n_b = boundary_sizes(p.files, true).len() as u64;
    let excluded: std::collections::HashSet<String> =
        (0..n_b).map(|uid| synth_path(p.seed, uid)).collect();
    let mut rep = FilterReport {
        boundary_files_removed_v1: 0,
        boundary_files_removed_v2: 0,
        files_kept_v1: 0,
        files_kept_v2: 0,
    };
    for v in ["v1", "v2"] {
        let t = walk(&src.join(v))?;
        let (mut removed, mut kept) = (0u64, 0u64);
        for f in &t.files {
            if excluded.contains(&f.rel) {
                removed += 1;
                continue;
            }
            let dst = out.join(v).join(&f.rel);
            if let Some(d) = dst.parent() {
                fs::create_dir_all(d)?;
            }
            if fs::hard_link(&f.abs, &dst).is_err() {
                fs::copy(&f.abs, &dst)?;
            }
            kept += 1;
        }
        for (rel, target) in &t.symlinks {
            symlink(target, &out.join(v).join(rel))?;
        }
        if removed != n_b {
            return Err(io::Error::other(format!(
                "expected {n_b} boundary files in {v}, found {removed}"
            )));
        }
        if v == "v1" {
            rep.boundary_files_removed_v1 = removed;
            rep.files_kept_v1 = kept;
        } else {
            rep.boundary_files_removed_v2 = removed;
            rep.files_kept_v2 = kept;
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_sizes_and_filter_preserve_every_other_byte() {
        assert!(boundary_sizes(999, true).is_empty());
        assert!(boundary_sizes(1000, false).is_empty());
        assert_eq!(boundary_sizes(1000, true).len(), 120);
        assert_eq!(boundary_sizes(20_000, true).len(), 123);
        let root = std::env::temp_dir().join(format!("chunkbench-filter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src");
        let out = root.join("out");
        // Small contents exercise the same path exclusion without generating gigabytes.
        for version in ["v1", "v2"] {
            for uid in 0..125 {
                write_file(
                    &src.join(version),
                    &synth_path(31, uid),
                    format!("{version}-{uid}").as_bytes(),
                    0o644,
                )
                .unwrap();
            }
        }
        let p = GenParams {
            seed: 31,
            files: 1000,
            symlinks: 0,
            boundary: true,
        };
        let r = filter_boundary(&src, &out, &p).unwrap();
        assert_eq!(
            (r.boundary_files_removed_v1, r.boundary_files_removed_v2),
            (120, 120)
        );
        assert_eq!((r.files_kept_v1, r.files_kept_v2), (5, 5));
        for version in ["v1", "v2"] {
            assert_eq!(walk(&out.join(version)).unwrap().files.len(), 5);
            for uid in 0..125 {
                let rel = synth_path(31, uid);
                if uid < 120 {
                    assert!(!out.join(version).join(rel).exists());
                } else {
                    assert_eq!(
                        fs::read(src.join(version).join(&rel)).unwrap(),
                        fs::read(out.join(version).join(&rel)).unwrap()
                    );
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
