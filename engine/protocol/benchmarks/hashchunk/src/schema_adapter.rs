// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD
//! Size measurements using the checked-in protocol types. These are size models,
//! not validated product manifests. Fixed blocks use unknown nonzero value 2
//! solely as a benchmark comparison; the normative enum is unchanged.
use crate::analysis::FileRec;
use crate::chunking::{Candidate, Family};
use buffa::Message;
use buffa_types::google::protobuf::Timestamp;
use impluvium_protocol::proto::impluvium::manifest::v3 as pb;
use std::collections::HashMap;

pub const MODEL_ID: &str = "protocol-manifest-v3";
const CREATED_AT: i64 = 1_700_000_000;

fn header(cand: &Candidate, hash: pb::HashAlgorithm) -> pb::Header {
    pb::Header {
        hash_algorithm: hash.into(),
        chunking: pb::Chunking {
            algorithm: match cand.family {
                Family::Cdc => pb::ChunkingAlgorithm::FastcdcV2020.into(),
                Family::Fixed => buffa::EnumValue::from(2),
            },
            min_size: cand.min,
            avg_size: cand.nominal,
            max_size: cand.max,
            ..Default::default()
        }
        .into(),
        ..Default::default()
    }
}

/// Sort the chunk table and remap every reference without changing file order.
fn assemble(
    mut entries: Vec<pb::Entry>,
    chunks: Vec<pb::Chunk>,
    cand: &Candidate,
    hash: pb::HashAlgorithm,
) -> pb::Manifest {
    let mut order: Vec<usize> = (0..chunks.len()).collect();
    order.sort_by(|a, b| chunks[*a].id.cmp(&chunks[*b].id));
    let mut remap = vec![0u32; chunks.len()];
    for (new, old) in order.iter().enumerate() {
        remap[*old] = new as u32;
    }
    for entry in &mut entries {
        for index in &mut entry.chunk_indexes {
            *index = remap[*index as usize];
        }
    }
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let total_size = entries.iter().map(|e| e.size).sum();
    let mut slots: Vec<Option<pb::Chunk>> = chunks.into_iter().map(Some).collect();
    pb::Manifest {
        format: 3,
        game_id: "00000000-0000-4000-8000-000000000001".into(),
        version_id: "00000000-0000-4000-8000-000000000002".into(),
        created_at: Timestamp {
            seconds: CREATED_AT,
            nanos: 0,
            ..Default::default()
        }
        .into(),
        header: header(cand, hash).into(),
        total_size,
        entries,
        chunks: order
            .into_iter()
            .map(|i| slots[i].take().expect("unique index"))
            .collect(),
        ..Default::default()
    }
}

pub fn measured(recs: &[FileRec], ci: usize, cand: &Candidate) -> pb::Manifest {
    let mut table = HashMap::new();
    let mut chunks = Vec::new();
    let entries = recs
        .iter()
        .map(|r| {
            let chunk_indexes = r.chunks[ci]
                .iter()
                .map(|(id, size)| {
                    let next = chunks.len() as u32;
                    *table.entry(*id).or_insert_with(|| {
                        chunks.push(pb::Chunk {
                            id: id.to_vec(),
                            size: u64::from(*size),
                            ..Default::default()
                        });
                        next
                    })
                })
                .collect();
            pb::Entry {
                path: r.rel.clone(),
                r#type: pb::EntryType::Regular.into(),
                size: r.size,
                hash: r.hash.to_vec(),
                mode: 0o644,
                chunk_indexes,
                ..Default::default()
            }
        })
        .collect();
    // Analysis identities are BLAKE3, independently of the default product hash.
    assemble(entries, chunks, cand, pb::HashAlgorithm::Blake3256)
}

pub fn measured_len(recs: &[FileRec], ci: usize, cand: &Candidate) -> u64 {
    measured(recs, ci, cand).encoded_len() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_fields_sorting_and_encoded_length() {
        let recs = vec![
            FileRec {
                rel: "z".into(),
                size: 9,
                hash: [3; 32],
                chunks: vec![vec![([9; 32], 4), ([1; 32], 5)]],
            },
            FileRec {
                rel: "empty".into(),
                size: 0,
                hash: *blake3::hash(&[]).as_bytes(),
                chunks: vec![vec![]],
            },
        ];
        let c = crate::chunking::cdc(256 * 1024);
        let m = measured(&recs, 0, &c);
        assert!(m.created_at.is_set());
        assert_eq!(m.created_at.seconds, CREATED_AT);
        assert_eq!(m.created_at.nanos, 0);
        assert_eq!(m.header.chunking.min_size, 65536u64);
        assert_eq!(m.header.chunking.avg_size, 262144u64);
        assert_eq!(m.header.chunking.max_size, 1048576u64);
        assert_eq!(m.header.hash_algorithm, pb::HashAlgorithm::Blake3256);
        assert_eq!(m.entries[0].path, "empty");
        assert_eq!(m.entries[0].hash, blake3::hash(&[]).as_bytes().to_vec());
        assert!(m.entries[0].chunk_indexes.is_empty());
        assert_eq!(m.chunks[0].id, vec![1; 32]);
        assert_eq!(m.entries[1].chunk_indexes, vec![1, 0]);
        assert_eq!(m.chunks[1].size, 4u64);
        assert_eq!(
            m.encoded_len(),
            u32::try_from(m.encode_to_vec().len()).unwrap()
        );
        let f = measured(&recs, 0, &crate::chunking::fixed(1024, true));
        assert_eq!(f.header.chunking.algorithm, buffa::EnumValue::from(2));
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ChunkModel {
    /// FastCDC-like: files up to `max` are one chunk, larger files are split into `ceil(size/avg)` pieces.
    Cdc { min: u32, avg: u32, max: u32 },
    /// Fixed blocks: files up to `block` are one chunk.
    Fixed { block: u32 },
}

#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    pub seed: u64,
    pub files: usize,
    pub total_bytes: u64,
    pub chunking: ChunkModel,
    /// Worst case: every split chunk of a CDC file has the minimum size.
    pub worst: bool,
}

impl ChunkModel {
    pub fn max(&self) -> u64 {
        match *self {
            ChunkModel::Cdc { max, .. } => max as u64,
            ChunkModel::Fixed { block } => block as u64,
        }
    }
    pub fn n_chunks(&self, size: u64, worst: bool) -> u64 {
        if size == 0 {
            0
        } else if size <= self.max() {
            1
        } else {
            match *self {
                ChunkModel::Cdc { min, avg, .. } => {
                    size.div_ceil(if worst { min as u64 } else { avg as u64 })
                }
                ChunkModel::Fixed { block } => size.div_ceil(block as u64),
            }
        }
    }
}

/// Builds a manifest with `files` entries (plus ~0.1 % symlinks) whose sizes follow `model_game_sizes`.
/// All chunks are distinct (no dedup), which is the conservative case for the size.
pub fn build(spec: &ModelSpec) -> pb::Manifest {
    use crate::corpus_model::{derive_seed, model_game_sizes, synth_path, Rng};
    use sha2::{Digest, Sha256};
    let sizes = model_game_sizes(spec.seed, spec.files, spec.total_bytes);
    let mut paths: Vec<(String, u64)> = sizes
        .iter()
        .enumerate()
        .map(|(i, s)| (synth_path(spec.seed, i as u64), *s))
        .collect();
    paths.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut rng = Rng::new(derive_seed(spec.seed, 0x6d61_6e69, 2));
    let mut entries = Vec::with_capacity(paths.len() + paths.len() / 1000);
    let mut chunks: Vec<pb::Chunk> = Vec::new();
    for (path, size) in paths {
        let n = spec.chunking.n_chunks(size, spec.worst);
        let mut idx = Vec::with_capacity(n as usize);
        let mut hash = [0u8; 32];
        rng.fill(&mut hash);
        if size == 0 {
            hash.copy_from_slice(&Sha256::digest([]));
        }
        let mut left = size;
        for k in 0..n {
            let piece = if k + 1 == n { left } else { size / n };
            left -= piece;
            let mut id = [0u8; 32];
            if n == 1 {
                id = hash; // a one-chunk file's chunk id equals its file hash
            } else {
                rng.fill(&mut id);
            }
            idx.push(chunks.len() as u32);
            chunks.push(pb::Chunk {
                id: id.to_vec(),
                size: piece,
                ..Default::default()
            });
        }
        entries.push(pb::Entry {
            path,
            r#type: pb::EntryType::Regular.into(),
            size,
            hash: hash.to_vec(),
            mode: if rng.below(100) < 3 { 0o755 } else { 0o644 },
            symlink_target: String::new(),
            chunk_indexes: idx,
            ..Default::default()
        });
    }
    // ~0.1 % symlinks with relative targets; names sort into the tree, so rebuild the sort afterwards.
    let n_links = spec.files / 1000;
    for i in 0..n_links {
        let t = &entries[(rng.below(entries.len() as u64)) as usize].path;
        entries.push(pb::Entry {
            path: format!("links/l{i:06x}"),
            r#type: pb::EntryType::Symlink.into(),
            size: 0,
            hash: Vec::new(),
            mode: 0,
            symlink_target: format!("../{t}"),
            chunk_indexes: Vec::new(),
            ..Default::default()
        });
    }
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let cand = match spec.chunking {
        ChunkModel::Cdc { min, avg, max } => Candidate {
            id: "model-cdc".into(),
            family: Family::Cdc,
            nominal: avg as u64,
            min: min as u64,
            max: max as u64,
            selectable: false,
        },
        ChunkModel::Fixed { block } => crate::chunking::fixed(block as u64, false),
    };
    assemble(entries, chunks, &cand, pb::HashAlgorithm::Sha256)
}

#[cfg(test)]
mod model_tests {
    use super::*;
    #[test]
    fn synthetic_model_links_empty_files_and_remapped_references() {
        let spec = ModelSpec {
            seed: 1,
            files: 1000,
            total_bytes: 100_000_000,
            chunking: ChunkModel::Cdc {
                min: 65536,
                avg: 262144,
                max: 1048576,
            },
            worst: false,
        };
        let m = build(&spec);
        assert!(m.chunks.windows(2).all(|w| w[0].id < w[1].id));
        assert!(m.entries.windows(2).all(|w| w[0].path < w[1].path));
        let link = m
            .entries
            .iter()
            .find(|e| e.r#type == pb::EntryType::Symlink)
            .unwrap();
        assert_eq!(link.mode, 0);
        assert!(link.hash.is_empty() && link.chunk_indexes.is_empty());
        for e in m
            .entries
            .iter()
            .filter(|e| e.r#type == pb::EntryType::Regular)
        {
            assert_eq!(
                e.chunk_indexes
                    .iter()
                    .map(|i| m.chunks[*i as usize].size)
                    .sum::<u64>(),
                e.size
            );
            if e.size == 0 {
                use sha2::{Digest, Sha256};
                assert!(e.chunk_indexes.is_empty());
                assert_eq!(e.hash, Sha256::digest([]).to_vec());
            } else if e.chunk_indexes.len() == 1 {
                assert_eq!(m.chunks[e.chunk_indexes[0] as usize].id, e.hash);
            }
        }
        assert_eq!(
            m.encoded_len(),
            u32::try_from(m.encode_to_vec().len()).unwrap()
        );
    }
}

#[cfg(test)]
mod empty_model_test {
    use super::*;
    #[test]
    fn empty_model_file_has_the_sha256_empty_hash_and_no_chunks() {
        use sha2::{Digest, Sha256};
        let m = build(&ModelSpec {
            seed: 1,
            files: 1,
            total_bytes: 0,
            chunking: ChunkModel::Fixed { block: 1024 },
            worst: false,
        });
        assert!(m.chunks.is_empty());
        assert!(m.entries[0].chunk_indexes.is_empty());
        assert_eq!(m.entries[0].hash, Sha256::digest([]).to_vec());
        assert_eq!(m.total_size, 0);
    }
}
