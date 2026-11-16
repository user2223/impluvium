// SPDX-License-Identifier: 0BSD
//! The 100 GB / 1 million file model game: chunk counts and manifest sizes per candidate, checked against the
//! format size limits (chunks <= 4 000 000, manifest <= 512 MiB). The manifest size is the real protobuf
//! encoded length of a synthetic manifest (all chunk ids distinct, i.e. no dedup: conservative).
use crate::chunking::{Candidate, Family};
use crate::schema_adapter::{build, ChunkModel, ModelSpec};
use buffa::Message;
use serde::Serialize;
use std::collections::BTreeMap;

pub const MODEL_FILES: usize = 1_000_000;
pub const MODEL_BYTES: u64 = 100_000_000_000;

#[derive(Serialize, Clone)]
pub struct ModelRow {
    pub id: String,
    pub chunks_typical: u64,
    pub chunks_worst_case: u64,
    pub manifest_bytes_typical: u64,
    pub manifest_bytes_worst_case: u64,
    pub mean_split_chunk_bytes_used: f64,
    pub violates: Vec<String>,
}

fn size_of(spec: ModelSpec) -> (u64, u64) {
    let m = build(&spec);
    (m.chunks.len() as u64, m.encoded_len() as u64)
}

/// `mean_split` maps a candidate id to the measured mean chunk size of split files (falls back to the nominal size).
pub fn rows(
    cands: &[Candidate],
    mean_split: &BTreeMap<String, f64>,
    files: usize,
    total: u64,
) -> Vec<ModelRow> {
    let chunk_limit = 4_000_000;
    let manifest_limit = 512u64 * 1024 * 1024;
    cands
        .iter()
        .map(|c| {
            let eff = mean_split
                .get(&c.id)
                .copied()
                .filter(|v| *v > 0.0)
                .unwrap_or(c.nominal as f64);
            let (typ, worst) = match c.family {
                Family::Cdc => (
                    ChunkModel::Cdc {
                        min: c.min as u32,
                        avg: eff as u32,
                        max: c.max as u32,
                    },
                    ChunkModel::Cdc {
                        min: c.min as u32,
                        avg: c.nominal as u32,
                        max: c.max as u32,
                    },
                ),
                Family::Fixed => (
                    ChunkModel::Fixed {
                        block: c.nominal as u32,
                    },
                    ChunkModel::Fixed {
                        block: c.nominal as u32,
                    },
                ),
            };
            let (ct, bt) = size_of(ModelSpec {
                seed: 1,
                files,
                total_bytes: total,
                chunking: typ,
                worst: false,
            });
            let (cw, bw) = size_of(ModelSpec {
                seed: 1,
                files,
                total_bytes: total,
                chunking: worst,
                worst: true,
            });
            let mut v = Vec::new();
            if cw > chunk_limit {
                v.push(format!("worst-case chunks {cw} > {}", chunk_limit));
            }
            if bw > manifest_limit {
                v.push(format!(
                    "worst-case manifest {bw} bytes > {}",
                    manifest_limit
                ));
            }
            ModelRow {
                id: c.id.clone(),
                chunks_typical: ct,
                chunks_worst_case: cw,
                manifest_bytes_typical: bt,
                manifest_bytes_worst_case: bw,
                mean_split_chunk_bytes_used: eff,
                violates: v,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_model_uses_measured_split_mean_and_preserves_limits() {
        let c = crate::chunking::cdc(256 * 1024);
        let means = [(c.id.clone(), 128.0 * 1024.0)].into();
        let rows = rows(&[c], &means, 20, 100_000_000);
        let r = &rows[0];
        assert_eq!(r.mean_split_chunk_bytes_used, 131072.0);
        assert!(r.chunks_worst_case >= r.chunks_typical);
        assert!(r.manifest_bytes_worst_case >= r.manifest_bytes_typical);
        assert!(r.violates.is_empty());
    }
}
