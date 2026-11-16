// SPDX-License-Identifier: 0BSD
//! Deterministic building blocks for the synthetic corpus and the manifest model.
//!
//! Everything here uses integer arithmetic only, so the same seed yields the same bytes on every platform and with
//! every compiler version. Do not replace the PRNG or the tables without bumping `MODEL_VERSION`: the generated
//! corpus (and therefore every published benchmark number) depends on them.

pub const MODEL_VERSION: &str = "corpus-model/1";

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

/// SplitMix64 finalizer; also used to derive independent per-file seeds.
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derives a sub-seed from a base seed and two labels.
pub fn derive_seed(seed: u64, a: u64, b: u64) -> u64 {
    splitmix64(
        splitmix64(seed ^ a.wrapping_mul(0xD6E8_FEB8_6659_FD93))
            ^ b.wrapping_mul(0xA076_1D64_78BD_642F),
    )
}

/// xoshiro256** seeded through SplitMix64.
#[derive(Clone)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        let mut s = [0u64; 4];
        for v in s.iter_mut() {
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            *v = splitmix64(x);
        }
        Rng { s }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }

    /// Uniform in `0..n` (n > 0), multiply-shift, no modulo bias worth caring about for n << 2^64.
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        (((self.next_u64() as u128) * (n as u128)) >> 64) as u64
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        debug_assert!(lo <= hi);
        lo + self.below(hi - lo + 1)
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        let (chunks, rem) = buf.as_chunks_mut::<8>();
        for c in chunks {
            *c = self.next_u64().to_le_bytes();
        }
        if !rem.is_empty() {
            let b = self.next_u64().to_le_bytes();
            rem.copy_from_slice(&b[..rem.len()]);
        }
    }
}

/// File size classes of the "many small files" corpus: (weight out of 10000, lowest size, highest size), uniform
/// inside a class. Rationale: game trees have many tiny config/script files, a body of 8-64 KiB assets, and a thin
/// tail of large pak-like files. The tail is cut at 32 MiB to keep the corpus below the disk budget.
pub const SIZE_TABLE: [(u64, u64, u64); 9] = [
    (50, 0, 0),
    (4113, 1, KIB),
    (3300, KIB + 1, 8 * KIB),
    (2000, 8 * KIB + 1, 64 * KIB),
    (400, 64 * KIB + 1, 256 * KIB),
    (100, 256 * KIB + 1, MIB),
    (30, MIB + 1, 4 * MIB),
    (6, 4 * MIB + 1, 16 * MIB),
    (1, 16 * MIB + 1, 32 * MIB),
];

/// Draws one file size from `SIZE_TABLE`.
pub fn sample_size(rng: &mut Rng) -> u64 {
    let mut r = rng.below(10_000);
    for (w, lo, hi) in SIZE_TABLE {
        if r < w {
            return rng.range(lo, hi);
        }
        r -= w;
    }
    unreachable!("weights sum to 10000")
}

const EXTS: [&str; 7] = ["dat", "bin", "cfg", "txt", "pak", "tex", "snd"];

/// Synthetic relative path for a file id: depth 1..5 below a few top-level folders, 1 % long names.
/// Paths are unique because the file id is part of the name.
pub fn synth_path(seed: u64, uid: u64) -> String {
    let mut r = Rng::new(derive_seed(seed, uid, 0x70));
    let depth = 1 + r.below(5);
    let mut p = String::new();
    let top = ["data", "maps", "bin", "cfg", "media"][r.below(5) as usize];
    p.push_str(top);
    for _ in 0..depth {
        p.push('/');
        p.push_str(&format!("dir{:02}", r.below(40)));
    }
    p.push('/');
    if r.below(100) == 0 {
        // long name, stays far below the 255-byte component limit
        let len = 60 + r.below(120) as usize;
        let mut n = format!("long{uid:06x}_");
        while n.len() < len {
            n.push((b'a' + r.below(26) as u8) as char);
        }
        p.push_str(&n);
    } else {
        p.push_str(&format!("f{uid:06x}"));
    }
    p.push('.');
    p.push_str(EXTS[r.below(EXTS.len() as u64) as usize]);
    p
}

/// The size of the 100 GB / 1 million file model game: 999 000 files drawn from `SIZE_TABLE`, plus 1 000 large
/// files that fill the remainder up to `total` bytes (weights uniform in 0.5..1.5, so sizes differ by up to 3x).
pub fn model_game_sizes(seed: u64, files: usize, total: u64) -> Vec<u64> {
    let big = (files / 1000).max(1);
    let small = files - big;
    let mut r = Rng::new(derive_seed(seed, 0x6d6f_6465, 1));
    let mut sizes: Vec<u64> = (0..small).map(|_| sample_size(&mut r)).collect();
    let used: u64 = sizes.iter().sum();
    let rest = total.saturating_sub(used);
    let w: Vec<u64> = (0..big).map(|_| 500 + r.below(1000)).collect();
    let ws: u64 = w.iter().sum();
    let mut assigned = 0u64;
    for (i, wi) in w.iter().enumerate() {
        let s = if i + 1 == big {
            rest - assigned
        } else {
            ((rest as u128 * *wi as u128) / ws as u128) as u64
        };
        assigned += s;
        sizes.push(s);
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum() {
        assert_eq!(SIZE_TABLE.iter().map(|t| t.0).sum::<u64>(), 10_000);
    }

    #[test]
    fn rng_is_stable() {
        // Golden values: changing them changes every generated corpus.
        let mut r = Rng::new(1);
        let v: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        assert_eq!(v, GOLDEN);
    }
    const GOLDEN: [u64; 3] = [
        18001451845637162709,
        15091038358276433251,
        7540168338507695658,
    ];

    #[test]
    fn fill_preserves_word_order_and_partial_word_consumption() {
        for len in 0usize..=25 {
            let mut rng = Rng::new(7);
            let mut reference = rng.clone();
            let expected: Vec<u8> = (0..len.div_ceil(8))
                .flat_map(|_| reference.next_u64().to_le_bytes())
                .take(len)
                .collect();
            let mut actual = vec![0; len];
            rng.fill(&mut actual);
            assert_eq!(actual, expected);
            assert_eq!(rng.next_u64(), reference.next_u64());
        }
    }

    #[test]
    fn path_is_deterministic_and_bounded() {
        for uid in 0..2000 {
            let a = synth_path(7, uid);
            assert_eq!(a, synth_path(7, uid));
            assert!(a.len() < 4096 && a.split('/').all(|c| c.len() <= 255 && !c.is_empty()));
        }
    }

    #[test]
    fn model_game_totals() {
        let s = model_game_sizes(3, 100_000, 10_000_000_000);
        assert_eq!(s.len(), 100_000);
        assert_eq!(s.iter().sum::<u64>(), 10_000_000_000);
    }
}
