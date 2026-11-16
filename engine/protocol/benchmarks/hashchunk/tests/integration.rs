// SPDX-License-Identifier: 0BSD
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    // CARGO_BIN_EXE_ is Cargo's prefix for executable paths in integration tests.
    Command::new(env!(concat!("CARGO_BIN_EXE_", "chunkbench")))
}

fn tmp(name: &str) -> PathBuf {
    let base = std::env::temp_dir();
    let d = base.join(format!("chunkbench-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn gen_small(dir: &Path, seed: &str, threads: &str) -> serde_json::Value {
    let o = bin()
        .env("RAYON_NUM_THREADS", threads)
        .args(["gen", "--out"])
        .arg(dir)
        .args(["--seed", seed, "--files", "300", "--symlinks", "10"])
        .output()
        .unwrap();
    serde_json::from_str(&ok(&o)).unwrap()
}

#[test]
fn generator_is_deterministic_across_thread_counts() {
    let (a, b) = (tmp("gen-a"), tmp("gen-b"));
    let ra = gen_small(&a, "7", "1");
    let rb = gen_small(&b, "7", "5");
    assert_eq!(ra["version_digests"], rb["version_digests"]);
    assert_eq!(ra["counts"], rb["counts"]);
    let c = tmp("gen-c");
    let rc = gen_small(&c, "8", "2");
    assert_ne!(ra["version_digests"], rc["version_digests"]);
    // the digest command reproduces the recorded digest
    let d = ok(&bin().arg("digest").arg(a.join("v1")).output().unwrap());
    assert_eq!(d.trim(), ra["version_digests"][0].as_str().unwrap());
    for p in [a, b, c] {
        std::fs::remove_dir_all(p).ok();
    }
}

#[test]
fn analysis_matches_generator_change_pattern() {
    let d = tmp("ana");
    let g = gen_small(&d, "11", "3");
    let cfg = d.join("cfg.json");
    std::fs::write(&cfg, serde_json::json!({"classes":[{"name":"small","pairs":[{"label":"p","old":d.join("v1"),"new":d.join("v2")}]}]}).to_string()).unwrap();
    let out = d.join("res.json");
    let o = bin()
        .args(["analyze", "--no-model", "--config"])
        .arg(&cfg)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let r: serde_json::Value = serde_json::from_str(&ok(&o)).unwrap();
    let c = &g["counts"];
    let cl = &r["classes"][0];
    assert_eq!(cl["files_modified_sum"], c["modified"]);
    assert_eq!(cl["files_moved_sum"], c["moved"]);
    assert_eq!(cl["files_new_content_sum"], c["new"]);
    assert_eq!(cl["files_unchanged_sum"], c["unchanged"]);
    assert_eq!(
        cl["files_deleted_sum"].as_u64().unwrap(),
        c["deleted"].as_u64().unwrap() + c["moved"].as_u64().unwrap()
    );
    for cand in cl["candidates"].as_array().unwrap() {
        let t = cand["t_mean_percent"].as_f64().unwrap();
        assert!((0.0..=100.0).contains(&t), "{cand}");
    }
    // moved files cost nothing at chunk level: file-level (path) baseline pays for them, content baseline does not
    assert!(
        cl["t_file_level_path_mean_percent"].as_f64().unwrap()
            >= cl["t_file_level_content_mean_percent"].as_f64().unwrap()
    );
    std::fs::remove_dir_all(d).ok();
}

fn corpus_fill(b: &mut [u8], seed: u64) {
    let mut x = seed;
    for c in b.chunks_mut(8) {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let v = (x ^ (x >> 29)).to_le_bytes();
        c.copy_from_slice(&v[..c.len()]);
    }
}

#[test]
fn hash_pipeline_runs_and_verifies() {
    let d = tmp("hash");
    gen_small(&d, "13", "2");
    let out = d.join("h.json");
    let o = bin()
        .args([
            "hash",
            "--reps",
            "1",
            "--no-micro",
            "--workers",
            "1,4",
            "--stages",
            "build,verify,chunk-only",
            "--tree",
        ])
        .arg(d.join("v2"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let r: serde_json::Value = serde_json::from_str(&ok(&o)).unwrap();
    let conds = r["conditions"].as_array().unwrap();
    assert!(!conds.is_empty());
    let mut chunk_counts = std::collections::BTreeSet::new();
    for c in conds {
        assert_eq!(c["mismatches"], 0, "{c}");
        assert!(c["mib_per_s_median"].as_f64().unwrap() > 0.0);
        if c["stage"] != "chunk-only" {
            chunk_counts.insert(c["chunks"].as_u64().unwrap());
        }
    }
    assert_eq!(
        chunk_counts.len(),
        1,
        "build and verify must see the same chunk boundaries"
    );
    assert_eq!(r["machine"]["tool_version"], env!("CARGO_PKG_VERSION"));
    // the selected setting is the default and the parameters actually used are in the result
    let ch = &r["parameters"]["chunking"];
    assert_eq!(
        (
            ch["id"].as_str().unwrap(),
            ch["nominal"].as_u64().unwrap(),
            ch["min"].as_u64().unwrap(),
            ch["max"].as_u64().unwrap()
        ),
        ("cdc-256k", 262_144, 65_536, 1_048_576)
    );
    assert_eq!(
        r["decision_grade"], false,
        "no busy limit enforced: not decision grade"
    );
    std::fs::remove_dir_all(d).ok();
}

/// Independent check of the weighting: pair mean within a class (not byte-weighted), class mean across classes.
#[test]
fn weighting_is_pair_mean_then_class_mean() {
    let d = tmp("weights");
    let mut base = vec![0u8; 8 << 20];
    corpus_fill(&mut base, 5);
    let mut small = vec![0u8; 2 << 20];
    corpus_fill(&mut small, 6);
    let put = |dir: &str, data: &[u8]| {
        std::fs::create_dir_all(d.join(dir)).unwrap();
        std::fs::write(d.join(dir).join("f.bin"), data).unwrap();
    };
    put("a1", &base);
    let mut a2 = base.clone();
    a2[100] ^= 1;
    put("a2", &a2); // pair A: 1 of 8 blocks (fixed-1m) changes -> T = 12.5 %
    put("b1", &small);
    let mut b2 = small.clone();
    for x in b2.iter_mut() {
        *x ^= 0x55; // pair B: everything changes -> 100 %
    }
    put("b2", &b2);
    let analyze = |classes: serde_json::Value| -> serde_json::Value {
        let cfg = d.join("c.json");
        std::fs::write(&cfg, serde_json::json!({ "classes": classes }).to_string()).unwrap();
        let o = bin()
            .args(["analyze", "--no-model", "--config"])
            .arg(&cfg)
            .arg("--out")
            .arg(d.join("o.json"))
            .output()
            .unwrap();
        serde_json::from_str(&ok(&o)).unwrap()
    };
    let t = |r: &serde_json::Value, c: usize| {
        r["classes"][c]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == "fixed-1m")
            .unwrap()["t_mean_percent"]
            .as_f64()
            .unwrap()
    };
    let pa = |p: &str, q: &str| serde_json::json!({"label":"p","old":d.join(p),"new":d.join(q)});
    let class_ab = analyze(
        serde_json::json!([{"name":"x","kind":"many-small-files","pairs":[pa("a1","a2"), pa("b1","b2")]}]),
    );
    // pair means: (12.5 + 100) / 2, NOT byte weighted ((1 + 2) / (8 + 2) = 30 %)
    assert!(
        (t(&class_ab, 0) - 56.25).abs() < 0.01,
        "{}",
        t(&class_ab, 0)
    );
    // two classes with equal weight although one has two pairs
    let two = analyze(
        serde_json::json!([{"name":"x","kind":"many-small-files","pairs":[pa("a1","a2"), pa("b1","b2")]}, {"name":"y","kind":"compressed-archive","pairs":[pa("b1","b2")]}]),
    );
    let tbar = two["selection"]["t_bar"]["fixed-1m"].as_f64().unwrap();
    assert!((tbar - (56.25 + 100.0) / 2.0).abs() < 0.01, "{tbar}");
    std::fs::remove_dir_all(d).ok();
}

/// Fast mutation counter-check for main's error-to-exit mapping (also used after a quiet timeout).
#[test]
fn processing_error_exits_nonzero() {
    let d = tmp("processing-error-exit");
    let o = bin()
        .args(["hash", "--no-micro", "--tree"])
        .arg(d.join("missing"))
        .arg("--out")
        .arg(d.join("o.json"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "processing error must exit 1");
    assert!(String::from_utf8_lossy(&o.stderr).contains("error:"));
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn metadata_is_explicit_and_malformed_input_fails() {
    let d = tmp("metadata");
    let unknown: serde_json::Value =
        serde_json::from_str(&ok(&bin().arg("sysinfo").output().unwrap())).unwrap();
    assert!(unknown["source_revision"].is_null() && unknown["rustc"].is_null());
    let path = d.join("metadata.json");
    std::fs::write(
        &path,
        r#"{"source_revision":"invalid","rustc":"not a compiler"}"#,
    )
    .unwrap();
    let bad = bin()
        .arg("--run-metadata")
        .arg(&path)
        .arg("sysinfo")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    std::fs::write(&path, "{}").unwrap();
    let partial: serde_json::Value = serde_json::from_str(&ok(&bin()
        .arg("--run-metadata")
        .arg(&path)
        .arg("sysinfo")
        .output()
        .unwrap()))
    .unwrap();
    assert!(partial["rustc"].is_null());
    std::fs::write(&path, "{").unwrap();
    assert_eq!(
        bin()
            .arg("--run-metadata")
            .arg(&path)
            .arg("sysinfo")
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn relative_config_paths_and_invalid_cli_inputs() {
    let d = tmp("relative-config");
    for v in ["old", "new"] {
        std::fs::create_dir(d.join(v)).unwrap();
        std::fs::write(d.join(v).join("f"), v).unwrap();
    }
    let config = d.join("config.json");
    std::fs::write(
        &config,
        r#"{"classes":[{"name":"sample","pairs":[{"label":"pair","old":"old","new":"new"}]}]}"#,
    )
    .unwrap();
    let report: serde_json::Value = serde_json::from_str(&ok(&bin()
        .args(["analyze", "--no-model", "--config"])
        .arg(&config)
        .arg("--out")
        .arg(d.join("report.json"))
        .output()
        .unwrap()))
    .unwrap();
    assert_eq!(report["model_id"], "protocol-manifest-v3");
    assert_eq!(report["size_model_only"], true);
    assert_eq!(report["selection"]["decidable"], false);
    assert!(report["selection"]["chosen"].is_null());
    for args in [
        ["--workers", "0"],
        ["--readers", "2"],
        ["--reps", "0"],
        ["--max-busy-percent", "NaN"],
    ] {
        assert!(!bin()
            .args(["hash", "--no-micro", "--tree"])
            .arg(d.join("new"))
            .arg("--out")
            .arg(d.join("hash.json"))
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    assert!(!bin()
        .arg("--unknown-option")
        .output()
        .unwrap()
        .status
        .success());
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn controlled_pair_shows_cdc_advantage_on_insert_and_none_on_overwrite() {
    let d = tmp("ctrl");
    let mut base = vec![0u8; 16 << 20];
    corpus_fill(&mut base, 99);
    let write = |dir: &str, data: &[u8]| {
        std::fs::create_dir_all(d.join(dir)).unwrap();
        std::fs::write(d.join(dir).join("pak.bin"), data).unwrap();
    };
    write("v1", &base);
    let mut ins = base.clone();
    ins.splice(6 << 20..6 << 20, vec![0xAB; 1000]); // 1000 bytes inserted in the middle: everything after shifts
    write("v2-insert", &ins);
    let mut ovr = base.clone();
    for b in &mut ovr[6 << 20..(6 << 20) + 1000] {
        *b = 0xCD; // in-place overwrite of 1000 bytes
    }
    write("v2-overwrite", &ovr);
    let run = |new: &str| -> std::collections::BTreeMap<String, f64> {
        let cfg = d.join(format!("{new}.json"));
        std::fs::write(&cfg, serde_json::json!({"classes":[{"name":"c","pairs":[{"label":"p","old":d.join("v1"),"new":d.join(new)}]}]}).to_string()).unwrap();
        let o = bin()
            .args(["analyze", "--no-model", "--config"])
            .arg(&cfg)
            .arg("--out")
            .arg(d.join("o.json"))
            .output()
            .unwrap();
        let r: serde_json::Value = serde_json::from_str(&ok(&o)).unwrap();
        r["classes"][0]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["id"].as_str().unwrap().to_string(),
                    c["t_mean_percent"].as_f64().unwrap(),
                )
            })
            .collect()
    };
    let ins = run("v2-insert");
    let ovr = run("v2-overwrite");
    assert!(
        ins["fixed-1m"] > 40.0,
        "fixed blocks lose everything after the insert: {}",
        ins["fixed-1m"]
    );
    assert!(
        ins["cdc-256k"] < 10.0 && ins["cdc-1m"] < 20.0,
        "CDC resynchronises: {:?}",
        ins
    );
    assert!(
        ovr["fixed-1m"] < 8.0 && ovr["fixed-1m"] > 5.0,
        "one 1 MiB block of 16: {}",
        ovr["fixed-1m"]
    );
    assert!(
        ovr["fixed-64m"] > 99.0,
        "a 64 MiB block is one chunk: the whole 16 MiB file again ({})",
        ovr["fixed-64m"]
    );
    std::fs::remove_dir_all(d).ok();
}
