// SPDX-License-Identifier: 0BSD
//! Candidate grid and the production chunking rules (benchmark protocol):
//! a file of at most `max` bytes is exactly one chunk (an empty file has zero chunks), larger files are split per
//! file (FastCDC v2020 via the `fastcdc` crate with its default normalisation, or fixed blocks), and chunks never
//! cross file boundaries.
use crate::util::io_err;
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * 1024;
/// Files up to this size are loaded into memory for chunking, larger ones are streamed.
pub const LOAD_LIMIT: u64 = 256 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Cdc,
    Fixed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub id: String,
    pub family: Family,
    /// FastCDC average size, or the fixed block size.
    pub nominal: u64,
    pub min: u64,
    pub max: u64,
    /// Baselines (the predecessor project's 64 MiB blocks) are measured but never selected.
    pub selectable: bool,
}

fn label(n: u64) -> String {
    if n >= MIB {
        format!("{}m", n / MIB)
    } else {
        format!("{}k", n / KIB)
    }
}

pub fn cdc(avg: u64) -> Candidate {
    Candidate {
        id: format!("cdc-{}", label(avg)),
        family: Family::Cdc,
        nominal: avg,
        min: avg / 4,
        max: (avg * 4).min(16 * MIB),
        selectable: true,
    }
}

/// Informative candidates below the fixed grid (never selectable, outside the benchmark protocol).
fn cdc_info(avg: u64) -> Candidate {
    Candidate {
        selectable: false,
        ..cdc(avg)
    }
}

pub fn fixed(block: u64, selectable: bool) -> Candidate {
    Candidate {
        id: format!("fixed-{}", label(block)),
        family: Family::Fixed,
        nominal: block,
        min: block,
        max: block,
        selectable,
    }
}

/// The grid of the benchmark protocol: FastCDC avg 256 KiB / 1 MiB / 4 MiB, fixed 256 KiB / 1 MiB / 4 MiB / 16 MiB, and the
/// fixed 64 MiB baseline (the predecessor project's block size). Two informative candidates (FastCDC avg 128 KiB and
/// 64 KiB) follow the grid; they are measured but never selectable and never enter the rule.
pub fn grid() -> Vec<Candidate> {
    vec![
        cdc(256 * KIB),
        cdc(MIB),
        cdc(4 * MIB),
        fixed(256 * KIB, true),
        fixed(MIB, true),
        fixed(4 * MIB, true),
        fixed(16 * MIB, true),
        fixed(64 * MIB, false),
        cdc_info(128 * KIB),
        cdc_info(64 * KIB),
    ]
}

impl Candidate {
    /// Chunk lengths of an in-memory file.
    pub fn lengths(&self, data: &[u8]) -> Vec<usize> {
        let n = data.len() as u64;
        if n == 0 {
            return Vec::new();
        }
        if n <= self.max {
            return vec![n as usize];
        }
        match self.family {
            Family::Fixed => {
                let b = self.nominal as usize;
                let mut v = vec![b; data.len() / b];
                if !data.len().is_multiple_of(b) {
                    v.push(data.len() % b);
                }
                v
            }
            Family::Cdc => fastcdc::v2020::FastCDC::new(
                data,
                self.min as usize,
                self.nominal as usize,
                self.max as usize,
            )
            .map(|c| c.length)
            .collect(),
        }
    }

    /// Streams a file and calls `f` for every chunk.
    pub fn for_each_chunk(
        &self,
        path: &Path,
        size: u64,
        f: &mut dyn FnMut(&[u8]),
    ) -> io::Result<()> {
        if size == 0 {
            return Ok(());
        }
        let mut file = File::open(path)?;
        if size <= self.max {
            let mut buf = vec![0u8; size as usize];
            file.read_exact(&mut buf)?;
            f(&buf);
            return Ok(());
        }
        match self.family {
            Family::Fixed => {
                let mut buf = vec![0u8; self.nominal as usize];
                let mut left = size;
                while left > 0 {
                    let n = left.min(self.nominal) as usize;
                    file.read_exact(&mut buf[..n])?;
                    f(&buf[..n]);
                    left -= n as u64;
                }
            }
            Family::Cdc => {
                let ch = fastcdc::v2020::StreamCDC::new(
                    file,
                    self.min as usize,
                    self.nominal as usize,
                    self.max as usize,
                );
                for c in ch {
                    let c = c.map_err(io_err)?;
                    f(&c.data);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus_model::Rng;

    fn data(n: usize, seed: u64) -> Vec<u8> {
        let mut v = vec![0u8; n];
        Rng::new(seed).fill(&mut v);
        v
    }

    #[test]
    fn small_files_are_one_chunk() {
        for c in grid() {
            for n in [1u64, 100, c.max - 1, c.max] {
                let d = data(n as usize, 1);
                assert_eq!(c.lengths(&d), vec![n as usize], "{} size {n}", c.id);
            }
            assert!(c.lengths(&[]).is_empty());
            let over = c.lengths(&data(c.max as usize + 1, 1));
            assert!(over.len() >= 2);
            assert_eq!(over.iter().sum::<usize>(), c.max as usize + 1);
        }
    }

    #[test]
    fn lengths_cover_file_and_respect_bounds() {
        for c in grid().into_iter().filter(|c| c.max <= 16 * MIB) {
            let d = data((c.max * 3 + 12345) as usize, 2);
            let l = c.lengths(&d);
            assert_eq!(l.iter().sum::<usize>(), d.len());
            for (i, x) in l.iter().enumerate() {
                assert!((*x as u64) <= c.max);
                if i + 1 < l.len() {
                    assert!(
                        (*x as u64) >= c.min,
                        "{}: chunk {i} {x} < min {}",
                        c.id,
                        c.min
                    );
                }
            }
        }
    }

    #[test]
    fn stream_equals_memory() {
        let dir = std::env::temp_dir().join(format!("chunkbench-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for c in [cdc(256 * KIB), cdc(MIB), fixed(MIB, true)] {
            for n in [
                0,
                c.max as usize,
                c.max as usize + 1,
                20 * MIB as usize + 777,
            ] {
                let d = data(n, 3);
                let p = dir.join("f.bin");
                std::fs::write(&p, &d).unwrap();
                let mem = c.lengths(&d);
                let mut st = Vec::new();
                c.for_each_chunk(&p, d.len() as u64, &mut |b| st.push(b.len()))
                    .unwrap();
                assert_eq!(mem, st, "{} size {n}", c.id);
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deterministic_and_insert_stable() {
        let c = cdc(256 * KIB);
        let d = data(16 * MIB as usize, 4);
        assert_eq!(c.lengths(&d), c.lengths(&d));
        let mut d2 = d.clone();
        d2.splice(5 * MIB as usize..5 * MIB as usize, data(10_000, 9));
        let id = |d: &[u8], l: &[usize]| {
            let mut o = 0;
            let mut s = std::collections::HashSet::new();
            for x in l {
                s.insert(d[o..o + x].to_vec());
                o += x;
            }
            s
        };
        let a = id(&d, &c.lengths(&d));
        let b = id(&d2, &c.lengths(&d2));
        let common = a.intersection(&b).count();
        assert!(
            common as f64 > 0.8 * a.len() as f64,
            "CDC should resynchronise after an insert ({common}/{})",
            a.len()
        );
        // fixed blocks do not
        let f = fixed(256 * KIB, true);
        let a = id(&d, &f.lengths(&d));
        let b = id(&d2, &f.lengths(&d2));
        assert!((a.intersection(&b).count() as f64) < 0.5 * a.len() as f64);
    }
}
