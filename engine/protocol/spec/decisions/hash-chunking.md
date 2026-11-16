<!-- SPDX-License-Identifier: 0BSD -->
# SHA-256 and FastCDC defaults

For 0.1, content hashing uses SHA-256. Chunking uses FastCDC v2020 with
minimum 65536, average 262144 and maximum 1048576 bytes (64/256/1024 KiB).
The decision is final for this version. A file no larger than the maximum is
one chunk; an empty file has a whole-file hash and no chunks. Chunks do not
cross file boundaries. Unchanged files can be reused whole.

The [benchmark materials](../../benchmarks/hashchunk/README.md) contain the
input descriptions, pinned digests, relative configurations, loader tests and
complete historical result index. Corpus bytes are obtained separately from
the public sources; they are not part of this repository.

The selectable CDC grid has average sizes 256 KiB, 1 MiB and 4 MiB, with
minimum average/4 and maximum min(4*average, 16 MiB). Fixed comparison blocks
are 256 KiB, 1 MiB, 4 MiB and 16 MiB. Fixed 64 MiB is an unselectable baseline.
The FastCDC library version is 5.0.0, v2020, normalization level 1. Chunk
identity in the transfer analysis uses BLAKE3-256 independently of the selected
manifest hash. Fixed blocks are a benchmark comparison, not a new protocol enum.

For each old/new pair, T counts bytes of distinct chunks in the new version
that are absent from the old one, divided by the new version's byte count.
Pair percentages are averaged within each class; class percentages then have
equal weight. Byte-weighting the pairs or mixing diagnostics into that mean
would implement a different rule. The deciding classes are a seeded synthetic
many-small-files tree, compressed Linux archives and game package files.

The generator supplies one pair. Archives supply Linux 7.2 to 7.3 and 7.3 to
7.3.1; package files supply Xonotic 0.8.5 to 0.8.6 and 0 A.D. 0.27.1 to
0.28.0. The extracted Linux source trees and two independently encoded Sintel
videos are diagnostics. Independent video encodings do not represent an update
pair and cannot dilute the deciding archive class toward full replacement.

Candidates first have to fit a 100 GB, one-million-file model: at most
4000000 chunks and a 512 MiB encoded manifest. All-distinct chunk IDs prevent
deduplication from hiding model cost; the worst CDC case uses minimum-size
split chunks. The model contains 999000 files from the size distribution and
1000 large files filling the remaining bytes. It is a size model, not a valid
product manifest or product validator.

Within CDC, start at the largest eligible candidate. A smaller candidate
replaces the current one only if it improves the equally weighted transfer
mean by at least five percentage points. Apply the same walk to fixed blocks,
starting at 16 MiB. Compare each candidate to the current choice, rather than
the neighboring candidate in the original grid.

Call the family winners C* and F*. CDC wins only if C* beats both F* and the
fixed block of the same nominal size by at least five points. A missing end
comparison is not a pass. For every decisive comparison, report any class
moving more than ten percentage points in the opposite direction, including
comparisons within a family and both end comparisons. Exactly ten is not
more than ten. Diagnostics and incomplete grids cannot establish a decision.

The complete historical analysis yielded:

| Candidate | Small files T % | Archives T % | Package files T % | Equal class mean T % |
|---|---:|---:|---:|---:|
| cdc-256k | 12.27 | 100 | 49.43 | 53.90 |
| cdc-1m | 14.89 | 100 | 68.90 | 61.26 |
| cdc-4m | 21.85 | 100 | 85.82 | 69.22 |
| fixed-256k | 12.31 | 100 | 90.71 | 67.67 |
| fixed-1m | 13.45 | 100 | 93.04 | 68.83 |
| fixed-4m | 15.82 | 100 | 95.74 | 70.52 |
| fixed-16m | 20.57 | 100 | 97.60 | 72.72 |
| fixed-64m, baseline | 24.68 | 100 | 98.22 | 74.30 |

CDC moved 4 MiB to 1 MiB (7.96 points), then to 256 KiB (7.36 points).
Fixed 4 MiB and 1 MiB did not replace 16 MiB (2.20 and 3.89 points), but
256 KiB did (5.05 points). Both final comparisons were against fixed 256 KiB;
CDC gained 13.77 points. No opposing class deviation exceeded ten points.
No selectable candidate violated the historical model limits. The selected
CDC worst case was 2180778 chunks and about 168 MiB of provisional manifest.
The benefit comes mainly from package files; compressed archives were full
replacement for every candidate. Small files changed little between the
selected CDC and same-size fixed blocks.

The generator includes a boundary group around the chunking thresholds.
Removing exactly those files from the same tree retained the choice (mean
about 54.4 percent; fixed-family winner 16 MiB). Regenerating without the group
is also recorded, but changes the generation process and is a separate
diagnostic. Restricting the package class to Xonotic retained the choice
(mean about 47.9 percent). These sensitivities do not redefine the base grid.

Informative, unselectable 128 KiB CDC reached mean T 50.40 percent, improving
256 KiB by only 3.50 points. Its historical worst case was 3524365 chunks and
about 224.4 MiB. Informative 64 KiB reached 47.79 percent but 6139361
worst-case chunks, exceeding the four-million limit. These historical values
supply context; no further smaller-chunk study is required for 0.1.

The complete historical hash set contains sixteen JSON files: four trees
(generator, Linux archive, Xonotic and 0 A.D.), 1/4 readers, warm/cold caches.
Each includes SHA-256 and BLAKE3 with 1/4 workers, build and verify paths,
chunk-only baseline and five repetitions. The tool revision is `6c2451abe2cb`.
The host reported Ryzen 9 5900X, SHA-NI, 24 logical CPUs, a six-CPU container
limit and 12 GiB memory limit. The recorded preflight had load average 0.93,
no override and five excluded background service containers.

The historical hash rule used warm measurements with **one reader**: changing
the number of readers does not replace that comparison. Build includes index
construction; verification includes comparisons against stored hashes. Both
also include whole-file hashing of split files. Cache residency and quiet
CPU samples were checked for each repetition. Cold results are informative.
The warm generator verify path with four workers gave BLAKE3/SHA-256 median
throughput ratio 0.891998, below parity; its observed cross-repetition extreme
ratio range was about 0.880 to 1.182, not a confidence interval. Thus the
historical all-warm-comparisons condition for replacing SHA-256 failed.
The recorded physical reference without SHA-NI was also absent. The final
choice of SHA-256 closes that investigation; no new physical reference is due.

Later SHA-NI off/on Linux VM measurements are informative: the host was not
quiet and executable revisions were missing. Masking CPU features in a VM
does not measure a physical CPU without SHA extensions. Windows supplied a
warm plausibility run with an explicit historical quiet-gate bypass, unknown
cache state and missing machine fields. Its retained patch is evidence of
that limitation, not a change for the current tool. Raw `decision_grade` flags
remain intact; the index supplies the correct informative assessment.

Historical manifest sizes and parser timings belong to
`historical-provisional-manifest`. The current adapter uses
`protocol-manifest-v3`: Timestamp fields, sorted chunk IDs and remapped file
indexes, 64-bit sizes and mode zero for symlinks. New measurements get new
files, a result-format version and their own tool/schema-adapter revision.
The input generator remains `corpus-model/1`; preserving the generator does
not make these schema models identical. Old parser measurements neither
validate current product parsing nor establish its memory/time budget.

Checksums and prepared-tree digests are pinned separately from signatures.
Linux PGP and 0 A.D. Minisign are still `not_verified`; this is an open
integrity-evidence task, not an open hash decision. License descriptions are
separate again, including the limitation that the hashed 0 A.D. license source
is a historical `master` snapshot. No corpus data or copied upstream license
text accompanies these measurement records.
