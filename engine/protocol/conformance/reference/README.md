# Independent protocol reference package

This private Node 24 package checks fixed protocol cases. Its own code and
synthetic cases are 0BSD; foreign data retain the separate notices listed in
`../../NOTICE` and `../../testdata/foreign/SOURCES.json`.

Install from the repository root with the frozen root lock and both exact
engine directory filters in `pnpm-workspace.yaml`. The package has no public
exports, generated build output or installation lifecycle scripts.

The eight commands are `typecheck`, `imports`, `corpus`, `coverage`,
`primitives`, `vectors`, `limits` and `counterexamples`. Their integration
launcher requires an independently supplied `--source-sha`. It forwards
`--out` to the checker commands and `--stage 4` only to the vector selection.
The caller must provide a fresh result directory outside source and data.
The separate container entry exercises the pinned BLAKE3 dependency.
The coverage package command uses that same entry so coverage includes actual
BLAKE3 execution; the source-only command reports those cases as pending.

`MODULES.md` describes the checker modules and `FORMAT.md` describes data,
reports and field-level provenance. Pending independent calculations and
blocked specification conditions remain pending in reports; successful
comparisons alone do not establish complete conformance. Coverage and
behavioral counterexamples require their respective prerequisites.

`expectationsAccepted` records complete value-bound confirmations. A pending
hash execution remains pending even when its fixed expectation is confirmed.
The separate container entry completes the BLAKE3 execution checks.

The resolved import check examines local, command and container entry graphs,
all remaining package sources, and the separate pinned compiler/type graph.
Only `container/run.mjs` may introduce `@noble/hashes/blake3`, and local
entries may not reach container modules. The module policy grants read-only
filesystem operations to `src/loader.ts`
and output operations to `tooling/output.mjs`; no other package module may
import a filesystem module. Named open flags and direct handle operations are
checked separately for each of these two modules, and handles must not escape.
The writer creates exclusive temporary files, synchronizes them and renames
them into place; it cannot truncate an existing file through its open flags.
The loader must reject symlinks at every component,
read only test data, required package documentation and the explicitly listed
source files needed by static self-checks, and exclude installed
packages from tree enumeration. Output requires an explicit destination and
resolved path containment. The caller reviews and tests those runtime path
checks separately from import admission. Dynamic imports, require and runtime
module resolution remain forbidden. The container entry has no Node module
grant; its only external import is the static BLAKE3 dependency. The scanner diagnoses these
violations without modifying checker code or expectations.

Each invocation binds its logs and reports to an independently verified Git
source receipt before and after execution. Import diagnostics remain blocking.
No command updates fixed expectations. The import scanner does not prove
independent authorship; source provenance and independent review remain
necessary.
