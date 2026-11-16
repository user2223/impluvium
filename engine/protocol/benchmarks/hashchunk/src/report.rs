// SPDX-License-Identifier: 0BSD
//! Command implementations and measurement reports.

use crate::analysis::{self, CandStats, Config, PairStats};
use crate::chunking::{self, Candidate, Family};
use crate::hashbench::{self, Algo, BenchCfg, Stage};
use crate::select::{self, ClassT};
use crate::treewalk::walk;
use crate::{gen, model, schema_adapter, sysinfo, util};
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

pub enum Command {
    Gen {
        out: PathBuf,
        seed: u64,
        files: usize,
        symlinks: usize,
        boundary: bool,
    },
    Analyze {
        config: PathBuf,
        out: PathBuf,
        threads: Option<usize>,
        no_model: bool,
    },
    Hash {
        tree: PathBuf,
        out: PathBuf,
        algos: String,
        workers: String,
        stages: String,
        cache: String,
        reps: usize,
        readers: usize,
        chunking: String,
        max_busy_percent: Option<f64>,
        no_file_hash: bool,
        no_micro: bool,
        label: String,
    },
    GenFilter {
        src: PathBuf,
        out: PathBuf,
        seed: u64,
        files: usize,
    },
    Digest {
        dir: PathBuf,
    },
    Sysinfo,
    Model {
        out: Option<PathBuf>,
    },
}

pub fn run(c: Command, metadata: &sysinfo::RunMetadata) -> io::Result<()> {
    match c {
        Command::Gen {
            out,
            seed,
            files,
            symlinks,
            boundary,
        } => {
            let p = gen::GenParams {
                seed,
                files,
                symlinks,
                boundary,
            };
            progress!(
                "generating {} files (seed {seed}) below {}",
                files,
                out.display()
            );
            let mut rep = gen::generate(&out, &p)?;
            let d1 = digest(&out.join("v1"))?;
            let d2 = digest(&out.join("v2"))?;
            rep.version_digests = Some((d1, d2));
            let json = serde_json::to_string_pretty(&rep).map_err(util::io_err)?;
            std::fs::write(out.join("gen.json"), &json)?;
            println!("{json}");
            Ok(())
        }
        Command::GenFilter {
            src,
            out,
            seed,
            files,
        } => {
            let p = gen::GenParams {
                seed,
                files,
                symlinks: 0,
                boundary: true,
            };
            let rep = gen::filter_boundary(&src, &out, &p)?;
            let d1 = digest(&out.join("v1"))?;
            let d2 = digest(&out.join("v2"))?;
            let json = serde_json::json!({"filter": rep, "tree_digests": [d1, d2]});
            let text = serde_json::to_string_pretty(&json).map_err(util::io_err)?;
            std::fs::write(out.join("gen-filter.json"), &text)?;
            println!("{text}");
            Ok(())
        }
        Command::Digest { dir } => {
            println!("{}", digest(&dir)?);
            Ok(())
        }
        Command::Sysinfo => {
            println!(
                "{}",
                serde_json::to_string_pretty(&sysinfo::collect(metadata)).map_err(util::io_err)?
            );
            Ok(())
        }
        Command::Model { out } => {
            let rows = model::rows(
                &chunking::grid(),
                &BTreeMap::new(),
                model::MODEL_FILES,
                model::MODEL_BYTES,
            );
            let s = serde_json::to_string_pretty(&serde_json::json!({"model_id": schema_adapter::MODEL_ID, "size_model_only": true, "rows": rows})).map_err(util::io_err)?;
            if let Some(o) = out {
                std::fs::write(o, &s)?;
            }
            println!("{s}");
            Ok(())
        }
        Command::Analyze {
            config,
            out,
            threads,
            no_model,
        } => analyze(config, out, threads, no_model, metadata),
        Command::Hash {
            tree,
            out,
            algos,
            workers,
            stages,
            cache,
            reps,
            readers,
            chunking,
            max_busy_percent,
            no_file_hash,
            no_micro,
            label,
        } => hash(
            tree,
            out,
            algos,
            workers,
            stages,
            cache,
            reps,
            readers,
            chunking,
            max_busy_percent,
            no_file_hash,
            no_micro,
            label,
            metadata,
        ),
    }
}

/// Reproducible tree digest: SHA-256 over sorted lines `path NUL size NUL mode NUL sha256(file)` and symlink lines.
pub fn digest(dir: &std::path::Path) -> io::Result<String> {
    let t = walk(dir)?;
    let lines: io::Result<Vec<String>> = t
        .files
        .par_iter()
        .map(|f| {
            let mut h = Sha256::new();
            let mut file = std::fs::File::open(&f.abs)?;
            io::copy(&mut file, &mut Sha256Writer(&mut h))?;
            Ok(format!(
                "F\0{}\0{}\0{:o}\0{}\n",
                f.rel,
                f.size,
                f.mode,
                util::hex(&h.finalize())
            ))
        })
        .collect();
    let mut all = Sha256::new();
    for l in lines? {
        all.update(l.as_bytes());
    }
    for (p, tgt) in &t.symlinks {
        all.update(format!("L\0{p}\0{tgt}\n").as_bytes());
    }
    Ok(util::hex(&all.finalize()))
}

struct Sha256Writer<'a>(&'a mut Sha256);
impl io::Write for Sha256Writer<'_> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct ClassCand {
    id: String,
    t_mean_percent: f64,
    chunk_refs_sum: u64,
    distinct_chunks_sum: u64,
    mean_chunk_bytes: f64,
    mean_split_chunk_bytes: f64,
    manifest_bytes_sum: u64,
    chunk_size_histogram_log2: Vec<u64>,
    t_bytes_by_size_class: Vec<u64>,
}

#[derive(Serialize)]
struct ClassReport {
    name: String,
    kind: String,
    pairs: usize,
    files_new_sum: u64,
    bytes_new_sum: u64,
    files_unchanged_sum: u64,
    files_modified_sum: u64,
    files_moved_sum: u64,
    files_new_content_sum: u64,
    files_deleted_sum: u64,
    empty_files_new_sum: u64,
    t_file_level_path_mean_percent: f64,
    t_file_level_content_mean_percent: f64,
    bytes_new_by_size_class: Vec<u64>,
    bytes_modified_by_size_class: Vec<u64>,
    bytes_new_content_by_size_class: Vec<u64>,
    candidates: Vec<ClassCand>,
    pairs_detail: Vec<PairDetail>,
}

#[derive(Serialize)]
struct PairDetail {
    label: String,
    stats: PairStats,
}

#[derive(Serialize)]
struct AnalyzeReport {
    tool: &'static str,
    model_id: &'static str,
    size_model_only: bool,
    machine: sysinfo::Machine,
    parameters: serde_json::Value,
    classes: Vec<ClassReport>,
    selection: select::Selection,
    model_100gb: Option<Vec<model::ModelRow>>,
    notes: Vec<String>,
}

fn analyze(
    config: PathBuf,
    out: PathBuf,
    threads: Option<usize>,
    no_model: bool,
    metadata: &sysinfo::RunMetadata,
) -> io::Result<()> {
    if let Some(t) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .map_err(util::io_err)?;
    }
    let mut cfg: Config = serde_json::from_slice(&std::fs::read(&config)?).map_err(util::io_err)?;
    let base = config.parent().unwrap_or_else(|| std::path::Path::new("."));
    for cl in &mut cfg.classes {
        for pair in &mut cl.pairs {
            pair.old = base.join(&pair.old).to_string_lossy().into_owned();
            pair.new = base.join(&pair.new).to_string_lossy().into_owned();
        }
    }
    let mut cands = chunking::grid();
    if let Some(ids) = &cfg.candidates {
        cands.retain(|c| ids.contains(&c.id));
    }
    let mut classes = Vec::new();
    let mut class_t: Vec<ClassT> = Vec::new();
    let mut class_kinds: Vec<String> = Vec::new();
    let mut split_acc: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for cl in &cfg.classes {
        let cname = cl.name.clone();
        progress!("class {}: {} pair(s)", cl.name, cl.pairs.len());
        let mut pairs: Vec<(String, PairStats)> = Vec::new();
        for p in &cl.pairs {
            progress!("  pair {}", p.label);
            let st = analysis::run_pair(&cands, &PathBuf::from(&p.old), &PathBuf::from(&p.new))?;
            pairs.push((p.label.clone(), st));
        }
        if pairs.is_empty() || pairs.iter().any(|p| p.1.bytes_new == 0) {
            return Err(io::Error::other("empty pair or class"));
        }
        let n = pairs.len() as f64;
        let mut cc = Vec::new();
        let mut tmap = BTreeMap::new();
        for (k, cand) in cands.iter().enumerate() {
            let cs: Vec<&CandStats> = pairs.iter().map(|p| &p.1.candidates[k]).collect();
            let t_mean = cs.iter().map(|c| c.t_percent).sum::<f64>() / n;
            tmap.insert(cand.id.clone(), t_mean);
            let refs: u64 = cs.iter().map(|c| c.chunk_refs).sum();
            let tot_bytes: f64 = pairs.iter().map(|p| p.1.bytes_new as f64).sum();
            let sc: u64 = cs.iter().map(|c| c.split_files_chunks).sum();
            let sb: u64 = cs.iter().map(|c| c.split_files_bytes).sum();
            let e = split_acc.entry(cand.id.clone()).or_default();
            e.0 += sc;
            e.1 += sb;
            let mut hist = vec![0u64; cs[0].chunk_size_histogram_log2.len()];
            for c in &cs {
                for (h, v) in hist.iter_mut().zip(&c.chunk_size_histogram_log2) {
                    *h += v;
                }
            }
            let mut tsc = vec![0u64; analysis::SIZE_CLASSES];
            for c in &cs {
                for (h, v) in tsc.iter_mut().zip(&c.t_bytes_by_size_class) {
                    *h += v;
                }
            }
            cc.push(ClassCand {
                id: cand.id.clone(),
                t_mean_percent: t_mean,
                chunk_refs_sum: refs,
                distinct_chunks_sum: cs.iter().map(|c| c.distinct_chunks).sum(),
                mean_chunk_bytes: if refs > 0 {
                    tot_bytes / refs as f64
                } else {
                    0.0
                },
                mean_split_chunk_bytes: if sc > 0 { sb as f64 / sc as f64 } else { 0.0 },
                manifest_bytes_sum: cs.iter().map(|c| c.manifest_bytes).sum(),
                chunk_size_histogram_log2: hist,
                t_bytes_by_size_class: tsc,
            });
        }
        class_t.push(ClassT {
            name: cname.clone(),
            t: tmap,
        });
        class_kinds.push(cl.kind.clone());
        let sum = |f: fn(&PairStats) -> u64| pairs.iter().map(|p| f(&p.1)).sum::<u64>();
        let vsum = |f: fn(&PairStats) -> &Vec<u64>| -> Vec<u64> {
            let mut v = vec![0u64; analysis::SIZE_CLASSES];
            for p in &pairs {
                for (a, b) in v.iter_mut().zip(f(&p.1)) {
                    *a += b;
                }
            }
            v
        };
        classes.push(ClassReport {
            name: cname.clone(),
            kind: cl.kind.clone(),
            bytes_new_by_size_class: vsum(|p| &p.bytes_new_by_size_class),
            bytes_modified_by_size_class: vsum(|p| &p.bytes_modified_by_size_class),
            bytes_new_content_by_size_class: vsum(|p| &p.bytes_new_content_by_size_class),
            pairs: pairs.len(),
            files_new_sum: sum(|p| p.files_new),
            bytes_new_sum: sum(|p| p.bytes_new),
            files_unchanged_sum: sum(|p| p.files_unchanged),
            files_modified_sum: sum(|p| p.files_modified),
            files_moved_sum: sum(|p| p.files_moved),
            files_new_content_sum: sum(|p| p.files_new_content),
            files_deleted_sum: sum(|p| p.files_deleted),
            empty_files_new_sum: sum(|p| p.empty_files_new),
            t_file_level_path_mean_percent: pairs
                .iter()
                .map(|p| p.1.t_file_level_path_percent)
                .sum::<f64>()
                / n,
            t_file_level_content_mean_percent: pairs
                .iter()
                .map(|p| p.1.t_file_level_content_percent)
                .sum::<f64>()
                / n,
            candidates: cc,
            pairs_detail: pairs
                .into_iter()
                .map(|(label, stats)| PairDetail { label, stats })
                .collect(),
        });
    }
    // 100 GB model with measured mean chunk sizes of split files.
    let mean_split: BTreeMap<String, f64> = split_acc
        .iter()
        .filter(|(_, v)| v.0 > 0)
        .map(|(k, v)| (k.clone(), v.1 as f64 / v.0 as f64))
        .collect();
    let model_rows = if no_model {
        None
    } else {
        Some(model::rows(
            &cands,
            &mean_split,
            model::MODEL_FILES,
            model::MODEL_BYTES,
        ))
    };
    let selectable_ids: BTreeSet<&str> = cands
        .iter()
        .filter(|c| c.selectable)
        .map(|c| c.id.as_str())
        .collect();
    let dropped: BTreeSet<String> = model_rows
        .as_ref()
        .map(|r| {
            r.iter()
                .filter(|x| !x.violates.is_empty() && selectable_ids.contains(x.id.as_str()))
                .map(|x| x.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let selection = selection_for(
        &cands,
        &class_t,
        &class_kinds,
        &dropped,
        cfg.candidates.is_some(),
        no_model,
        metadata.complete(),
    );
    let diagnostics = class_kinds
        .iter()
        .filter(|k| !analysis::DECIDING_KINDS.contains(&k.as_str()))
        .count();
    let mut notes = vec![
        "Chunk identity is BLAKE3-256 of the chunk bytes; FastCDC v2020 uses the fastcdc crate default normalisation (level 1).".to_string(),
        "Empty files have zero chunks; files of at most max bytes are exactly one chunk.".to_string(),
        "T per pair = bytes of distinct new-version chunks absent from the old version; class value = mean over pairs; T-bar = mean over classes with equal weight.".to_string(),
        "fixed-64m is the baseline (selectable=false). t_file_level_* are file-granular baselines (predecessor-like path+content, and content only).".to_string(),
    ];
    if diagnostics > 0 {
        notes.push(format!(
            "{diagnostics} diagnostic class(es) are reported but excluded from the deciding mean."
        ));
    }
    if !selection.decidable {
        notes.push(format!("NOT DECIDABLE: {}. `selection.chosen` is empty; `diagnostic_choice` is for diagnosis only.", selection.not_decidable_because.join("; ")));
    }
    let rep = AnalyzeReport {
        tool: "chunkbench analyze",
        model_id: schema_adapter::MODEL_ID,
        size_model_only: true,
        machine: sysinfo::collect(metadata),
        parameters: serde_json::json!({
            "candidates": cands,
            "load_limit_bytes": chunking::LOAD_LIMIT,
            "model": {"files": model::MODEL_FILES, "bytes": model::MODEL_BYTES, "limits": {"chunks": 4_000_000u64, "manifest_bytes": 512u64 * 1024 * 1024}},
            "selection_step_points": select::STEP_PP,
            "deviation_points": select::DEVIATION_PP,
        }),
        classes,
        selection,
        model_100gb: model_rows,
        notes,
    };
    let json = serde_json::to_string_pretty(&rep).map_err(util::io_err)?;
    std::fs::write(&out, &json)?;
    println!("{json}");
    Ok(())
}

fn selection_for(
    cands: &[Candidate],
    class_t: &[ClassT],
    class_kinds: &[String],
    dropped: &BTreeSet<String>,
    restricted: bool,
    no_model: bool,
    metadata_complete: bool,
) -> select::Selection {
    let sel_cands: Vec<&Candidate> = cands.iter().filter(|c| c.selectable).collect();
    let desc = |fam: Family| {
        let mut v: Vec<&Candidate> = sel_cands
            .iter()
            .copied()
            .filter(|c| c.family == fam)
            .collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.nominal));
        v.into_iter().map(|c| c.id.as_str()).collect::<Vec<_>>()
    };
    let (cdc_desc, fixed_desc) = (desc(Family::Cdc), desc(Family::Fixed));
    let same = |id: &str| id.strip_prefix("cdc-").map(|s| format!("fixed-{s}"));
    let mut pre = Vec::new();
    if !metadata_complete {
        pre.push("run metadata incomplete".into());
    }
    if restricted {
        pre.push("candidate grid restricted by the configuration".to_string());
    }
    if no_model {
        pre.push("step 1 (limits in the 100 GB model) was not executed".to_string());
    }
    // Only the three deciding kinds enter the mean; diagnostics are reported but excluded. Exactly one class per kind.
    let mut deciding: Vec<ClassT> = Vec::new();
    for kind in analysis::DECIDING_KINDS {
        let idx: Vec<usize> = (0..class_t.len())
            .filter(|i| class_kinds[*i] == kind)
            .collect();
        match idx.len() {
            0 => pre.push(format!("required class kind {kind} missing")),
            1 => deciding.push(class_t[idx[0]].clone()),
            n => {
                pre.push(format!("class kind {kind} given {n} times"));
                deciding.push(class_t[idx[0]].clone());
            }
        }
    }
    // A run without any deciding class (diagnostic corpus only) is evaluated on its diagnostic classes, never as a decision.
    let basis: Vec<ClassT> = if deciding.is_empty() {
        class_t.to_vec()
    } else {
        deciding
    };
    select::select(&basis, &cdc_desc, &fixed_desc, &same, dropped, &pre)
}

/// Local measurement quality also requires a busy limit of at most 10 percent.
pub const MAX_DECISION_BUSY_PERCENT: f64 = 10.0;
pub fn decision_grade(max_busy_percent: Option<f64>) -> bool {
    max_busy_percent
        .is_some_and(|m| m.is_finite() && (0.0..=MAX_DECISION_BUSY_PERCENT).contains(&m))
}

#[derive(Serialize)]
struct HashReport {
    tool: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    label: String,
    machine: sysinfo::Machine,
    /// /proc/loadavg before and after the timed runs (quiet-window evidence; the host-wide value, also inside containers).
    loadavg_start: String,
    loadavg_end: String,
    /// Result may be used for the decision: no override, a busy limit was enforced and held.
    decision_grade: bool,
    parameters: serde_json::Value,
    tree_files: usize,
    tree_bytes: u64,
    conditions: Vec<hashbench::Condition>,
    /// On a quiet-gate timeout, retain the failed condition and its complete wait evidence.
    quiet_abort: Option<hashbench::QuietAbort>,
    micro: Option<Vec<hashbench::Micro>>,
    notes: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn hash(
    tree: PathBuf,
    out: PathBuf,
    algos: String,
    workers: String,
    stages: String,
    cache: String,
    reps: usize,
    readers: usize,
    chunking_id: String,
    max_busy_percent: Option<f64>,
    no_file_hash: bool,
    no_micro: bool,
    label: String,
    metadata: &sysinfo::RunMetadata,
) -> io::Result<()> {
    if readers != 1 {
        return Err(io::Error::other(
            "hash measurements require exactly one reader",
        ));
    }
    if max_busy_percent.is_some_and(|m| !m.is_finite() || !(0.0..=100.0).contains(&m)) {
        return Err(io::Error::other("busy limit must be between 0 and 100"));
    }
    if reps == 0 {
        return Err(io::Error::other("--reps must be at least 1"));
    }
    let loadavg_start = sysinfo::loadavg();
    let t = walk(&tree)?;
    let algos: Vec<Algo> = algos
        .split(',')
        .map(|a| match a.trim() {
            "sha256" => Ok(Algo::Sha256),
            "blake3" => Ok(Algo::Blake3),
            _ => Err(io::Error::other("unknown algorithm")),
        })
        .collect::<io::Result<_>>()?;
    let mut workers: Vec<usize> = workers
        .split(',')
        .map(|w| w.trim().parse().map_err(util::io_err))
        .collect::<io::Result<_>>()?;
    if workers.contains(&0) {
        return Err(io::Error::other("workers must be positive"));
    }
    // On machines with fewer cores than a requested worker count use the core count (benchmark protocol).
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    for w in workers.iter_mut() {
        *w = (*w).min(cores);
    }
    workers.dedup();
    let stages: Vec<Stage> = stages
        .split(',')
        .map(|s| match s.trim() {
            "build" => Ok(Stage::Build),
            "verify" => Ok(Stage::Verify),
            "chunk-only" => Ok(Stage::ChunkOnly),
            _ => Err(io::Error::other("unknown stage")),
        })
        .collect::<io::Result<_>>()?;
    let caches: Vec<&'static str> = cache
        .split(',')
        .map(|s| match s.trim() {
            "warm" => Ok("warm"),
            "cold" => Ok("cold"),
            _ => Err(io::Error::other("unknown cache mode")),
        })
        .collect::<io::Result<_>>()?;
    let cand = chunking::grid()
        .into_iter()
        .find(|c| c.id == chunking_id)
        .ok_or_else(|| io::Error::other("unknown chunking candidate"))?;
    let cfg = BenchCfg {
        algos: algos.clone(),
        workers: workers.clone(),
        readers,
        stages,
        caches,
        reps,
        cand: cand.clone(),
        file_hash: !no_file_hash,
        max_busy_percent,
    };
    progress!("hash bench: {} files", t.files.len());
    let outcome = hashbench::bench(&t, &cfg)?;
    let aborted = outcome.quiet_abort.is_some();
    let micro = if no_micro || aborted {
        None
    } else {
        Some(hashbench::micro(&algos, &workers))
    };
    let conditions_valid = rep_conditions_valid(&outcome.conditions);
    let rep = HashReport {
        tool: "chunkbench hash",
        label,
        machine: sysinfo::collect(metadata),
        loadavg_start,
        loadavg_end: sysinfo::loadavg(),
        decision_grade: !aborted
            && metadata.complete()
            && !no_file_hash
            && conditions_valid
            && decision_grade(max_busy_percent),
        parameters: serde_json::json!({
            "chunking": cand,
            "file_hash_of_split_files": !no_file_hash,
            "readers": readers,
            "max_busy_percent": max_busy_percent,
            "quiet_gate": {
                "timeout_seconds": crate::quiet::TIMEOUT.as_secs(),
                "sample_interval_seconds": crate::quiet::INTERVAL.as_secs(),
                "consecutive_samples": crate::quiet::REQUIRED_SAMPLES,
                "accounting": "host /proc/stat first 8 counters; idle includes iowait; subtract /proc/self/stat utime+stime+cutime+cstime (all own threads and reaped children); no subprocesses in pipeline; kernel workers remain counted",
            },
            "workers": workers,
            "reps": reps,
            "pipeline": "reader/chunker thread(s) -> bounded queue of batches (<=512 KiB or 256 chunks) -> fixed worker pool; chunk data is copied once",
            "verify_path": "reads with the boundaries recorded by the build run, no chunking, compares hashes",
        }),
        tree_files: t.files.len(),
        tree_bytes: t.files.iter().map(|f| f.size).sum(),
        conditions: outcome.conditions,
        quiet_abort: outcome.quiet_abort,
        micro,
        notes: {
            let mut n = vec![
                "SHA-256 is the default product hash. Throughput runs do not reopen that choice."
                    .to_string(),
            ];
            if !metadata.complete() {
                n.push("NOT DECISION GRADE: incomplete run metadata.".into());
            }
            if no_file_hash {
                n.push("NOT DECISION GRADE: split-file hashes disabled.".into());
            }
            if !conditions_valid {
                n.push("NOT DECISION GRADE: cache state or hashes not verified.".into());
            }
            if !max_busy_percent.is_some_and(|m| m <= MAX_DECISION_BUSY_PERCENT) {
                n.push("NOT DECISION GRADE: no --max-busy-percent limit of at most 10 was enforced between conditions.".into());
            }
            if aborted {
                n.push("NOT DECISION GRADE: quiet gate timed out; partial results and failed-condition wait evidence retained in quiet_abort.".into());
            }
            n.push("Before each repetition: join all reader/worker threads, release previous run allocations, prepare cache, then require three consecutive 1 s external CPU samples <= limit within 120 s. Recheck cache residency after waiting. busy_percent_before_each_rep is the final external CPU sample; quiet_before_each_rep records actual wait and every sample (system, own, external). Failed probes reset the quiet streak.".into());
            n
        },
    };
    finish_hash(&rep, &out)
}

fn finish_hash(rep: &HashReport, out: &std::path::Path) -> io::Result<()> {
    let json = serde_json::to_string_pretty(rep).map_err(util::io_err)?;
    std::fs::write(out, &json)?;
    println!("{json}");
    if let Some(abort) = &rep.quiet_abort {
        let wait = abort
            .quiet_before_each_rep
            .last()
            .expect("abort has wait evidence");
        let last = wait
            .samples
            .last()
            .and_then(|s| s.busy)
            .map(|b| format!("{:.1} %", b.external_percent))
            .unwrap_or_else(|| "unavailable".into());
        return Err(io::Error::other(format!(
            "machine not quiet: CPU busy {last} before a condition (limit {} %, waited {:.3} s; need 3 consecutive quiet samples within 120 s)",
            abort.limit_percent, wait.wait_seconds,
        )));
    }
    validate_conditions(&rep.conditions)
}

fn rep_conditions_valid(conditions: &[hashbench::Condition]) -> bool {
    !conditions.is_empty()
        && conditions
            .iter()
            .all(|c| c.mismatches == 0 && c.cache_state == "valid")
}
fn validate_conditions(conditions: &[hashbench::Condition]) -> io::Result<()> {
    if conditions
        .iter()
        .any(|c| c.mismatches > 0 || c.cache_state == "invalid")
    {
        return Err(io::Error::other(
            "invalid hash run: hash mismatch or invalid cache state",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn class(name: &str, value: f64) -> ClassT {
        ClassT {
            name: name.into(),
            t: chunking::grid()
                .into_iter()
                .map(|c| (c.id, value))
                .collect(),
        }
    }
    #[test]
    fn deciding_kinds_are_unique_and_diagnostics_never_enter_the_mean() {
        let cands = chunking::grid();
        let rows = vec![
            class("c1", 12.5),
            class("c2", 100.0),
            class("c3", 12.5),
            class("diag", 99.0),
        ];
        let kinds: Vec<String> = [
            "many-small-files",
            "compressed-archive",
            "pak-files",
            "diagnostic",
        ]
        .map(String::from)
        .to_vec();
        let run = |rows: &[ClassT], kinds: &[String]| {
            selection_for(&cands, rows, kinds, &BTreeSet::new(), false, false, true)
        };
        let s = run(&rows, &kinds);
        assert!(s.decidable && s.chosen.is_some());
        assert_eq!(s.classes_used, vec!["c1", "c2", "c3"]);
        assert!((s.t_bar["fixed-1m"] - (12.5 + 100.0 + 12.5) / 3.0).abs() < 1e-9);
        let duplicate = run(&rows[..3], &vec!["many-small-files".into(); 3]);
        assert!(!duplicate.decidable && duplicate.chosen.is_none());
        assert!(duplicate
            .not_decidable_because
            .iter()
            .any(|r| r.contains("given 3 times")));
        let missing = run(&rows[..2], &kinds[..2]);
        assert!(!missing.decidable && missing.chosen.is_none());
        assert!(missing
            .not_decidable_because
            .iter()
            .any(|r| r.contains("pak-files missing")));
        let named_like_kind = run(&[class("pak-files", 1.0)], &["diagnostic".into()]);
        assert!(!named_like_kind.decidable && named_like_kind.chosen.is_none());
        let unknown = selection_for(&cands, &rows, &kinds, &BTreeSet::new(), false, false, false);
        assert!(!unknown.decidable && unknown.chosen.is_none());
        for (restricted, skipped) in [(true, false), (false, true)] {
            let s = selection_for(
                &cands,
                &rows,
                &kinds,
                &BTreeSet::new(),
                restricted,
                skipped,
                true,
            );
            assert!(!s.decidable && s.chosen.is_none());
        }
    }
    fn condition() -> hashbench::Condition {
        hashbench::Condition {
            algorithm: Algo::Sha256,
            workers: 1,
            readers: 1,
            stage: Stage::Verify,
            cache: "warm",
            seconds_each_rep: vec![1.0],
            seconds_median: 1.0,
            mib_per_s_median: 1.0,
            bytes: 1,
            files: 1,
            chunks: 1,
            resident_fraction_before_each_rep: vec![1.0],
            busy_percent_before_each_rep: vec![],
            quiet_before_each_rep: vec![],
            cache_state_valid: true,
            cache_state: "valid",
            mismatches: 0,
        }
    }
    #[test]
    fn invalid_measurements_fail_after_evidence_is_written() {
        let path =
            std::env::temp_dir().join(format!("chunkbench-evidence-{}.json", std::process::id()));
        for cache_failure in [false, true] {
            let mut c = condition();
            if cache_failure {
                c.cache_state = "invalid";
                c.cache_state_valid = false;
            } else {
                c.mismatches = 1;
            }
            let report = HashReport {
                tool: "chunkbench hash",
                label: String::new(),
                machine: sysinfo::collect(&sysinfo::RunMetadata::default()),
                loadavg_start: String::new(),
                loadavg_end: String::new(),
                decision_grade: false,
                parameters: serde_json::json!({}),
                tree_files: 1,
                tree_bytes: 1,
                conditions: vec![c],
                quiet_abort: None,
                micro: None,
                notes: vec![],
            };
            assert!(finish_hash(&report, &path).is_err());
            let evidence: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(evidence["decision_grade"], false);
            assert_eq!(
                evidence["conditions"][0]["cache_state"],
                if cache_failure { "invalid" } else { "valid" }
            );
            assert_eq!(
                evidence["conditions"][0]["mismatches"],
                if cache_failure { 0 } else { 1 }
            );
        }
        let _ = std::fs::remove_file(path);
        assert!(validate_conditions(&[condition()]).is_ok());
        let mut unknown = condition();
        unknown.cache_state = "unknown";
        assert!(!rep_conditions_valid(&[unknown]));
    }
    #[test]
    fn measurement_quality_rejects_missing_or_invalid_busy_limits() {
        for limit in [None, Some(-1.0), Some(f64::NAN), Some(10.5)] {
            assert!(!decision_grade(limit));
        }
        assert!(decision_grade(Some(10.0)));
    }
}

#[cfg(test)]
mod quiet_report_tests {
    use super::*;
    #[test]
    fn quiet_timeout_retains_failed_condition_evidence_and_fails() {
        let path = std::env::temp_dir().join(format!(
            "chunkbench-quiet-abort-{}.json",
            std::process::id()
        ));
        let report = HashReport {
            tool: "chunkbench hash",
            label: String::new(),
            machine: sysinfo::collect(&sysinfo::RunMetadata::default()),
            loadavg_start: String::new(),
            loadavg_end: String::new(),
            decision_grade: false,
            parameters: serde_json::json!({}),
            tree_files: 1,
            tree_bytes: 1,
            conditions: vec![],
            micro: None,
            notes: vec![],
            quiet_abort: Some(hashbench::QuietAbort {
                algorithm: Algo::Sha256,
                workers: 1,
                readers: 1,
                stage: Stage::Build,
                cache: "warm",
                repetition: 1,
                limit_percent: 10.0,
                quiet_before_each_rep: vec![crate::quiet::Wait {
                    wait_seconds: 120.0,
                    ready: false,
                    samples: vec![crate::quiet::Sample {
                        elapsed_seconds: 120.0,
                        busy: None,
                    }],
                }],
            }),
        };
        assert!(finish_hash(&report, &path).is_err());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved["quiet_abort"]["quiet_before_each_rep"][0]["ready"],
            false
        );
        assert_eq!(
            saved["quiet_abort"]["quiet_before_each_rep"][0]["wait_seconds"],
            120.0
        );
        std::fs::remove_file(path).unwrap();
    }
}
