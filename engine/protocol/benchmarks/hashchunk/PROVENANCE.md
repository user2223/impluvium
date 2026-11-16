<!-- SPDX-License-Identifier: 0BSD -->
# Benchmark source provenance

This package is the impluvium contributors' own work under 0BSD. It selectively
adapts the earlier 0BSD chunking and hashing experiment. It contains no source
from the inherited application and no copied third-party implementation.

| Module | Origin and adaptation |
|---|---|
| `corpus_model.rs` | Corpus model version 1: unchanged integer random stream, size distribution, paths and golden tests. |
| `gen.rs` | Deterministic two-version generator, boundary group and causal boundary filter; small filter tests added. |
| `chunking.rs` | Candidate grid, one-chunk rule and streaming adapter; boundary tests extended. |
| `select.rs` | Selection rule, both final comparisons and deviation checks; exact-threshold tests extended. |
| `analysis.rs` | Pair statistics, file reuse and transfer attribution; provisional schema construction replaced. |
| `model.rs` | Synthetic size-model parameters and limits; connected to the protocol adapter. |
| `schema_adapter.rs` | New adapter to the checked-in protocol types; synthetic sizing principles adapted from the earlier model. |
| `hashbench.rs` | Bounded reader/worker pipeline, whole-file hashes, verification and thread completion tests; empty-file hashes made explicit. |
| `cache.rs` | Linux page-cache preparation and residency probes; other platforms retain unknown cache state. |
| `quiet.rs` | CPU accounting and quiet-window gate, including deterministic clock/sampler tests. |
| `sysinfo.rs` | Machine information without hostnames; build-script and environment metadata replaced by explicit JSON input. |
| `treewalk.rs`, `util.rs` | Tree inventory and small formatting/statistics helpers; aggregate-output controls removed. |
| `main.rs`, `report.rs` | Reduced CLI and reports, relative configuration paths, measurement validation and small selection tests. |
| `tests/integration.rs` | Selected generator, weighting, digest, pipeline and CLI cases; small metadata/configuration cases added. |

The random generator is our own Rust rendition of the published SplitMix64
and xoshiro256** algorithms by Blackman and Vigna, whose reference code is
public domain. This attribution does not assert a cryptographic property.
The algorithm constants and corpus model version are preserved.

External libraries remain dependencies, with their own licenses. Their source,
FastCDC gear table and masks are not copied into this package:

| Direct dependency | Exact version | License |
|---|---|---|
| `impluvium-protocol` | 0.1.0 (workspace path, no client/server features) | 0BSD |
| `buffa`, `buffa-types` | 0.9.2 | Apache-2.0 |
| `fastcdc` | 5.0.0 | MIT |
| `sha2` | 0.11.1 | MIT OR Apache-2.0 |
| `blake3` | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 |
| `clap` | 4.6.7 | MIT OR Apache-2.0 |
| `libc` (Linux only) | 0.2.191 | MIT OR Apache-2.0 |
| `rayon` | 1.12.0 | MIT OR Apache-2.0 |
| `crossbeam-channel` | 0.5.17 | MIT OR Apache-2.0 |

Resolved transitive dependencies require the repository's existing license
boundary and dependency-license checks after lock resolution.

No separate workspace, lock file, build script, provisional schema, generated
code, parser, pre-scan, parser budget implementation or parser benchmark is
imported. No aggregate coarsening, legacy report fixtures, execution wrappers,
corpus data or copied upstream license text is included in this package.

`corpus/fetch_corpora.py` and selected loader tests adapt the earlier own 0BSD
loader: typed digest parts, separate verification statuses, direct dry runs,
stream-counted tar extraction and duplicate/symlink counterexamples are added.
`test_materials.py` and `compare_tree_digest.py` are new standard-library checks.
The manifest, six configurations and historical measurement records are our
own 0BSD descriptions and findings. Their JSON license assignment is carried
by the manifest and `results/index.json`; configurations also carry a license
field. The index documents label changes, digest encoding, rewritten support
notes and exclusions. Third-party corpus bytes, execution wrappers, private
results, the range-precheck tool and copied upstream license prose are omitted.

Size reports identify `protocol-manifest-v3` and `size_model_only: true`.
They use a fixed Timestamp, 64-bit sizes, sorted chunk IDs and remapped indexes.
Empty files have a whole-file hash and no chunks; model symlinks have mode zero.
Encoded sizes come from `buffa::Message::encoded_len()` and are tested against
encoded byte lengths. Model messages are not product-validation evidence.
Fixed blocks use unknown nonzero chunking value 2 only for benchmark comparison;
`fixed-64m` and the smaller informative CDC candidates are never selectable.

`--run-metadata FILE` accepts optional `source_revision` (40 lowercase hex digits),
`rustc` (the actual compiler version string) and `dependencies` (a map of actual
dependency versions). Incorrect supplied versions fail. Missing fields stay
unknown and prevent measurement qualification. Metadata is supplied by the
caller, not inferred from the build or from environment variables. Hash runs
also need a verified cache, whole-file hashes and the enforced quiet window to
qualify locally; this does not establish physical-machine measurement quality.
SHA-256 with FastCDC min/avg/max 65536/262144/1048576 remains the product default.
