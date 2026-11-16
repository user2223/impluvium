<!-- SPDX-License-Identifier: 0BSD -->
# Hash and chunk benchmark

This is a test tool for content-addressed game manifests. The selected defaults
are SHA-256 and FastCDC v2020 with min/average/max sizes
65536/262144/1048576 bytes. See the
[decision](../../spec/decisions/hash-chunking.md) for the selection rule and evidence.

`corpus/` contains public source descriptions, integrity pins, a standard-library
Python loader and six relative analysis configurations. Corpus archives and
extracted data are not distributed. `results/index.json` maps 54 historical
measurement and support records to published files or an explicit exclusion.
Our code, descriptions and measurements are under 0BSD; data obtained from the
listed upstream sources has its own licenses. See [PROVENANCE.md](PROVENANCE.md).

Use a disposable Linux container with the repository's pinned Rust toolchain,
Python 3.12 or newer and Node 24. Build prerequisites and the image pin are in
[the CI documentation](../../../../tools/ci/README.md). Keep source, dependencies,
data and results inside one disposable run volume. Prepare locked dependencies
with network access, then disconnect the container before testing or measuring.
Only explicit corpus loading needs access to public download servers.
No corpus data or benchmark executable belongs in a runtime image.

The corpus loader is written and tested for Linux filesystem behavior; it has
not been tested on other operating systems.

The current package uses the checked-in protocol types; it has no second schema
or build-time generator. From the repository root in the prepared container:

```sh
cargo test --locked --offline -p impluvium-hashchunk-bench --no-fail-fast
python3 -B -m unittest discover \
  -s engine/protocol/benchmarks/hashchunk/corpus -p 'test_*.py' -v
cargo build --locked --offline -p impluvium-hashchunk-bench
```

A small offline sequence exercises generation, relative configuration paths and
analysis without running the million-file model. Retain the output outside the
source export. These commands use the debug binary just built:

```sh
bench="$PWD/engine/protocol/benchmarks/hashchunk"
tool="$PWD/target/debug/chunkbench"
run="$(mktemp -d)"
mkdir -p "$run/data" "$run/results"
cp -R "$bench/corpus/configs" "$run/configs"
"$tool" gen --out "$run/data/gen" --files 12 --symlinks 2 --no-boundary
"$tool" digest "$run/data/gen/v1" > "$run/results/rust-digest.txt"
python3 -B "$bench/corpus/compare_tree_digest.py" \
  --tool "$tool" --tree "$run/data/gen/v1"
"$tool" analyze --config "$run/configs/class1-generator.json" \
  --no-model --out "$run/results/small-analysis.json"
```

The small analysis is a diagnostic: it lacks two deciding classes and a full
size model. Generation defaults to seed 20261101. The full historical corpus
uses 100000 requested regular files, 50 symlinks and the boundary group;
`--no-boundary` above keeps the smoke test small. Python and Rust tree digests
include relative file names, sizes, octal modes and SHA-256 content hashes,
followed by symlink names and targets. They sort files and links separately by
UTF-8 path bytes, prefix records with `F` or `L`, separate fields with NUL and
terminate each record with LF. A single-file root uses name `file` and mode 0644.
Use UTF-8 names; the comparison checks the same tree and file permissions.

To inspect planned downloads without network or filesystem changes:

```sh
python3 -B "$bench/corpus/fetch_corpora.py" \
  --manifest "$bench/corpus/corpus-manifest.json" \
  --dest "$run/data" --dry-run
```

For the full reconstruction, first create a fresh run directory inside the
disposable run volume. Do not reuse the smoke-test generator tree: existing
symlinks prevent generation into the same destination.

```sh
run="$(mktemp -d)"
mkdir -p "$run/data" "$run/results"
cp -R "$bench/corpus/configs" "$run/configs"
```

To explicitly load the full public set, enable network in the disposable
container and run the loader command above with this new destination and without
`--dry-run`, adding `--with-optional`
for the Linux source-tree diagnostic. `--only ID` is repeatable and restricts
loading to named download inputs. Download URLs and versions are historical
records, not a claim of present availability. No live retrieval is required by CI.
The historical complete corpus occupied roughly 30 GB; leave additional space
for downloads, temporary extraction and results.

The loader verifies official MD5/SHA-256/SHA-512 values where recorded, plus
strong SHA-256 historical pins even when an official checksum is weak or absent.
Every download requires an expected checksum before loading. The Sintel ZIP
uses its recorded historical SHA-256 pin, not an upstream-published checksum.
The unpinned download helper is omitted; recording an arbitrary first response
as trusted input is not supported.
All prepared-tree pins bind the first load. A mismatch removes a bad downloaded
file or temporary tree and fails; failed extraction retains the previous target.
Existing matching trees are reused, while stale trees are prepared again.
`recorded-checksums.json` records local verification independently of the pinned
manifest. A local record contradicting a manifest tree pin is an error.

Downloads require HTTPS, including at most five redirects, whose bodies are
closed unread. Streaming downloads use the smaller of the declared size and
`--max-download-bytes` (8 GiB by default); without a declared size the latter
limit applies. Defaults bound extraction to 1000000 members, 8 GiB per file and
64 GiB total, enforced while writing. The `--max-members`, `--max-file-bytes`,
`--max-total-bytes` and `--max-download-bytes` options can lower these limits.
Traversal, duplicate archive names, special files and escaping links are rejected;
symlinks above targets and in the download directory are refused. Extraction
occurs in a fresh sibling directory. Hard links in tar archives are skipped, as
in the historical preparation. Tar symlink targets must be relative, stay inside
the root at every lexical step and never traverse or name another archive symlink,
regardless of entry order; ZIP link attributes are treated as regular-file data.
All tar members count against the member limit, including
entries omitted by prefix stripping. The loader assumes no concurrent adversarial
writer to its destination. `--allow-file` exists for local test fixtures only.

Manifest version 2 encodes digests as `{ "algorithm": "sha256", "hex_parts": [...] }`.
Each part has exactly 32 lowercase hex characters: one for MD5, two for SHA-256,
four for SHA-512. Concatenation recovers the original value; malformed parts or
wrong algorithms fail. The historical loader image digest and license-source
hashes use the same representation. Version 1 manifests are not accepted.

`checksum_verification`, `tree_verification` and `signature_verification` are
separate fields. Historical checksum verification does not prove a signature.
Linux PGP and 0 A.D. Minisign remain `not_verified`. Unsigned sources are marked
`not_available`. Licensing is independent of both checks. The 0 A.D. license
source is a hashed snapshot of `master`, not release-specific evidence.
The boundary-filtered generator digest was not recorded and remains missing.

For the complete reconstruction, use the fresh directory created for public
loading above, never the smoke-test tree. After loading, disconnect network and
generate the original input with:

```sh
"$tool" gen --out "$run/data/gen" --seed 20261101 --files 100000 --symlinks 50
"$tool" gen-filter --src "$run/data/gen" --out "$run/data/gen-wo-boundary" \
  --seed 20261101 --files 100000
"$tool" gen --out "$run/data/gen-nb" --seed 20261101 --files 100000 \
  --symlinks 50 --no-boundary
"$tool" analyze --config "$run/configs/free-corpora.json" \
  --out "$run/results/current-analysis.json"
```

Use `digest` to compare generated v1/v2 trees with the recorded generator pins.
The six configurations resolve paths relative to their own directory and expect
`../data/`. `sensitivity-filtered-class1.json` removes exactly the boundary files
from the same generated trees; `diagnostic-no-boundary.json` regenerates without
the boundary group and is a different intervention. `sensitivity-xonotic-only.json`
removes the 0 A.D. pair. Diagnostic classes never enter the deciding class mean.

For a new qualified run, pass `--run-metadata FILE`: a JSON object containing
the actual 40-character `source_revision`, actual `rustc` version string and a
`dependencies` map of the exact versions in `Cargo.toml` (including the protocol
path package). Missing fields remain unknown and prevent qualification. Archive
this input with the source revision and results. Model results identify
`protocol-manifest-v3` and `size_model_only: true`. Give new result collections
an explicit format version and adapter revision in their accompanying index;
the tool itself does not emit a `result_format_version` field.

A hash invocation, in a measured quiet window with immutable data, is:

```sh
"$tool" --run-metadata "$run/run-metadata.json" hash \
  --tree "$run/data/gen/v2" --out "$run/results/hash-generator-r1-warm.json" \
  --chunking cdc-256k --algos sha256,blake3 --workers 1,4 --readers 1 \
  --stages build,verify,chunk-only --cache warm --reps 5 --max-busy-percent 10
```

Repeat over generator v2, Xonotic 0.8.6, 0 A.D. 0.28.0 and the Linux 7.3
archive, with 1/4 readers and warm/cold caches: 16 files. Warm one-reader
comparisons implement the historical hash rule; four-reader and cold values are
additional evidence. Whole-file hashes of split files remain enabled.
Build includes reading, chunking, hashing and index construction; verify reads
with recorded boundaries, hashes and compares. The quiet gate requires three
consecutive one-second CPU samples at at most ten percent external activity,
with a 120-second timeout, after cache preparation and before cache rechecking.
A local gate cannot establish that a VM host is quiet. Cache mismatch or hash
mismatch fails after writing evidence; unknown cache state cannot qualify.

Raw results retain per-repetition seconds, medians, throughput, readers, workers,
file-hash setting, cache state, CPU samples, machine information and revision.
`results/index.json` adds assessed quality, original quality, documentary revision
associations, missing metadata and publication changes. `not_recorded` never
means measured zero; `not_applicable` marks a different kind of record.
All historical measurement values are retained. Labels are neutralized, checksum
encoding is migrated, three round logs retain times/load in English metadata,
and the old execution-wrapper summary is excluded. The ZIP precheck contains
only our numeric findings; upstream license text is omitted.

Environment details that do not affect the measurements were reduced after measurement: kernel versions omit suffixes, recorded timezone offsets were converted to UTC, and load-average strings omit the last process number. Measurement values are unchanged. Checksums computed for published result files identify the reduced files; corpus-file and tree checksums continue to identify the unchanged inputs.

Historical analysis, size and parser results use a provisional schema with a
scalar timestamp, insertion-order chunks and different symlink modes. Current
sizes use the checked-in protocol schema, a Timestamp, sorted chunk IDs and
remapped indexes, 64-bit sizes and zero symlink modes. Identical manifest byte
sizes are not promised; `corpus-model/1` is a separate, preserved input axis.
Parser timings do not validate the current adapter or a current product budget.
VM and Windows records remain informative even where their raw quality flag
says otherwise; see the historical measurement notes. No additional hash-choice
or 128 KiB investigation is required for 0.1.
