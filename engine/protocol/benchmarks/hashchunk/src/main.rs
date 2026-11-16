// SPDX-License-Identifier: 0BSD
//! chunkbench: chunking and hashing benchmark for content-addressed game manifests.
//!
//! Subcommands: `gen` (synthetic corpus), `analyze` (chunk analysis + selection rule), `hash` (hash pipeline),
//! `digest` (reproducible tree digest), `sysinfo`, `model` (100 GB model table without data).

mod analysis;
mod cache;
mod chunking;
mod corpus_model;
mod gen;
mod hashbench;
mod model;
mod quiet;
mod schema_adapter;
mod select;
mod sysinfo;
mod treewalk;
#[macro_use]
mod util;
mod report;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "chunkbench",
    version,
    about = "Chunking and hashing benchmark (0BSD)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// JSON with source_revision, rustc and dependency versions; absent fields remain unknown.
    #[arg(long, global = true)]
    run_metadata: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate the synthetic many-small-files corpus (two versions) below OUT/v1 and OUT/v2.
    Gen {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 20261101)]
        seed: u64,
        #[arg(long, default_value_t = 100_000)]
        files: usize,
        #[arg(long, default_value_t = 50)]
        symlinks: usize,
        /// Leave out the boundary group (diagnostic corpus: isolates its influence on the result).
        #[arg(long)]
        no_boundary: bool,
    },
    /// Derive a corpus without the boundary group from an existing generated corpus (same files otherwise).
    GenFilter {
        #[arg(long)]
        src: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 20261101)]
        seed: u64,
        #[arg(long, default_value_t = 100_000)]
        files: usize,
    },
    /// Chunk analysis over the classes/pairs of a JSON config; runs the selection rule of the benchmark protocol.
    Analyze {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Chunk identity is BLAKE3; this only limits rayon threads.
        #[arg(long)]
        threads: Option<usize>,
        /// Skip the 100 GB model (slow, ~1 min).
        #[arg(long)]
        no_model: bool,
    },
    /// Hash pipeline benchmark on one tree.
    Hash {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "sha256,blake3")]
        algos: String,
        #[arg(long, default_value = "1,4")]
        workers: String,
        #[arg(long, default_value = "build,verify,chunk-only")]
        stages: String,
        #[arg(long, default_value = "warm")]
        cache: String,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
        reps: u64,
        #[arg(long, default_value_t = 1)]
        readers: usize,
        /// Chunking candidate id of the grid (default: the setting selected by the chunk analysis, FastCDC avg 256 KiB,
        /// min 64 KiB, max 1 MiB). The parameters actually used are written to the result (`parameters.chunking`).
        #[arg(long, default_value = "cdc-256k")]
        chunking: String,
        /// Before each repetition, wait up to 120 s for three consecutive 1 s CPU samples at or below this
        /// percentage (host-wide, excluding this process and reaped children). Quiet-window runs use 10.
        #[arg(long)]
        max_busy_percent: Option<f64>,
        #[arg(long)]
        no_file_hash: bool,
        #[arg(long)]
        no_micro: bool,
        /// Free-text label of the run.
        #[arg(long, default_value = "")]
        label: String,
    },
    /// Reproducible SHA-256 digest of a tree (paths, sizes, modes, content hashes, symlink targets).
    Digest { dir: PathBuf },
    /// Print machine description.
    Sysinfo,
    /// Print the 100 GB / 1 million file model table.
    Model {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = sysinfo::RunMetadata::load(cli.run_metadata.as_deref())
        .and_then(|metadata| report::run(cli.cmd_into(), &metadata));
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

impl Cli {
    fn cmd_into(self) -> report::Command {
        match self.cmd {
            Cmd::Gen {
                out,
                seed,
                files,
                symlinks,
                no_boundary,
            } => report::Command::Gen {
                out,
                seed,
                files,
                symlinks,
                boundary: !no_boundary,
            },
            Cmd::Analyze {
                config,
                out,
                threads,
                no_model,
            } => report::Command::Analyze {
                config,
                out,
                threads,
                no_model,
            },
            Cmd::Hash {
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
            } => report::Command::Hash {
                tree,
                out,
                algos,
                workers,
                stages,
                cache,
                reps: reps as usize,
                readers,
                chunking,
                max_busy_percent,
                no_file_hash,
                no_micro,
                label,
            },
            Cmd::GenFilter {
                src,
                out,
                seed,
                files,
            } => report::Command::GenFilter {
                src,
                out,
                seed,
                files,
            },
            Cmd::Digest { dir } => report::Command::Digest { dir },
            Cmd::Sysinfo => report::Command::Sysinfo,
            Cmd::Model { out } => report::Command::Model { out },
        }
    }
}
