<!-- SPDX-License-Identifier: 0BSD -->
# Historical measurement conditions, 2026-11-09

These notes describe the provisional manifest model and the historical tool.
SHA-256 is final for 0.1. The measurements do not reopen that choice.
See `../index.json` for per-file quality assessments and missing metadata.

The parser round ran from 01:08:22 to 01:08:42, with no CI jobs at either
endpoint and no virtual machines. Six background service containers were
excluded from the historical quiet precheck. The reported one-minute load
average was 1.35. Earlier load averages had ranged from 3 to 7; that earlier
period was not used. The tool revision was `e1af3927ada0`; the initial parser
records used `64306afbd875`.

| Decoder | Initial decode / validate seconds | Quiet decode / validate seconds | Quiet peak RSS, rounded decimal MB |
|---|---|---|---|
| prost, 1 million entries | 0.2975 / 0.0419 | 0.2693 / 0.0404 | 457 |
| buffa, 1 million entries | 0.2706 / 0.0494 | 0.2530 / 0.0471 | 537 |
| buffa view, 1 million entries | 0.1437 / 0.0502 | 0.1339 / 0.0487 | 321 |
| prost, 100 thousand entries | 0.0289 / 0.0042 | 0.0262 / 0.0040 | see raw record |
| prost, 2 million entries | 0.5928 / 0.0831 | 0.5398 / 0.0801 | 913 |

The regular manifests were approximately 128 and 256 decimal MB. The quiet
16 MiB flood took 0.0154 seconds in the precheck, 0.180 seconds in the
unguarded prost decode, and reached about 959 decimal MB RSS (56-fold
amplification). Buffa rejected it with `ElementMemoryLimitExceeded` in
0.0139 seconds at about 54 decimal MB RSS. These are historical parser
observations, not budgets or validation evidence for the current schema.
The complete raw records take precedence over rounded descriptions.

The Linux VM comparison used 4 virtual CPUs and 8 GiB memory on a Ryzen 9
5900X host. The guest reported zero SHA-NI CPU lines with SHA-NI disabled,
and four with it enabled. Only the guest quiet gate ran; the host was not
quiet. Endpoint load averages and times are retained in each `round.json`.
The off round ran from 01:03:25 to 01:56:24; the on round from 01:56:32 to
02:44:43 (UTC), with zero CI jobs at each endpoint.

The corpus was generated with seed 20261101, 2500 requested regular files
and no symlinks. Its second version contained 2549 files and 616092778 bytes.
Both algorithms ran build, verify and chunk-only stages, with one reader,
1 and 4 workers, five repetitions, `cdc-256k`, warm and cold caches, and a
10-percent guest CPU limit. All Linux VM cache states were valid.

| Warm stage / workers | SHA-256 off | BLAKE3 off | SHA-256 on | BLAKE3 on |
|---|---|---|---|---|
| Build / 1 | 157 | 1295 | 834 | 1282 |
| Build / 4 | 569 | 1555 | 1661 | 1567 |
| Verify / 1 | 158 | 1916 | 842 | 1926 |
| Verify / 4 | 607 | 4393 | 2725 | 4533 |

Throughputs are rounded MiB/s. Warm BLAKE3/SHA-256 ratios were respectively
8.24, 2.73, 12.16 and 7.24 with SHA-NI off, and 1.54, 0.94, 2.29 and 1.66
with it on. Cold ratios were 5.90 / 0.98, 1.66 / 0.99, 9.40 / 1.76 and
2.95 / 1.17 (off / on). Chunk-only throughput was about 1462 MiB/s.
A virtual CPU with a masked instruction set is not a physical reference
without SHA extensions. There is no outstanding search for such a reference.

The Windows plausibility round ran from 02:44:50 to 02:48:11, with zero CI
jobs at both endpoints. Its cross-built executable targeted
`x86_64-pc-windows-gnu`, took about 25 seconds to build and was 1940623 bytes.
It was associated with revision `e1af3927ada0` in the accompanying notes;
the executable did not embed a revision. The retained
`hash-windows-plausibility-2026-11-09/quiet-gate-windows.patch` bypassed the
Linux CPU gate on other operating systems. This patch is a historical
measurement artifact and must not be applied to the current tool.

The Windows round used the same corpus and warm-cache parameters. Cache
verification was unavailable (`unknown`); CPU-name, core-count and memory
metadata were absent. Rounded SHA-256/BLAKE3 MiB/s were 614/797 (build, one
worker), 779/773 (build, four), 517/1239 (verify, one), and 1280/1574 (verify,
four). Ratios were 1.30, 0.99, 2.40 and 1.23. Chunk-only throughput was
about 882 MiB/s. There were no Windows cold-cache measurements.

Every VM and Windows raw `decision_grade: true` field is retained for
historical fidelity, but its assessed quality is **informative**. The Linux
VM host was not quiet, the executable revisions were not embedded, and the
Windows gate was bypassed. Documentary revision associations do not prove
binary provenance. Each condition has one round of five repetitions;
shared CPU cores, clocks, memory and caches limit interpretation. An earlier
aborted VM attempt was discarded; these records contain the completed round.
Private execution details and incomplete executable checksum fragments are
omitted. None of these measurements proves current native Windows behavior.
