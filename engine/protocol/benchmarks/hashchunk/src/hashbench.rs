// SPDX-License-Identifier: 0BSD
//! Hash pipeline benchmark: SHA-256 versus BLAKE3 in the same pipeline.
//!
//! Pipeline: one reader/chunker thread (configurable) reads files and cuts chunks, a fixed pool of N workers hashes
//! them. Two paths are measured separately:
//! * build path: read, chunk (production rules), hash chunks (and the whole-file hash of split files);
//! * verify path: read with the boundaries recorded by a build run (no chunking), hash and compare.
//!
//! A third stage, `chunk-only`, runs the reader/chunker without hashing to show where the pipeline saturates.
//! Warm = data in the page cache before the run (checked with mincore); cold = dropped with posix_fadvise(DONTNEED)
//! before every run, residency checked afterwards of the drop.

use crate::cache;
use crate::chunking::Candidate;
use crate::quiet;
use crate::treewalk::Tree;
use crate::util::{io_err, median};
use crossbeam_channel::{bounded, Receiver, Sender};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
    Sha256,
    Blake3,
}

pub fn hash_bytes(a: Algo, d: &[u8]) -> [u8; 32] {
    match a {
        Algo::Sha256 => {
            let o = Sha256::digest(d);
            let mut r = [0u8; 32];
            r.copy_from_slice(&o);
            r
        }
        Algo::Blake3 => *blake3::hash(d).as_bytes(),
    }
}

fn hash_file_stream(a: Algo, path: &Path, buf: &mut Vec<u8>) -> io::Result<[u8; 32]> {
    let mut f = File::open(path)?;
    buf.resize(1 << 20, 0);
    match a {
        Algo::Sha256 => {
            let mut h = Sha256::new();
            loop {
                let n = f.read(buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let o = h.finalize();
            let mut r = [0u8; 32];
            r.copy_from_slice(&o);
            Ok(r)
        }
        Algo::Blake3 => {
            let mut h = blake3::Hasher::new();
            loop {
                let n = f.read(buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(*h.finalize().as_bytes())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Build,
    Verify,
    ChunkOnly,
}

#[derive(Clone)]
pub struct InFile {
    pub path: PathBuf,
    pub size: u64,
}

/// Boundaries and hashes recorded by a build run.
#[derive(Clone, Default)]
pub struct FileIndex {
    pub chunks: Vec<(u32, [u8; 32])>,
    pub file_hash: Option<[u8; 32]>,
}

struct Piece {
    file: u32,
    idx: u32,
    data: Vec<u8>,
}

enum Job {
    Batch(Vec<Piece>),
    Whole(u32),
}

struct Done {
    file: u32,
    idx: u32, // u32::MAX = whole-file hash
    len: u32,
    hash: [u8; 32],
}

#[derive(Default)]
pub struct RunOut {
    pub seconds: f64,
    pub chunks: u64,
    pub index: Vec<FileIndex>,
    pub mismatches: u64,
}

pub struct RunCfg<'a> {
    pub algo: Algo,
    pub workers: usize,
    pub readers: usize,
    pub stage: Stage,
    pub cand: &'a Candidate,
    pub file_hash: bool,
}

const BATCH_BYTES: usize = 512 * 1024;
const BATCH_PIECES: usize = 256;

/// Consume every handle, including after an I/O error or panic. Dropping a JoinHandle detaches its thread.
fn join_all<T>(
    handles: Vec<std::thread::JoinHandle<io::Result<T>>>,
    panic_message: &str,
) -> io::Result<Vec<T>> {
    let mut values = Vec::new();
    let mut error = None;
    for handle in handles {
        match handle
            .join()
            .unwrap_or_else(|_| Err(io::Error::other(panic_message.to_owned())))
        {
            Ok(value) => values.push(value),
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(values),
    }
}

fn flush(batch: &mut Vec<Piece>, bytes: &mut usize, tx: &Option<Sender<Job>>) {
    if !batch.is_empty() {
        if let Some(tx) = tx {
            let _ = tx.send(Job::Batch(std::mem::take(batch)));
        } else {
            batch.clear();
        }
        *bytes = 0;
    }
}

/// One timed pipeline run. `expected` must be given for `Stage::Verify`.
pub fn run(files: &[InFile], cfg: &RunCfg, expected: Option<&[FileIndex]>) -> io::Result<RunOut> {
    if cfg.stage == Stage::Verify && expected.is_none_or(|e| e.len() != files.len()) {
        return Err(io::Error::other("verify needs one index per file"));
    }
    let start = Instant::now();
    let hashing = cfg.stage != Stage::ChunkOnly;
    let (tx, rx): (Sender<Job>, Receiver<Job>) = bounded(cfg.workers.max(1) * 4);
    let files_arc: Arc<Vec<InFile>> = Arc::new(files.to_vec());
    let next = Arc::new(AtomicUsize::new(0));
    let chunk_count = Arc::new(AtomicUsize::new(0));
    let structural = Arc::new(AtomicU64::new(0));
    let cand = cfg.cand.clone();
    let algo = cfg.algo;
    let do_file_hash = cfg.file_hash;
    let stage = cfg.stage;
    let exp: Option<Arc<Vec<Vec<u32>>>> = expected.map(|e| {
        Arc::new(
            e.iter()
                .map(|f| f.chunks.iter().map(|c| c.0).collect())
                .collect(),
        )
    });

    let mut readers = Vec::new();
    for _ in 0..cfg.readers.max(1) {
        let (files, next, cc, cand, exp, structural) = (
            files_arc.clone(),
            next.clone(),
            chunk_count.clone(),
            cand.clone(),
            exp.clone(),
            structural.clone(),
        );
        let txo = if hashing { Some(tx.clone()) } else { None };
        readers.push(std::thread::spawn(move || -> io::Result<()> {
            let mut batch: Vec<Piece> = Vec::new();
            let mut bytes = 0usize;
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= files.len() {
                    break;
                }
                let f = &files[i];
                let push = |idx: u32, data: Vec<u8>, batch: &mut Vec<Piece>, bytes: &mut usize| {
                    cc.fetch_add(1, Ordering::Relaxed);
                    *bytes += data.len();
                    batch.push(Piece {
                        file: i as u32,
                        idx,
                        data,
                    });
                    if *bytes >= BATCH_BYTES || batch.len() >= BATCH_PIECES {
                        flush(batch, bytes, &txo);
                    }
                };
                let split = f.size > cand.max;
                match stage {
                    Stage::Verify => {
                        let lens = &exp.as_ref().unwrap()[i];
                        let mut file = File::open(&f.path)?;
                        // Completeness: the file must have exactly the size the index covers (appended data,
                        // truncation and changes to originally empty files are caught here).
                        let expected_len: u64 = lens.iter().map(|l| *l as u64).sum();
                        if file.metadata()?.len() != expected_len {
                            structural.fetch_add(1, Ordering::Relaxed);
                        }
                        for (k, l) in lens.iter().enumerate() {
                            let mut buf = vec![0u8; *l as usize];
                            match file.read_exact(&mut buf) {
                                Ok(()) => push(k as u32, buf, &mut batch, &mut bytes),
                                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                                    structural.fetch_add(1, Ordering::Relaxed);
                                    break;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        let mut probe = [0u8; 1];
                        if file.read(&mut probe)? != 0 {
                            structural.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Stage::Build | Stage::ChunkOnly => {
                        let mut k = 0u32;
                        cand.for_each_chunk(&f.path, f.size, &mut |b| {
                            push(k, b.to_vec(), &mut batch, &mut bytes);
                            k += 1;
                        })?;
                    }
                }
                if split && do_file_hash && hashing {
                    flush(&mut batch, &mut bytes, &txo);
                    if let Some(t) = &txo {
                        let _ = t.send(Job::Whole(i as u32));
                    }
                }
            }
            flush(&mut batch, &mut bytes, &txo);
            Ok(())
        }));
    }
    drop(tx);

    let mut workers = Vec::new();
    if hashing {
        for _ in 0..cfg.workers.max(1) {
            let (rx, files) = (rx.clone(), files_arc.clone());
            workers.push(std::thread::spawn(move || -> io::Result<Vec<Done>> {
                let mut out = Vec::new();
                let mut buf = Vec::new();
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Batch(b) => {
                            for p in b {
                                let h = hash_bytes(algo, &p.data);
                                out.push(Done {
                                    file: p.file,
                                    idx: p.idx,
                                    len: p.data.len() as u32,
                                    hash: h,
                                });
                            }
                        }
                        Job::Whole(i) => {
                            let h = hash_file_stream(algo, &files[i as usize].path, &mut buf)?;
                            out.push(Done {
                                file: i,
                                idx: u32::MAX,
                                len: 0,
                                hash: h,
                            });
                        }
                    }
                }
                Ok(out)
            }));
        }
    }
    drop(rx);
    // Join BOTH groups before propagating an error: no own work may outlive run().
    let readers_result = join_all(readers, "reader panicked");
    let workers_result = join_all(workers, "worker panicked");
    readers_result?;
    let mut done: Vec<Done> = workers_result?.into_iter().flatten().collect();
    let mut out = RunOut {
        seconds: 0.0,
        chunks: chunk_count.load(Ordering::Relaxed) as u64,
        mismatches: structural.load(Ordering::Relaxed),
        ..Default::default()
    };
    if cfg.stage == Stage::Build {
        let mut index = vec![FileIndex::default(); files.len()];
        for (file, entry) in files.iter().zip(&mut index) {
            if file.size == 0 {
                entry.file_hash = Some(hash_bytes(algo, &[]));
            }
        }
        done.sort_by_key(|d| (d.file, d.idx));
        for d in done {
            let fi = &mut index[d.file as usize];
            if d.idx == u32::MAX {
                fi.file_hash = Some(d.hash);
            } else {
                fi.chunks.push((d.len, d.hash));
            }
        }
        out.index = index;
    } else if cfg.stage == Stage::Verify {
        let e = expected.ok_or_else(|| io_err("verify needs an index"))?;
        for (file, entry) in files.iter().zip(e) {
            if file.size == 0 && entry.file_hash != Some(hash_bytes(algo, &[])) {
                out.mismatches += 1;
            }
        }
        for d in done {
            let ef = &e[d.file as usize];
            let ok = if d.idx == u32::MAX {
                ef.file_hash == Some(d.hash)
            } else {
                ef.chunks.get(d.idx as usize).map(|c| c.1) == Some(d.hash)
            };
            if !ok {
                out.mismatches += 1;
            }
        }
    }
    // End to end: includes index assembly (build) and the comparison with the recorded hashes (verify).
    out.seconds = start.elapsed().as_secs_f64();
    Ok(out)
}

#[derive(Serialize)]
pub struct Condition {
    pub algorithm: Algo,
    pub workers: usize,
    pub readers: usize,
    pub stage: Stage,
    pub cache: &'static str,
    pub seconds_each_rep: Vec<f64>,
    pub seconds_median: f64,
    pub mib_per_s_median: f64,
    pub bytes: u64,
    pub files: usize,
    pub chunks: u64,
    pub resident_fraction_before_each_rep: Vec<f64>,
    pub busy_percent_before_each_rep: Vec<f64>,
    pub quiet_before_each_rep: Vec<quiet::Wait>,
    pub cache_state_valid: bool,
    /// "valid", "invalid" or "unknown" (no page-cache probe on this platform: informative only).
    pub cache_state: &'static str,
    pub mismatches: u64,
}

pub struct BenchCfg {
    pub algos: Vec<Algo>,
    pub workers: Vec<usize>,
    pub readers: usize,
    pub stages: Vec<Stage>,
    pub caches: Vec<&'static str>,
    pub reps: usize,
    pub cand: Candidate,
    pub file_hash: bool,
    pub max_busy_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct QuietAbort {
    pub algorithm: Algo,
    pub workers: usize,
    pub readers: usize,
    pub stage: Stage,
    pub cache: &'static str,
    pub repetition: usize,
    pub limit_percent: f64,
    pub quiet_before_each_rep: Vec<quiet::Wait>,
}

pub struct BenchOutcome {
    pub conditions: Vec<Condition>,
    pub quiet_abort: Option<QuietAbort>,
}

fn prepare_cache(files: &[InFile], kind: &str) {
    if kind == "cold" {
        for f in files {
            cache::drop_file(&f.path);
        }
    } else {
        let mut buf = vec![0u8; 1 << 20];
        for f in files {
            if let Ok(mut h) = File::open(&f.path) {
                while h.read(&mut buf).map(|n| n > 0).unwrap_or(false) {}
            }
        }
    }
}

pub fn bench(tree: &Tree, c: &BenchCfg) -> io::Result<BenchOutcome> {
    let files: Vec<InFile> = tree
        .files
        .iter()
        .map(|f| InFile {
            path: f.abs.clone(),
            size: f.size,
        })
        .collect();
    let cache_files: Vec<(PathBuf, u64)> = files.iter().map(|f| (f.path.clone(), f.size)).collect();
    let bytes: u64 = files.iter().map(|f| f.size).sum();
    let mut out = Vec::new();
    let mut indexes: std::collections::BTreeMap<Algo, Vec<FileIndex>> = Default::default();
    for &algo in &c.algos {
        // Untimed build to obtain the boundaries and hashes for the verify path (also warms the cache).
        let r = run(
            &files,
            &RunCfg {
                algo,
                workers: 1,
                readers: c.readers,
                stage: Stage::Build,
                cand: &c.cand,
                file_hash: c.file_hash,
            },
            None,
        )?;
        indexes.insert(algo, r.index);
    }
    let mut stages = c.stages.clone();
    stages.sort_by_key(|s| match s {
        Stage::ChunkOnly => 0,
        Stage::Build => 1,
        Stage::Verify => 2,
    });
    for &stage in &stages {
        for &cache_kind in &c.caches {
            let algo_list: Vec<Algo> = if stage == Stage::ChunkOnly {
                vec![c.algos[0]]
            } else {
                c.algos.clone()
            };
            let worker_list: Vec<usize> = if stage == Stage::ChunkOnly {
                vec![0]
            } else {
                c.workers.clone()
            };
            for &algo in &algo_list {
                for &w in &worker_list {
                    let mut secs = Vec::new();
                    let mut res = Vec::new();
                    let mut busy = Vec::new();
                    let mut quiet_waits = Vec::new();
                    let mut chunks = 0;
                    let mut mism = 0;
                    for repetition in 1..=c.reps {
                        // run() has joined every reader/worker; prior RunOut allocations have been dropped.
                        // Cache preparation is synchronous but can leave kernel work behind: settle AFTER it.
                        prepare_cache(&files, cache_kind);
                        if let Some(limit) = c.max_busy_percent {
                            let wait = quiet::wait(limit);
                            let ready = wait.ready;
                            if let Some(b) = wait.samples.last().and_then(|s| s.busy) {
                                busy.push(b.external_percent);
                            }
                            quiet_waits.push(wait);
                            if !ready {
                                return Ok(BenchOutcome {
                                    conditions: out,
                                    quiet_abort: Some(QuietAbort {
                                        algorithm: algo,
                                        workers: w,
                                        readers: c.readers,
                                        stage,
                                        cache: cache_kind,
                                        repetition,
                                        limit_percent: limit,
                                        quiet_before_each_rep: quiet_waits,
                                    }),
                                });
                            }
                        }
                        // Recheck residency after settling, immediately before timing (no second cache preparation).
                        res.push(cache::resident_fraction(&cache_files));
                        let r = run(
                            &files,
                            &RunCfg {
                                algo,
                                workers: w,
                                readers: c.readers,
                                stage,
                                cand: &c.cand,
                                file_hash: c.file_hash,
                            },
                            indexes.get(&algo).map(|v| v.as_slice()),
                        )?;
                        secs.push(r.seconds);
                        mism += r.mismatches;
                        chunks = r.chunks;
                        // In particular, free the build index before the next cache preparation / quiet gate.
                    }
                    let med = median(&secs);
                    let unknown = res.iter().any(|f| *f < 0.0);
                    let valid = !unknown
                        && res.iter().all(|f| {
                            if cache_kind == "cold" {
                                *f <= 0.01
                            } else {
                                *f >= 0.99
                            }
                        });
                    let cache_state = if unknown {
                        "unknown"
                    } else if valid {
                        "valid"
                    } else {
                        "invalid"
                    };
                    out.push(Condition {
                        algorithm: algo,
                        workers: w,
                        readers: c.readers,
                        stage,
                        cache: cache_kind,
                        seconds_each_rep: secs,
                        seconds_median: med,
                        mib_per_s_median: if med > 0.0 {
                            bytes as f64 / 1048576.0 / med
                        } else {
                            0.0
                        },
                        bytes,
                        files: files.len(),
                        chunks,
                        resident_fraction_before_each_rep: res,
                        busy_percent_before_each_rep: busy,
                        quiet_before_each_rep: quiet_waits,
                        cache_state_valid: valid,
                        cache_state,
                        mismatches: mism,
                    });
                }
            }
        }
    }
    Ok(BenchOutcome {
        conditions: out,
        quiet_abort: None,
    })
}

#[derive(Serialize)]
pub struct Micro {
    pub algorithm: Algo,
    pub threads: usize,
    pub piece_bytes: usize,
    pub mib_per_s: f64,
}

/// Memory-only hashing speed (no I/O, no chunking): one buffer per thread, hashed in `piece` sized calls.
pub fn micro(algos: &[Algo], threads: &[usize]) -> Vec<Micro> {
    let mut out = Vec::new();
    let total_per_thread: usize = 512 << 20;
    for &a in algos {
        for &t in threads {
            for piece in [64 << 10, 1 << 20] {
                let start = Instant::now();
                let hs: Vec<_> = (0..t)
                    .map(|k| {
                        std::thread::spawn(move || {
                            let mut buf = vec![0u8; 16 << 20];
                            crate::corpus_model::Rng::new(k as u64 + 1).fill(&mut buf);
                            let t0 = Instant::now();
                            let mut n = 0usize;
                            let mut acc = 0u8;
                            while n < total_per_thread {
                                for ch in buf.chunks(piece) {
                                    acc ^= hash_bytes(a, ch)[0];
                                    n += ch.len();
                                }
                            }
                            (t0.elapsed().as_secs_f64(), acc)
                        })
                    })
                    .collect();
                let mut worst = 0.0f64;
                for h in hs {
                    let (s, _) = h.join().unwrap();
                    worst = worst.max(s);
                }
                let _ = start;
                out.push(Micro {
                    algorithm: a,
                    threads: t,
                    piece_bytes: piece,
                    mib_per_s: (total_per_thread * t) as f64 / 1048576.0 / worst,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunking::cdc;

    #[test]
    fn joins_remaining_threads_after_io_error_or_panic() {
        for panic in [false, true] {
            let completed = Arc::new(AtomicUsize::new(0));
            let first = std::thread::spawn(move || -> io::Result<()> {
                if panic {
                    panic!("intentional reader failure");
                }
                Err(io::Error::other("intentional I/O failure"))
            });
            let handles = std::iter::once(first)
                .chain((0..4).map(|_| {
                    let completed = completed.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                }))
                .collect();
            assert!(join_all(handles, "reader panicked").is_err());
            assert_eq!(
                completed.load(Ordering::SeqCst),
                4,
                "all remaining threads must finish before returning an error"
            );
        }
    }

    #[test]
    fn verify_detects_corruption_and_matches_independent_hash() {
        let dir = std::env::temp_dir().join(format!("chunkbench-hb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = Vec::new();
        for (i, n) in [0usize, 10, 300_000, 5_000_000].iter().enumerate() {
            let mut d = vec![0u8; *n];
            crate::corpus_model::Rng::new(i as u64 + 1).fill(&mut d);
            let p = dir.join(format!("f{i}"));
            std::fs::write(&p, &d).unwrap();
            files.push(InFile {
                path: p,
                size: *n as u64,
            });
        }
        let cand = cdc(256 * 1024);
        for algo in [Algo::Sha256, Algo::Blake3] {
            let cfg = RunCfg {
                algo,
                workers: 3,
                readers: 2,
                stage: Stage::Build,
                cand: &cand,
                file_hash: true,
            };
            let b = run(&files, &cfg, None).unwrap();
            // independent check of every recorded chunk hash and the whole-file hash
            for (f, fi) in files.iter().zip(&b.index) {
                let data = std::fs::read(&f.path).unwrap();
                let mut off = 0usize;
                for (len, h) in &fi.chunks {
                    assert_eq!(hash_bytes(algo, &data[off..off + *len as usize]), *h);
                    off += *len as usize;
                }
                assert_eq!(off, data.len());
                if f.size > cand.max {
                    assert_eq!(fi.file_hash, Some(hash_bytes(algo, &data)));
                } else if f.size == 0 {
                    assert_eq!(fi.file_hash, Some(hash_bytes(algo, &[])));
                } else {
                    assert_eq!(fi.file_hash, None);
                }
            }
            let v = run(
                &files,
                &RunCfg {
                    stage: Stage::Verify,
                    ..cfg
                },
                Some(&b.index),
            )
            .unwrap();
            assert_eq!(v.mismatches, 0);
            // corrupt one byte in the large file
            let mut d = std::fs::read(&files[3].path).unwrap();
            d[123_456] ^= 1;
            std::fs::write(&files[3].path, &d).unwrap();
            let v = run(
                &files,
                &RunCfg {
                    algo,
                    workers: 2,
                    readers: 1,
                    stage: Stage::Verify,
                    cand: &cand,
                    file_hash: true,
                },
                Some(&b.index),
            )
            .unwrap();
            assert!(v.mismatches >= 1, "corruption must be detected");
            d[123_456] ^= 1;
            std::fs::write(&files[3].path, &d).unwrap();
            let v = run(
                &files,
                &RunCfg {
                    algo,
                    workers: 2,
                    readers: 1,
                    stage: Stage::Verify,
                    cand: &cand,
                    file_hash: true,
                },
                Some(&b.index),
            )
            .unwrap();
            assert_eq!(
                v.mismatches, 0,
                "restoring the byte must make the verify run green again"
            );
            // appended data: single-chunk file (10 bytes), split file, originally empty file; each exactly 1 finding
            for (i, label) in [(0usize, "empty"), (1, "one-chunk"), (3, "split")] {
                let orig = std::fs::read(&files[i].path).unwrap();
                let mut app = orig.clone();
                app.push(7);
                std::fs::write(&files[i].path, &app).unwrap();
                let v = run(
                    &files,
                    &RunCfg {
                        algo,
                        workers: 2,
                        readers: 1,
                        stage: Stage::Verify,
                        cand: &cand,
                        file_hash: true,
                    },
                    Some(&b.index),
                )
                .unwrap();
                assert!(v.mismatches >= 1, "appended byte undetected ({label})");
                std::fs::write(&files[i].path, &orig).unwrap();
                assert_eq!(
                    run(
                        &files,
                        &RunCfg {
                            stage: Stage::Verify,
                            ..cfg
                        },
                        Some(&b.index)
                    )
                    .unwrap()
                    .mismatches,
                    0
                );
            }
            // truncation of a single-chunk file and of the split file
            for i in [1usize, 3] {
                let orig = std::fs::read(&files[i].path).unwrap();
                std::fs::write(&files[i].path, &orig[..orig.len() - 1]).unwrap();
                let v = run(
                    &files,
                    &RunCfg {
                        algo,
                        workers: 2,
                        readers: 1,
                        stage: Stage::Verify,
                        cand: &cand,
                        file_hash: true,
                    },
                    Some(&b.index),
                )
                .unwrap();
                assert!(v.mismatches >= 1, "truncation undetected (file {i})");
                std::fs::write(&files[i].path, &orig).unwrap();
            }
            let v = run(
                &files,
                &RunCfg {
                    algo,
                    workers: 2,
                    readers: 1,
                    stage: Stage::Verify,
                    cand: &cand,
                    file_hash: true,
                },
                Some(&b.index),
            )
            .unwrap();
            assert_eq!(v.mismatches, 0, "all files restored");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod pipeline_error_tests {
    use super::*;
    #[test]
    fn reader_io_failure_is_propagated_with_workers_running() {
        let missing = std::env::temp_dir()
            .join(format!("chunkbench-missing-{}", std::process::id()))
            .join("file");
        let files = [InFile {
            path: missing,
            size: 10,
        }];
        let cand = crate::chunking::cdc(256 * 1024);
        for algo in [Algo::Sha256, Algo::Blake3] {
            let cfg = RunCfg {
                algo,
                workers: 3,
                readers: 2,
                stage: Stage::Build,
                cand: &cand,
                file_hash: true,
            };
            assert!(run(&files, &cfg, None).is_err());
        }
    }
}
