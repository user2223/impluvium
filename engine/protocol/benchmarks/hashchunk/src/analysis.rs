// SPDX-License-Identifier: 0BSD
//! Chunk analysis: transfer share T per version pair, per class and overall, plus chunk counts and manifest sizes.
//!
//! T(pair) = bytes of the distinct chunks of the new version that do not occur in the old version.
//! Chunk identity = BLAKE3-256 of the chunk bytes (identity only; independent of the manifest hash decision).
//! Unchanged files (same relative path and same content hash) are reused whole, i.e. not chunked again.

use crate::chunking::{Candidate, LOAD_LIMIT};
use crate::treewalk::{walk, Tree};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

pub type Id = [u8; 32];
pub type ChunkRefs = Vec<(Id, u32)>;
const HIST: usize = 26;
/// File size classes (upper bounds, bytes) used to attribute transfer and changes; the last class is "above the last bound".
pub const SIZE_CLASS_BOUNDS: [u64; 5] = [64 << 10, 256 << 10, 1 << 20, 4 << 20, 16 << 20];
pub const SIZE_CLASSES: usize = SIZE_CLASS_BOUNDS.len() + 1;
pub fn size_class(size: u64) -> usize {
    SIZE_CLASS_BOUNDS
        .iter()
        .position(|b| size <= *b)
        .unwrap_or(SIZE_CLASS_BOUNDS.len())
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub classes: Vec<ClassCfg>,
    /// Optional subset of candidate ids; default is the full grid.
    pub candidates: Option<Vec<String>>,
}
fn default_kind() -> String {
    "diagnostic".into()
}
pub const DECIDING_KINDS: [&str; 3] = ["many-small-files", "compressed-archive", "pak-files"];

#[derive(Deserialize, Clone)]
pub struct ClassCfg {
    pub name: String,
    /// Stable kind, independent of the display name: "many-small-files", "compressed-archive", "pak-files" are the three
    /// deciding classes; anything else (default "diagnostic") is reported but never enters the deciding mean.
    #[serde(default = "default_kind")]
    pub kind: String,
    pub pairs: Vec<PairCfg>,
}
#[derive(Deserialize, Clone)]
pub struct PairCfg {
    pub label: String,
    pub old: String,
    pub new: String,
}

pub struct FileRec {
    pub rel: String,
    pub size: u64,
    pub hash: Id,
    pub chunks: Vec<ChunkRefs>,
}

fn chunk_one(path: &Path, size: u64, cands: &[Candidate]) -> io::Result<(Id, Vec<ChunkRefs>)> {
    if size <= LOAD_LIMIT {
        let data = fs::read(path)?;
        if data.len() as u64 != size {
            return Err(io::Error::other("file changed while reading"));
        }
        let hash: Id = *blake3::hash(&data).as_bytes();
        let per = cands
            .iter()
            .map(|c| {
                if size == 0 {
                    Vec::new()
                } else if size <= c.max {
                    vec![(hash, size as u32)]
                } else {
                    let mut off = 0usize;
                    c.lengths(&data)
                        .into_iter()
                        .map(|l| {
                            let id = *blake3::hash(&data[off..off + l]).as_bytes();
                            off += l;
                            (id, l as u32)
                        })
                        .collect()
                }
            })
            .collect();
        Ok((hash, per))
    } else {
        let mut h = blake3::Hasher::new();
        let mut f = fs::File::open(path)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        let hash: Id = *h.finalize().as_bytes();
        let mut per = Vec::new();
        for c in cands {
            let mut v = Vec::new();
            c.for_each_chunk(path, size, &mut |b| {
                v.push((*blake3::hash(b).as_bytes(), b.len() as u32))
            })?;
            per.push(v);
        }
        Ok((hash, per))
    }
}

/// Chunks a tree; files that equal a file of `old` (same path, same hash) copy the old chunk lists.
/// Returns the records and the number of reused files.
pub fn scan(
    tree: &Tree,
    cands: &[Candidate],
    old: Option<&[FileRec]>,
) -> io::Result<(Vec<FileRec>, u64)> {
    let old_ix: HashMap<&str, &FileRec> = old
        .map(|o| o.iter().map(|r| (r.rel.as_str(), r)).collect())
        .unwrap_or_default();
    let res: io::Result<Vec<(FileRec, bool)>> = tree
        .files
        .par_iter()
        .map(|f| {
            // Hash first (cheap for reuse decision); chunk only if the file is not an unchanged file.
            let (hash, per) =
                if let Some(o) = old_ix.get(f.rel.as_str()).filter(|o| o.size == f.size) {
                    let h = file_hash(&f.abs, f.size)?;
                    if h == o.hash {
                        return Ok((
                            FileRec {
                                rel: f.rel.clone(),
                                size: f.size,
                                hash: h,
                                chunks: o.chunks.clone(),
                            },
                            true,
                        ));
                    }
                    chunk_one(&f.abs, f.size, cands)?
                } else {
                    chunk_one(&f.abs, f.size, cands)?
                };
            Ok((
                FileRec {
                    rel: f.rel.clone(),
                    size: f.size,
                    hash,
                    chunks: per,
                },
                false,
            ))
        })
        .collect();
    let v = res?;
    let reused = v.iter().filter(|x| x.1).count() as u64;
    Ok((v.into_iter().map(|x| x.0).collect(), reused))
}

fn file_hash(path: &Path, size: u64) -> io::Result<Id> {
    if size <= LOAD_LIMIT {
        return Ok(*blake3::hash(&fs::read(path)?).as_bytes());
    }
    let mut h = blake3::Hasher::new();
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(*h.finalize().as_bytes())
}

#[derive(Serialize, Clone, Default)]
pub struct CandStats {
    pub id: String,
    pub chunk_refs: u64,
    pub distinct_chunks: u64,
    pub t_bytes: u64,
    pub t_percent: f64,
    pub t_bytes_modified_files: u64,
    pub t_bytes_new_files: u64,
    pub t_bytes_moved_files: u64,
    pub mean_chunk_bytes: f64,
    /// chunks and bytes belonging to files larger than the candidate's max (the split files)
    pub split_files_chunks: u64,
    pub split_files_bytes: u64,
    pub chunk_size_histogram_log2: Vec<u64>,
    pub manifest_bytes: u64,
    /// T bytes attributed to the size class of the file in which each new chunk first occurs
    /// (classes: <=64 KiB, <=256 KiB, <=1 MiB, <=4 MiB, <=16 MiB, larger).
    pub t_bytes_by_size_class: Vec<u64>,
}

#[derive(Serialize, Clone, Default)]
pub struct PairStats {
    pub files_old: u64,
    pub files_new: u64,
    pub bytes_old: u64,
    pub bytes_new: u64,
    pub files_unchanged: u64,
    pub files_modified: u64,
    pub files_moved: u64,
    pub files_new_content: u64,
    pub files_deleted: u64,
    pub reused_files: u64,
    /// Empty files of the new version: zero chunks under the implemented rule (+1 chunk each under "one empty chunk").
    pub empty_files_new: u64,
    /// predecessor-like baseline: whole files whose (path, content) is not in the old version.
    pub t_file_level_path_percent: f64,
    /// Whole-file dedup by content only (a moved file costs nothing).
    pub t_file_level_content_percent: f64,
    pub bytes_new_by_size_class: Vec<u64>,
    pub bytes_modified_by_size_class: Vec<u64>,
    pub bytes_new_content_by_size_class: Vec<u64>,
    pub candidates: Vec<CandStats>,
}

pub fn compare(cands: &[Candidate], old: &[FileRec], new: &[FileRec], reused: u64) -> PairStats {
    let old_by_path: HashMap<&str, &FileRec> = old.iter().map(|r| (r.rel.as_str(), r)).collect();
    let old_hashes: HashSet<Id> = old.iter().map(|r| r.hash).collect();
    #[derive(Clone, Copy, PartialEq)]
    enum Cat {
        Unchanged,
        Modified,
        Moved,
        New,
    }
    let cats: Vec<Cat> = new
        .iter()
        .map(|r| match old_by_path.get(r.rel.as_str()) {
            Some(o) if o.hash == r.hash && o.size == r.size => Cat::Unchanged,
            Some(_) => Cat::Modified,
            None if old_hashes.contains(&r.hash) => Cat::Moved,
            None => Cat::New,
        })
        .collect();
    let new_paths: HashSet<&str> = new.iter().map(|r| r.rel.as_str()).collect();
    let mut ps = PairStats {
        files_old: old.len() as u64,
        files_new: new.len() as u64,
        bytes_old: old.iter().map(|r| r.size).sum(),
        bytes_new: new.iter().map(|r| r.size).sum(),
        empty_files_new: new.iter().filter(|r| r.size == 0).count() as u64,
        files_deleted: old
            .iter()
            .filter(|r| !new_paths.contains(r.rel.as_str()))
            .count() as u64,
        reused_files: reused,
        ..Default::default()
    };
    ps.bytes_new_by_size_class = vec![0; SIZE_CLASSES];
    ps.bytes_modified_by_size_class = vec![0; SIZE_CLASSES];
    ps.bytes_new_content_by_size_class = vec![0; SIZE_CLASSES];
    for (r, c) in new.iter().zip(&cats) {
        let k = size_class(r.size);
        ps.bytes_new_by_size_class[k] += r.size;
        match c {
            Cat::Modified => ps.bytes_modified_by_size_class[k] += r.size,
            Cat::New => ps.bytes_new_content_by_size_class[k] += r.size,
            _ => {}
        }
        match c {
            Cat::Unchanged => ps.files_unchanged += 1,
            Cat::Modified => ps.files_modified += 1,
            Cat::Moved => ps.files_moved += 1,
            Cat::New => ps.files_new_content += 1,
        }
    }
    let bn = ps.bytes_new.max(1) as f64;
    let mut tp = 0u64;
    let mut tc = 0u64;
    for (r, c) in new.iter().zip(&cats) {
        if *c != Cat::Unchanged {
            tp += r.size;
        }
        if matches!(c, Cat::Modified | Cat::New) && !old_hashes.contains(&r.hash) {
            tc += r.size;
        }
    }
    ps.t_file_level_path_percent = 100.0 * tp as f64 / bn;
    ps.t_file_level_content_percent = 100.0 * tc as f64 / bn;
    for (ci, cand) in cands.iter().enumerate() {
        let old_set: HashSet<Id> = old
            .iter()
            .flat_map(|r| r.chunks[ci].iter().map(|x| x.0))
            .collect();
        let mut seen: HashSet<Id> = HashSet::new();
        let mut cs = CandStats {
            id: cand.id.clone(),
            chunk_size_histogram_log2: vec![0; HIST],
            t_bytes_by_size_class: vec![0; SIZE_CLASSES],
            ..Default::default()
        };
        let mut total_bytes = 0u64;
        for (r, cat) in new.iter().zip(&cats) {
            if r.size > cand.max {
                cs.split_files_chunks += r.chunks[ci].len() as u64;
                cs.split_files_bytes += r.size;
            }
            for (id, sz) in &r.chunks[ci] {
                cs.chunk_refs += 1;
                total_bytes += *sz as u64;
                let b = (63 - (*sz as u64).max(1).leading_zeros() as usize).min(HIST - 1);
                cs.chunk_size_histogram_log2[b] += 1;
                if !old_set.contains(id) && seen.insert(*id) {
                    cs.t_bytes += *sz as u64;
                    cs.t_bytes_by_size_class[size_class(r.size)] += *sz as u64;
                    match cat {
                        Cat::Modified => cs.t_bytes_modified_files += *sz as u64,
                        Cat::New => cs.t_bytes_new_files += *sz as u64,
                        Cat::Moved => cs.t_bytes_moved_files += *sz as u64,
                        Cat::Unchanged => {}
                    }
                }
            }
        }
        cs.distinct_chunks = new
            .iter()
            .flat_map(|r| r.chunks[ci].iter().map(|x| x.0))
            .collect::<HashSet<_>>()
            .len() as u64;
        cs.t_percent = 100.0 * cs.t_bytes as f64 / bn;
        cs.mean_chunk_bytes = if cs.chunk_refs > 0 {
            total_bytes as f64 / cs.chunk_refs as f64
        } else {
            0.0
        };
        cs.manifest_bytes = crate::schema_adapter::measured_len(new, ci, cand);
        ps.candidates.push(cs);
    }
    ps
}

pub fn run_pair(cands: &[Candidate], old_dir: &Path, new_dir: &Path) -> io::Result<PairStats> {
    let to = walk(old_dir)?;
    let tn = walk(new_dir)?;
    let (old, _) = scan(&to, cands, None)?;
    let (new, reused) = scan(&tn, cands, Some(&old))?;
    Ok(compare(cands, &old, &new, reused))
}
