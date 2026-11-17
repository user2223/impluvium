# CI checks

These tools require Node 24 and use only its standard library. Checkers read
files and never invoke Cargo, a compiler, a package manager, Git or a shell.
The explicitly named capture and bootstrap helpers start tools in disposable
containers and retain their evidence; they do not approve their own results. Each
entry point accepts `--help`. Exit codes are 0 for success, 1 for a failed check,
and 2 for invalid arguments, unreadable files or malformed/unsupported input.
Diagnostics use `file:line: message`. Entry points also work through symlinked
directory paths.

The tools catch honest mistakes in workflows and Dockerfiles reviewed by
humans; each version of `ci.yml` is approved separately. They are not a sandbox
against deliberate evasion. The design rule is **FAIL CLOSED**: input the
parsers cannot analyse must produce an input error, never pass silently.

Run the self-tests **inside a disposable Node 24 container**, from the root of
an exported source tree without `.git`:

```sh
node --test tools/ci/test/*.test.mjs
```

The fixtures are synthetic. Their domains, addresses and absolute paths use
reserved example values. No installation, network access or build is needed
for the self-tests. Repository walks skip `.git`, `node_modules`, `target`,
`.nuxt`, `.output` and `tools/ci/test/fixtures`.

```sh
node tools/ci/check-pins.mjs --root .
node tools/ci/check-features.mjs --root . --previous-baseline /tmp/base-feature-gates.json
cargo metadata --locked --all-features --format-version=1 > /tmp/metadata.json
pnpm --filter "{./engine/protocol/ts}..." list --depth Infinity --json > /tmp/pnpm-graph.json
node tools/ci/check-licence-boundary.mjs --root . --metadata /tmp/metadata.json --pnpm-graph /tmp/pnpm-graph.json --pnpm-lock pnpm-lock.yaml
```

`check-pins` applies generic checks to every file under `.gitea/workflows/` and
`.github/workflows/`. Only `ci.yml` must have the six required job IDs, without
job names, matrices or job-level conditions. Step conditions are allowed. It
checks workflow actions and containers, Dockerfile images, workflow tool versions/checksums, Cargo
commands and pnpm installs in workflow `run`, Dockerfile `RUN`, and
`package.json` scripts. Deployment templates are excluded. It compares Node 24
across `.node-version`, tagged Node images, Fedora `nodejs24`/`nodejs24-npm`
packages and `engines.node` (required in root `package.json`). Other manifests
may omit engines; any declared Node range must stay within that major. Root
`packageManager` must specify an exact pnpm version and integrity hash.

Docker image defaults belong in global `ARG`s used by `FROM`, or directly in
`FROM`. Identical image literals can recur across workflow containers,
Dockerfile ARGs, FROM, COPY and mounts. Each repository plus tag has one digest;
different tags are independent. Untagged images share one digest per repository.
Actions may reuse an identical commit pin. `COPY --from=SOURCE`
and `RUN --mount=...,from=SOURCE,...` accept an earlier stage name/index, a
pinned image ARG, or a literal image with a SHA256 digest. Shell-form RUN accepts
leading `--mount=`, `--network=` and `--security=` options; unknown options fail.
Mount field names are case insensitive, including `from`; unknown or duplicate
field names, missing `=` on value fields and empty image sources fail with exit 2.
Backslash-newline pairs and intervening blank/comment lines are removed before
instruction options are parsed. A character-to-line map retains physical line
numbers for image and shell diagnostics, including options on later lines.
Other nonempty lines must begin with a
known Dockerfile instruction; `ONBUILD` and `SHELL` are unsupported and fail
explicitly.

Only workflow **top-level** `env` names ending in `_VERSION` or `_SHA256`
establish central tool pins. Step/job variables such as
`IMPLUVIUM_VERSION: ${{ steps.version.outputs.version }}` are ordinary runtime data.
Reusing a central tool name at those levels or assigning it in a shell script
(including `export`, `env` or `declare`) fails. An unchanged pass-through such as
`env BUILDKIT_VERSION="$BUILDKIT_VERSION"` or `env BUILDKIT_VERSION="${BUILDKIT_VERSION}"`
is allowed. Non-Corepack versions need a
matching `_SHA256`; scripts reference these variables instead of repeating
literal values. Canonical Node/pnpm/image/toolchain override names remain
reserved. Image context overrides fail. The one npm exception is
`npm install -g "corepack@${COREPACK_VERSION}"`, which bootstraps the package
manager; application installs must use frozen pnpm locks.

`cargo +CHANNEL` and `rustup run CHANNEL COMMAND` must name a channel from one
of the repository's `rust-toolchain.toml` files. When wiring a nightly
command, update its explicit `+nightly-DATE` whenever the desktop toolchain file
changes, or use that directory as the working directory and omit the override.
The Docker Rust image and `ENV RUSTUP_TOOLCHAIN` must match the root toolchain.

The parsers support the repository's data-oriented TOML and workflow YAML:
quoted strings/keys, comments, single-line flow collections, multiline plain or
quoted values, literal shell blocks and simple folded shell blocks. **Indentless
YAML block sequences and multiline flow collections produce explicit exit-2
input errors**: indent sequence items beneath their key and keep flow
collections on one line. YAML tags, aliases, merge keys, complex folded blocks,
Dockerfile here-documents and exec-form RUN also produce input errors. Use
literal `|` for shell scripts containing blank or indented lines.

The **supported shell subset** is explicit. Shell scanning handles quotes,
continuations, separators, `else`, brace groups,
command substitutions (also inside double quotes), `env`/`sudo`/`command`/`exec`
prefixes with recognised options, `runuser -u USER -- COMMAND`, `rustup run`
and explicit shell `-c` scripts (including absolute shell paths). Wrapper
options that cannot be parsed exactly are not unwrapped.

Workflow step `shell` and job/workflow `defaults.run.shell` accept only `bash`,
`sh`, `bash -e {0}`, `bash --noprofile --norc -eo pipefail {0}` and `sh -e {0}`.
Other values fail with exit 2 (`unsupported shell`). Shell settings themselves
are scanned for tool words, including mentions in comments or arguments.

Every occurrence of a shell word `cargo`, `pnpm`, `npm`, `npx`, `corepack` or
`yarn` in a run script, RUN instruction or package script must be attributable
to a recognised and checked command. This includes occurrences inside quotes,
backticks, substitutions and wrapper arguments. Otherwise exit 2 reports
`cannot verify TOOL invocation at FILE:LINE; write it as a plain command`.
Backticks and unknown wrappers cannot hide tool words.
A plain mention in `echo 'cargo build'` also fails with exit 2; this deliberate
false positive keeps the rule simple. Shell comments are ignored outside
heredoc bodies. This is a static check: write commands and flags plainly.

In any script containing a tool word, a word in command position or tool-action
position containing `$` or backticks is an exit-2 **unsupported construct**, even
when quoted or escaped. For example, `pnpm "$ACTION"`, `pnpm exec "$COMMAND"`
and `cargo fmt; "$COMMAND"` are unsupported. Variable references in ordinary
arguments, including the pinned Corepack bootstrap, remain supported.
Workflow `run` scripts reject `${{ matrix.* }}` and `${{ inputs.* }}` expressions
anywhere, even without a literal tool word. Tool words in any `strategy.matrix`
value also produce exit 2, including nested include/exclude entries and values
unused by a run script. Expressions are never resolved or executed by the checker.

Heredocs with literal delimiters (unquoted, quoted, escaped or mixed quoting,
including `<<-`) are recognized before shell comments are discarded. Any body
mentioning a tool word is exit 2, including `# $(cargo build)` and a quoted
body containing an otherwise locked command. Write those commands directly in
`run`, RUN or a package script instead. Tool-free shell-shaped heredoc bodies
retain pin/override checks; delimiter syntax and bodies the scanner cannot parse
are input errors. Dockerfile heredocs remain unsupported entirely.

Redirections `<`, `>`, `>>`, `<>`, `>|`, `<&` and `>&`, with optional adjacent
numeric descriptors, may appear before or after a command. Operators and their
operands are removed before checking arguments: `cargo build > --locked` and
`pnpm install > --frozen-lockfile` both fail with exit 1. Descriptor duplication
accepts literal numbers or `-`; filename operands may contain quoted expansions.
Here-strings, process substitutions, `&>`/`&>>`, missing operands and unsupported
descriptor operands are exit-2 unsupported constructs. Quoted operators remain
ordinary arguments.

Only `cargo fmt`, `cargo --version`, `cargo -V`, `cargo help` and `cargo version`
are exempt from locking. Every other Cargo invocation, including built-in and
configured aliases, needs `--locked` or `--frozen` before any `--` separator.
The subcommand must follow `cargo`, an optional `+CHANNEL` and optional
`--locked`/`--frozen` flags. Other leading global options are exit 2 because
their operands could hide a dynamic action; place those options after the action.

Package-manager dispatch uses these action allowlists. Unknown actions produce
exit 2; there is no implicit script-name fallback. Actions are classified before
an invocation is marked checked.

| Manager | Install-like (exit 1 unless policy satisfied) | Allowed actions | Download launchers |
| --- | --- | --- | --- |
| pnpm | `install`, `i`: require `--frozen-lockfile` without an override | `run`, `exec`, `help`, `list`, `ls`, `why`, `--version`, `-v`, `--help`, `-h` | `dlx`, `create` |
| npm | `install`, `i`, `ci`: use frozen pnpm instead; only the centrally pinned Corepack bootstrap is exempt | `run`, `run-script`, `help`, `list`, `ls`, `why`, `--version`, `-v`, `--help`, `-h` | `exec`, `x`, `create`, `init` |
| yarn | bare `yarn`, `install`: use frozen pnpm instead (also with `--silent`) | `run`, `exec`, `help`, `list`, `why`, `--version`, `-v`, `--help`, `-h` | `dlx`, `create` |

All three managers reject these lock-changing actions with exit 1, even with a
frozen flag: `update`, `upgrade`, `add`, `remove`, `up`, `dedupe`, `import`,
`uninstall`, `un`, `unlink`, `link`, `rm`, `upgrade-interactive`, `rebuild`,
`patch`, `patch-commit`, `set`, `config`. This list is conservative: configuration
and lifecycle changes belong outside CI too. Other actions are unsupported.
Before the action, the supported options are `--filter`/`-F`, `--dir`/`-C`,
`--prefix` (with an operand), `--filter=`, `--dir=`, `--prefix=`, `-r`,
`--recursive`, `-w`, `--workspace-root` and `--silent`. Unknown prefix options
are input errors. Prefer explicit `pnpm run NAME` or `pnpm exec COMMAND`.

`corepack <package-manager>`, `pnpm exec` and `yarn exec` are unwrapped and
checked recursively, including inner Cargo and Tauri commands. Other supported
Corepack actions are `enable`, `disable`, `pack`, `--version`, `-v`, `--help`,
`-h`. `npx` supports those four version/help flags; otherwise it is a
download launcher. Download launchers (including `npm exec`/`x`) unwrap the inner
command and apply its checks. Packages absent from the root pnpm/npm lockfile
fail with exit 1. `create NAME`/`init NAME` look up `create-NAME` (also for scoped
packages). Merely finding a package in a lock does not verify the binary selected:
that ambiguity or unsupported launcher options produce exit 2. Use a plain
command or `pnpm exec` for local binaries instead.

The repository passes the full check; the `checks` job of `ci.yml` runs it on
every change, together with the self-tests, `check-features`, `check-names`,
`check-env`, the identity document check, `check-identity`, `check-licence-boundary` and `cargo deny`
(configured in `deny.toml`).

`check-features` associates each Rust file with its nearest package manifest,
ignores strings/comments, and checks gate **names**, including nested
`cfg_attr`. `feature-gates.json` permits the existing desktop gates; unlisted
crates permit none. Its format is:

```json
{"version":1,"crates":{"desktop/bridge/Cargo.toml":["existing_gate"]}}
```

An empty `crates` object is valid. Remove unused gates in reviewed edits.
`--previous-baseline` rejects growth of this allow-list.

`check-licence-boundary` requires `--root` naming the repository root and resolved
Cargo metadata, including dependencies and `resolve.nodes`; there is no
manifest-only fallback. Generate it with
`--all-features` and without `--no-deps` or a target filter so optional and
platform dependency edges are available. Walks from every `engine/` package
reject any reachable package inside the repository but outside `engine/`, any
licence expression that has no permitted alternative free of copyleft obligations, and any explicit
`license_file` whose name or content indicates MPL. Conventional crate licence
files and shared engine ancestor notices are
also inspected. Registry crates, renamed workspace dependencies and `[patch]`
redirects use resolved package IDs. Inherited licence fields are already
resolved by Cargo. Capture metadata and check it in the same filesystem, with
explicit licence files (including registry files) readable; missing files or
an incomplete graph are input errors. Empty metadata, empty workspace members,
workspace manifests outside `--root`, and any on-disk engine package manifest
missing from `metadata.packages` are input errors. A virtual `Cargo.toml` with
`[workspace]` and no `[package]` needs no metadata package; a manifest with both
tables does. Selecting no `engine/` packages is always exit 2.
Metadata must cover every engine workspace in one document (merge the packages, workspace members and
resolved nodes of separate captures if necessary). The capture's workspace
root may be `engine/`; `--root` must still name the repository root, at the
same filesystem location used during capture. The project's own material
under `engine/` is 0BSD; third-party material listed in
`engine/protocol/NOTICE` keeps its own license. Dependencies must remain
permissively licensed throughout the dependency graph.

The Cargo and pnpm adapters feed one boundary traversal. SPDX syntax is
parsed before admission: `OR` needs a usable alternative, `AND` needs every
term, parentheses preserve grouping and `WITH` binds an exception to one
license. Cargo's deprecated `/` separator means `OR` with the same precedence;
it never means `AND` or bypasses validation of either operand.
Admission reads `deny.toml` rather than maintaining another allow
list. The engine boundary still forbids MPL/GPL/LGPL/AGPL obligations even
when the inherited workspace allows MPL. For example,
`MIT OR Apache-2.0 OR LGPL-2.1-or-later` passes; LGPL alone or
`MIT AND LGPL-2.1-or-later` fails. Missing, unknown or malformed expressions
are input errors; known unapproved terms simply cannot satisfy an
alternative. An expression without a permitted alternative is a boundary
violation. MPL files remain forbidden independently of
expression alternatives. No target filter is applied to this boundary.

`spdx-license-ids.json` and `spdx-deprecated-ids.json` are unchanged identifier
tables from `spdx-license-ids` 3.0.23 (CC0-1.0,
https://github.com/jslicense/spdx-license-ids). Exception names were checked
against `spdx-exceptions` 2.5.0 and the public SPDX exception vocabulary
(https://spdx.org/licenses/exceptions-index.html). These lexical tables
distinguish unknown names from valid but unapproved alternatives; they do
not authorize any license. Admission still has exactly one source,
`deny.toml`. No third-party parser implementation or license text is copied.

The pnpm adapter requires the installed filesystem and root lock version 9.
It enumerates engine workspace manifests separately, including empty roots,
and matches each importer section, alias/link, installed manifest identity,
snapshot and peer context. Missing installed files or graph/lock edges fail
closed. npm installations under `node_modules` are external packages, while
workspace links retain their repository boundary. The pnpm JSON license
field alone is never used as evidence.

## Independent reference integration

The reference package at `engine/protocol/conformance/reference/` is a private,
non-exported Node 24 tool under 0BSD. Its manifest, explicit workspace member,
root-lock importer and CI installation are one contract. Both installation and
licence-graph capture test each engine manifest separately and use both
`{./engine/protocol/ts}...` and `{./engine/protocol/conformance/reference}...`
with `--fail-if-no-match`. All installed dev/type packages remain in licence scope.
The npm adapter discovers the package; no Cargo tool classification is added.

`reference-boundary.mjs` checks package identity, exact dependencies, the sole
root lock and installed links against the closed runtime/type-tool policy.
`check-reference-imports.mjs` uses the pinned TypeScript AST and type resolver,
plus explicit Node package-condition resolution, to inspect all own executable
files and transitive runtime/declaration imports. It rejects unsupported
module names, product paths, type-tool runtime imports and code-loading
capabilities. The separately inventoried compiler/type graph includes all
compiler inputs, installed edges and file hashes. Unknown resolution fails;
this is a technical import boundary, not an authorship proof or a sandbox.

The runtime policy lists each module's capabilities explicitly. Only
`src/loader.ts` can import the asynchronous filesystem for reading and only
`tooling/output.mjs` for output and path containment metadata. Imports must
use the enumerated named operations, and raw filesystem capabilities may not
escape through exports or aliases. An `open` call must create an owned handle
with the exact named flags for that module, including `O_NOFOLLOW`.
The reader uses constant handles. Only the writer may assign its explicitly
named local handle from an awaited open, reset it to `undefined`, and test it
in an `if` guard for cleanup. Its open flags require exclusive creation and
exclude truncation; its only crypto import is `randomBytes` for temporary names.
Reader handles may only read, inspect metadata and close; writer handles may
only write, synchronize and close. Handles cannot escape their module.
Runtime containment, documentation/data
roots, every-component symlink rejection and installation-directory exclusion
remain requirements of the checker implementation and its tests. Source-text
self-check reads have a separate fixed file list, excluding package metadata.
`test/reference-access.test.mjs` exercises project I/O in isolated copies,
including hardlink preservation, symbolic-file replacement and a deterministic
target-directory change immediately before the real rename operation. Other files
receive no filesystem grant. The only external import in `container/run.mjs`
is static `@noble/hashes/blake3`; dynamic imports and require fail everywhere
in package code. Type/declaration extension substitution resolves a `.ts`
reference to its `.d.ts` when appropriate, preserving missing-file and real
symlink-target errors and examining every reached dependency declaration.

The existing blocking licence gate additionally invokes the exact reference
pin/notice check when that package is present. Noble's name, version and SPDX
expression must match the reviewed `@noble/hashes` 1.8.0 MIT package, and its
regular `LICENSE` file must match the recorded SHA-256. The unchanged MIT
notice in `test/fixtures/noble-hashes-LICENSE` comes from that package; no
library implementation is vendored. Manifest alternatives, missing, changed
and symlinked notices are counterexamples. The generic SPDX traversal of both
engine roots and all compiler/type edges remains in force.

The names ratchet checks inherited product names, and image leak tests check
`.env` exclusion. Neither is a check for caller-specific source vocabulary.
`check-reference-publication.mjs --root DIR --policy FILE` supplies that
separate source admission check. The nonempty literal-term policy must be
supplied outside the public source tree; it is not part of the package or CI
secrets. The caller runs this gate before accepting source for publication
and keeps its policy and diagnostics separately. Visible source, filenames
and decoded JSON strings are checked with no word-list-definition exception.
Only the exact package installation directory is excluded; symlinks and
unreadable text fail. The normal publication/privacy checks outside CI remain
required. This generic scanner is not a proof of independent authorship.

`reference-import-cases.mjs` tests direct and aliased type imports, reexports,
import-type expressions, dynamic paths, literal dynamic product imports,
escaped require, filesystem access, symlink targets and transitive package
runtime/type exports. It first proves a positive baseline, then requires the
specific boundary diagnostic for each mutation. An unchanged/comment-only
control survives. Fixture modules are analyzed, never executed. The full AST
probes run with the installed compiler in the disposable container. Standard
module fixture tests cover manifests, lock/link drift, licence inclusion and
the exact Source exception; they do not require installed compiler code.

The workflow contract has 27 required execution blocks, including the three
blocking steps `Reference types and imports`, `Reference conformance` and
`Reference counterexamples`. It also binds the existing pinned `always()`
artifact upload to `/work/results/reference/**`, with no excluding patterns.
`reference-run.mjs` checks an independent Git-object source receipt before and
after each invocation, requires an independent `--source-sha`, records its source SHA and logs, and rejects missing
entry files and empty/skipped test runs. The launcher passes `--out` and vector selection to the checker command,
consumes `--source-sha` itself, and validates JSON reports with actual case IDs.
Pending calculations remain visible. The separate container entry executes
the BLAKE3 checks. Import findings and incomplete coverage remain blocking. No conditional success path is installed for that
intermediate state. The six existing job IDs and all prior gates remain.

`reference-checks.md` lists import, workflow, product and report counterexamples
with their expected diagnostics. Dependency-free import admission tests use
Node's built-in syntax parser and the production resolver; the compiler AST
extraction and installed declarations still require the pinned tool graph.
Known forbidden targets appear as resolved paths in diagnostics. Unknown
computed expressions fail without claiming a resolved target.

The required pins block also invokes `reference-products.mjs`: final Docker context
exclusions, Node manifests/lock aliases, direct product-source references and
client resource/frontend roots keep validation inputs outside runtime products.
This source check complements real image/client artifact checks.

Expectation acceptance means that all value-bound confirmation evidence is
complete; it is separate from whether each hash execution has run. Pending
hash results remain pending. Counterexample reports require twelve executed
baseline witnesses, fifteen detected harmful mutations and four surviving
harmless controls, with matching operation traces, errors and summary counts.
Container reports require completed hash execution and BLAKE3 evidence.

## Protocol evidence and tools

`protocol-tools.mjs --root DIR --work DIR --kind codegen|windows` prepares
tools in a disposable container. Codegen pins and measured frozen locks
come from the workflow and `codegen-tools.json`. Windows pins come only
from `versions.env`. Archive verification precedes extraction or execution;
tool-lock comparison precedes fetch and offline build. Failure-order tests
exercise those operations without downloading or compiling tools.

`capture-protocol-graph.mjs --root DIR --contract FILE --profile NAME
--target TARGET --run build|test|clippy --source-sha SHA --source-receipt FILE --out DIR` starts
the actual calls from the contract's four package/feature selections.
Cross-builds additionally supply `--windows-tools DIR` containing the
verified installation receipt. The output and target directory must be
fresh. Capture sets and records `CC_ENABLE_DEBUG_OUTPUT=1` itself.

Each completed output directory contains `invocation.json`, `metadata.json`,
`packages.tree`, `graph.json`, `cargo.stdout`, `cargo.jsonl`, `build.log`,
`tools.json` and `diagnostics.json`. The graph and invocation use
schema version 2, with representation `package-features`; the policy encoding
and workspace report use version 3. The package call is
`cargo tree ... --no-dedupe --prefix none --edges normal,build --format '{p}|{f}'`;
tests and Clippy additionally include `dev` edges. Package, feature and target
arguments come from the same structured selection as the actual build.
There is no feature-pseudonode tree or depth limit.

Each normalized row contains an opaque package ID and its sorted active
feature set. Exact repeats, including `(*)` display markers, collapse to one
row; two feature sets for one package remain separate rows and are never
unioned. Build dependencies and proc macros remain included. Metadata is an
unfiltered identity catalogue, so Linux-host dependencies are available even
for Windows builds. Neither metadata features nor metadata dependency edges
are used to reconstruct profile resolution. Cargo artifacts must corroborate
every observed package/feature row, and every artifact must match a row.
Required runtime packages/features must also have target artifacts.

This profile check does not reconstruct dependency aliases, individual edges,
target conditions on edges, feature-activation paths, or a host/target label
for every list row. Moving an existing edge without changing the package and
feature sets is not detected. Build contexts still come from actual artifacts
and compiler/linker records. The separate workspace check retains full
versioned paths for inherited exceptions; the licence boundary retains its
own graph traversal. Runtime TLS/trust routing requires its own tests.

Child stdout streams
directly to files with backpressure, a 2 GiB ceiling per file and a SHA-256;
stderr is retained separately with a 16 MiB ceiling. Both capture and checker
read the package list and hash evidence in bounded chunks. Normalization keeps
only unique package/feature rows. Exceeding a limit fails rather than truncating
evidence.

`capture-status.json` records running, failed or complete status. Failures
retain partial raw evidence for diagnosis; a candidate is written only after
the actual Cargo run and tool attribution finish without diagnostics. List
analysis never prevents the actual build and recorders from running. Independent
package-row, Cargo-stream, command-display and recorder-JSON errors accumulate
in `diagnostics.json`, with the raw filename, line numbers, text and reason.
Identical failures retain all their source lines. Failed prerequisites that
would make tool execution unsafe still stop before execution. The checker
rejects an incomplete capture or nonempty diagnostics. Retry with a fresh results directory, retaining the failed
one separately; never reuse or approve its partial files.

The capture preserves separate stdout/stderr, including C commands emitted
on Cargo stdout before `build-finished`, and derives a separate
`tests.txt` with paired suite headers for the existing test counter.
Multiline Cargo command displays and shell-escaped punctuation are parsed
as data, never evaluated. Expected failed compiler capability probes are
retained; only successful compile/archive calls establish a C build.
`record-linker.mjs` is copied into the results directory with a per-run
configuration. Cargo's linker setting points to that launcher, which
delegates to the previously resolved compiler driver and records the actual
arguments, directory, status and executable in `linker.jsonl`. Both source
and delegation are checked. `record-compiler.mjs` similarly records actual
C/C++ compiler and archiver invocations in `compiler.jsonl`, including their
real working directory and originating Cargo package. The original target-
selecting invocation path and the resolved binary path are both retained.
Delegation never replaces a prefixed llvm-mingw driver with an unprefixed
Clang invocation. Rust command displays do not prove a working directory:
their `cwd` stays null, with the parent Cargo working directory recorded
separately. `CARGO_MANIFEST_DIR` is not treated as a working-directory
observation. Launcher calls remain in the unabridged build-script
output with `CC_ENABLE_DEBUG_OUTPUT=1`; the checker correlates them with the
actual delegated commands. Missing compile/archive calls for built
C code and foreign tool paths fail, while unused tools are explicit.
Product capture rejects protocol generators and `protoc` on PATH. Compiler
recorders read Cargo's `OUT_DIR` only to attribute C build context; protocol
generation still uses checked-in sources and never reads it.

Linker context records the Cargo-selected delegation, never a substring in
an output path. Cross builds select separate host and target launchers;
their configured roles, launcher paths and actual drivers must agree.
For native Linux, host and target share the same verified driver and Cargo
linker setting, recorded as the target delegation. This field does not
distinguish native host compilation units from native target units.
Custom build programs are accepted only by their exact Cargo artifact
filename, package ID and matching metadata `custom-build` target; their
names need not be `build-script-build`.

`check-protocol-graph.mjs --root DIR --contract FILE --source-sha SHA --input DIR` only reads
evidence. It replays normalization, checks raw hashes, selections, locks,
artifacts and effective environment. Each observed name/version/source/feature-set
row must belong to the reviewed permission set for that profile, target and run;
extra reviewed rows need not be observed. A changed feature set needs its own
permission row, not a union with another row. Windows additionally needs the shared archive pin,
verified tool bytes and actual target/static-CRT evidence. Engine product
roots never receive legacy exceptions.

`permission-candidate.json` is a capture result for review, not an automatic
update. `protocol-graph.json.permissions` is populated only from reviewed captures; every missing or
incomplete entry fails. Review each Linux build/test/Clippy selection and
each Windows build selection against the manifests and provider rules
before adding the corresponding candidate. Compression, system proxy,
hyper-rustls trust-source features, native TLS and a second provider remain
forbidden even if someone adds them to a permission entry. In particular,
rustls-native-certs is not banned as a package when the client uses the
platform verifier; the flat list does not prove its incoming edges.

`capture-protocol-workspace.mjs --root DIR --source-sha SHA --base-sha SHA
--contract FILE --source-receipt FILE --out DIR` separately captures complete all-feature workspace metadata for
all targets conservatively and enumerates every inherited forbidden path. First run it
on the actual public base export, review `legacy-candidate.json` and freeze its
SHA and exact exceptions in `protocol-graph.json.legacy`. Each exception
includes the root, full versioned path, target, edges and removal owner.
The current workspace is compared using `check-protocol-graph.mjs
--root DIR --contract FILE --source-sha SHA --workspace DIR/workspace.json`. Empty pending
legacy data fails; current observations never authorize their own paths.
Only a capture whose source SHA equals the supplied base SHA emits
`legacy-candidate.json`. A current capture emits `workspace.json` with both
SHAs for comparison, without proposing baseline exceptions.

The matrix driver uses the fixed four-profile selection, verifies each complete
capture, applies its separate test or Clippy ratchet, then deletes only that
selection's target tree. It retains raw evidence and processes all 16 selections
even when some fail; any failure makes the driver exit nonzero:

```sh
node tools/ci/protocol-matrix.mjs --root /work/repo \
  --contract /work/repo/tools/ci/protocol-graph.json --source-sha "$SOURCE_SHA" \
  --source-receipt /work/source.json --windows-tools /work/llvm-mingw \
  --out /work/results/profiles --previous-baselines /work/previous-baselines \
  --previous-comparison /work/comparison/source.json \
  --previous-source-receipt /work/base-source.json
```

An explicitly supplied comparison directory must exist and be readable. The
existing `protocol-base.mjs` comparison record binds its event base to the
current source SHA. Each copied baseline must match the base source receipt's
Git blob and SHA-256. A missing copy fails whenever that path is in the attested
catalogue; only its proven absence permits a first run without that baseline.
An explicit no-comparison event retains its recorded reason and cannot reuse
stale baseline copies. CI prepares this evidence in the required baseline step.
For source exports without Git, reuse the same comparison record format and
the base receipt produced with `--commit-object`; the matrix needs no Git.

The workflow checker binds each required step to its job, execution conditions,
environment, container definition, triggers, shell, order and exact run text.
Only line endings and surrounding whitespace are normalized. Matrix control
flow is tested in the driver, not inferred from
shell fragments. For contract updates, use the attestation and rebuild procedure
below; all product builds need a generator-free PATH.

The contract fixes required steps, environments, triggers and job dependencies.
Additional steps remain trusted code: the contract does not isolate their
effects on tools and the working tree. Workflow changes therefore require
review of any replacement or modification of Git, generators and checkers.

The required drift block first reads the event commit's Git objects and verifies
the working source bytes with `export-protocol-source.mjs`, then invokes
`check-drift`. This comparison ignores index flags and also rejects untracked
outputs, including ignored files. The existing exact exceptions cover the
checkout-local Cargo and filtered pnpm outputs; generated protocol trees remain
fully checked. This CI block requires Git. In a source export without Git,
verify the original source receipt immediately before `check-drift` with
`verifySource(root, receipt, expectedCommit)` from `protocol-source.mjs` instead;
it also rejects changed tracked bytes and extra files in generated trees.

### Feature closure of the pinned dependencies

The following table documents the reviewed feature closure of the pinned
manifests, not an authorization to reuse stale captures. The versioned
contract must be regenerated when source, evidence or resolution rules change.

The following sets apply to both targets and all three Linux runs unless
explicitly qualified. A dash means absent, and `[]` means an empty feature set.
Windows has build captures only. Version numbers match `Cargo.lock`;
`buffa`, `buffa-types` and `connectrpc` also match `locked-versions.json`.

| Package | Version | Core | Server | Client | Combined |
|---|---|---|---|---|---|
| buffa | 0.9.2 | `json,std,text` | `fast-utf8,json,std,text` | `fast-utf8,json,std,text` | `fast-utf8,json,std,text` |
| buffa-types | 0.9.2 | `json,std` | `json,std` | `json,std` | `json,std` |
| sha2 | 0.11.1 | `[]` | `[]` | `[]` | `[]` |
| blake3 | 1.8.7 | `std` | `std` | `std` | `std` |
| connectrpc | 0.9.1 | — | `axum,json` | `client,json` | `axum,client,json` |
| hyper-rustls | 0.27.10 | — | — | `aws-lc-rs,http1,http2,tls12` | `aws-lc-rs,http1,http2,tls12` |
| reqwest | 0.13.5 | — | — | `__rustls,__rustls-aws-lc-rs,__tls,http2,json,rustls,stream` | `__rustls,__rustls-aws-lc-rs,__tls,http2,json,rustls,stream` |
| rustls | 0.23.46 | — | — | `aws-lc-rs,aws_lc_rs,std,tls12` | `aws-lc-rs,aws_lc_rs,std,tls12` |
| aws-lc-rs | 1.19.0 | — | — | `aws-lc-sys,prebuilt-nasm` | `aws-lc-sys,prebuilt-nasm` |
| aws-lc-sys | 0.47.0 | — | — | `prebuilt-nasm` | `prebuilt-nasm` |
| hyper-util | 0.1.21 | — | — | `client,client-legacy,client-proxy,default,http1,http2,tokio` | `client,client-legacy,client-proxy,default,http1,http2,tokio` |
| rustls-platform-verifier | 0.7.1 | — | — | `[]` | `[]` |
| hyper | 1.12.0 | — | `default,http2` | `client,default,http1,http2` | `client,default,http1,http2` |
| tokio | 1.54.1 | — | `bytes,default,io-util,macros,rt,sync,time,tokio-macros` | `bytes,default,fs,io-util,libc,macros,mio,net,rt,socket2,sync,time,tokio-macros` | `bytes,default,fs,io-util,libc,macros,mio,net,rt,socket2,sync,time,tokio-macros` |
| tokio-rustls | 0.26.6 | — | — | `aws-lc-rs,aws_lc_rs,tls12` | `aws-lc-rs,aws_lc_rs,tls12` |
| rustls-webpki | 0.103.16 | — | — | `alloc,aws-lc-rs,std` | `alloc,aws-lc-rs,std` |

For Windows, client and combined add `tokio/windows-sys` to the Tokio set
above. Their Linux-only `rustls-native-certs 0.8.5` and
`openssl-probe 0.2.2` (both with no features) are replaced by
`windows-link 0.2.1` and `windows-sys 0.61.2`. The latter carries the Windows
API feature closure for Tokio and the platform verifier, recorded exactly
in the permission rows. Server has `libc 0.2.191/default,std` on Linux only.
Core sets are identical across targets; all Linux run sets are identical
within their profile.

The resolved sets contain these additions to the explicitly requested
manifest features, which were reviewed rather than silently discarded:

- `buffa/text` is required by `buffa-types 0.9.2`;
  `buffa/fast-utf8` is required by `connectrpc 0.9.1` when present.
- `reqwest/__rustls,__rustls-aws-lc-rs,__tls` are the pinned crate's
  `rustls` feature closure. Thus its resolved set has seven features rather
  than just the four requested public features. No unrelated public feature
  was added. `rustls/aws_lc_rs` is the pinned crate's alias behind
  `aws-lc-rs`, not an additional provider. `hyper-rustls` has exactly the
  requested four features without additions.
- The resulting `aws-lc-rs/aws-lc-sys,prebuilt-nasm` and
  `aws-lc-sys/prebuilt-nasm` are forwarded by rustls' provider feature.
  They introduce no FIPS or bindgen profile. Resolved CMake dependencies
  do not establish an actual CMake invocation; actual tool evidence remains
  mandatory independently of permissions.
- `hyper-util/client,client-proxy,default` extend its direct selection through
  reqwest and feature forwarding. `client-proxy` supports explicit proxies;
  `reqwest/system-proxy` would enable `client-proxy-system`, which is absent.
  The pinned Hyper, hyper-util and Tokio `default` features are empty.
  Hyper and Tokio have no separately enumerated exact feature limit in
  this contract's product selection; their observed closures are frozen
  above and in the permission rows. No `full` feature is enabled.
- Linux certificate discovery comes from the pinned platform verifier.
  `openssl-probe` is not the forbidden `openssl` or `openssl-sys` TLS stack;
  hyper-rustls has no native-certificate or bundled-root feature enabled.

These explanations refer to the named versions' published Cargo manifests
(`[features]` and the corresponding dependency declarations). They approve
only the exact observed rows, not future aliases or extra features.
The package scaffold does not yet implement the later runtime `no_proxy`
requirement or HTTP/TLS policy; package permissions do not claim that behavior.

### Source attestations and complete workspace coverage

Create source receipts independently from Git objects before any capture:

```sh
node tools/ci/export-protocol-source.mjs --repository CHECKOUT --commit SOURCE_SHA \
  --receipt /work/source.json
node tools/ci/export-protocol-source.mjs --repository CHECKOUT --commit PUBLIC_BASE_SHA \
  --out /work/base --receipt /work/base-source.json
```

The first form verifies the checkout; the second creates a fresh export.
Both read the commit object and its complete tracked file catalogue directly
from Git. CI uses this form.

For source exports without a Git repository, supply `--commit-object FILE`.
Here `--repository DIR` names the untouched source export. The file must contain
the raw commit payload emitted by `git cat-file commit SHA`, and must be kept
outside the export, along with receipts and output directories:

```sh
node tools/ci/export-protocol-source.mjs --repository /work/source --commit "$SOURCE_SHA" \
  --commit-object /work/commit.raw --out /work/build --receipt /work/source.json
node tools/ci/export-protocol-source.mjs --repository /work/base-input --commit "$BASE_SHA" \
  --commit-object /work/base-commit.raw --out /work/base --receipt /work/base-source.json
```

This form reads **every file** before installation or building, with no
exclusions for build outputs, installed dependencies or hidden files. It
rejects symlinks and special files. Empty directories are ignored because
Git does not record them. Only the owner's executable bit selects mode
`100644` or `100755`; archive permission differences such as `0664` versus
`0644`, or `0775` versus `0755`, do not change the catalogue.
The reconstructed tree must match the tree in the supplied commit object,
and the SHA-1 of that Git commit object must match the full requested commit ID.
Both forms produce byte-identical receipts for the same source commit.
Keep the original export pristine; install dependencies and build in the
fresh `/work/build` copy, using the same receipt for its subsequent capture.

Receipts include blob IDs, SHA-256 hashes and file modes. Capture
and checker reconstruct the Git tree, verify the commit object ID and compare
all repository source bytes, even without `.git`. Missing, changed or extra
source inputs fail. Extra-file discovery exempts only the exact root-relative
output directories `target`, `desktop/src-tauri/target`, `node_modules`,
`engine/protocol/ts/node_modules`, `engine/protocol/ts/dist`,
`engine/protocol/conformance/reference/node_modules`, and
`torrential/src/proto`, plus the root Git administration entry. Every tracked
file remains mandatory. These paths cover Cargo output, the filtered pnpm
installation, the TS build and the inherited protocol build script. Identical
names below Rust source or test directories are never exceptions. Engine generated
sources are tracked inputs and are never exempted. Receipts and results
must be outside source exports. No capture
may generate its own receipt from the current directory and merely label it
with a caller-supplied SHA.

Pass `--source-receipt /work/source.json` to each product capture. Its source
proof is embedded in `invocation.json` and replayed by the read-only checker.
Both checker modes require an independently supplied `--source-sha SHA`;
a receipt for another commit fails even if its tree is identical.
Pass `--contract FILE --source-receipt FILE` to workspace capture too.
Workspace scope is `all-targets-conservative`: one **unfiltered**
`cargo metadata --locked --offline --all-features --format-version=1`
retains every resolved edge and its target condition. Conditions are data,
not evaluated on the build host. In particular an outer Windows build edge
and an inner Unix host edge remain one complete path. These conservative
paths have target `all`; they are not claims that every such path executes
on every platform. Older platform-filtered exceptions are invalid.

Engine manifests are inventoried from the complete attested file catalogue.
The licence boundary also inventories all real Engine directories without
name-based exclusions. An independent workspace cannot evade coverage under
a directory named `target` or `dist`.
Every local Engine package is classified by exact manifest path in
`enginePackages`. Products must be selected by a reviewed profile. The separate tool classifications are `impluvium-protocol-xtask` at
`engine/protocol/xtask/Cargo.toml` and `impluvium-hashchunk-bench` at
`engine/protocol/benchmarks/hashchunk/Cargo.toml`. Each requires its exact
name/path pair. Unknown Engine
packages fail instead of silently inheriting the workspace's legacy exception.
Every selected product root also needs a matching **target** artifact,
including roots with no enabled features.

Local inherited workspace packages also freeze their **declared** dependency
requirements: package name, alias, kind, target condition, optionality, defaults
and requested features. Their entire local feature tables are frozen too, so
indirect activation through a new default or local feature cannot bypass the
ratchet. This deliberately covers all inherited declarations, avoiding another
reachability classification. Engine products use the profile rules; the explicitly classified tools use
their separate tests and Clippy ratchets. The inherited requirement table is
unchanged by adding a tool.
Removing requests or restricting features, defaults and optionality is allowed;
new declarations, conditions or broader requests fail with the affected root
and declaration. Review the change and independently recapture the selected
public base before updating its requirements. Workspace-wide feature unification
cannot conceal these changes. The unfiltered full-path check still applies.

### Hash and chunk benchmark tool

The Rust job runs the tool's small functional tests and Clippy after fetching
locked dependencies. It does not download corpora, run the million-file model,
perform decision-quality throughput measurements or add a fifth product profile.
Source attribution and the size-model boundary are documented in
`engine/protocol/benchmarks/hashchunk/PROVENANCE.md`.

The test target is named `hashchunk_integration` so Cargo metadata can attribute
its suites without confusing it with other workspace integration targets.
Capture stdout and stderr together for tests, and Cargo JSON stdout separately
for Clippy. Retain both logs in the protocol evidence artifact:

```sh
cargo test --locked --offline -p impluvium-hashchunk-bench --no-fail-fast \
  > /work/results/hashchunk-tests.txt 2>&1
cargo clippy --locked --offline -p impluvium-hashchunk-bench \
  --all-targets --message-format=json -- --cap-lints warn \
  > /work/results/hashchunk-clippy.jsonl
cargo metadata --locked --offline --format-version=1 > /work/results/hashchunk-metadata.json
node tools/ci/count-tests.mjs --baseline tools/ci/protocol-tests-hashchunk.json \
  --metadata /work/results/hashchunk-metadata.json --input /work/results/hashchunk-tests.txt
node tools/ci/clippy-ratchet.mjs --baseline tools/ci/clippy-baseline.json \
  --metadata /work/results/hashchunk-metadata.json --input /work/results/hashchunk-clippy.jsonl \
  --package impluvium-hashchunk-bench --package impluvium-protocol
```

Supply `--previous-baseline FILE` with the respective baseline from the actual
comparison base whenever present. The Clippy stream includes the protocol
workspace dependency: both packages must be selected and both artifacts must
be present. Both budgets are zero; the existing protocol baseline is preserved.

An unmeasured test count is represented by `tests.exact: null`, which the
existing counter rejects as malformed input. Replace it only with the successful
non-ignored count from the complete package log, never by counting test source
annotations. Binary-only doc tests have a floor of zero.

The base-revision retrieval loop includes `protocol-tests-hashchunk`. Its dynamic
profile-path check applies only to `core`, `server`, `client` and `combined`.
Each tool baseline is checked against its concrete path in the actual base
workflow. Absence before introduction is allowed; a used but missing baseline
fails. Exercise both the introduction against the preceding public revision
and the later missing-file case in a disposable checkout. Keep the entire shell
block and the literal workflow contract synchronized.

Adding the workspace member requires lock resolution in a disposable container.
Use `cargo update --workspace` from the root with the pinned stable toolchain,
then `cargo fetch --locked`; review that only the new member and necessary
resolution were added. Do not replace the root lock with the experimental lock.
Recapture all 16 existing product selections and both complete workspaces with
their independent source receipts, then review the writer's contract candidate.
The tool classification alone does not refresh any measured graph data.

The Rust bootstrap also installs Python. The mandatory
`Corpus loader, result data and tree digest tests` block runs standard-library
loader and publication-data tests without network, builds the locked tool and
compares Rust/Python digests of a small generated two-version tree and an
executable single-file fixture:

```sh
python3 -B -m unittest discover \
  -s engine/protocol/benchmarks/hashchunk/corpus -p 'test_*.py' -v
cargo build --locked --offline -p impluvium-hashchunk-bench
python3 -B engine/protocol/benchmarks/hashchunk/corpus/compare_tree_digest.py \
  --tool target/debug/chunkbench
```

The loader tests use local fixtures and mock redirect responses, not public
servers. Publication checks require exact index/file agreement, all sixteen
historical hash files, repetitions, mandatory measurement metadata, distinct
model IDs and independent checksum/tree/signature states. Static checks reject
archives, binary bytes, long encoded blocks and private host-path shapes in
published data and documentation. They supplement the publication guard; they
do not authorize publication. Retain `hashchunk-loader-tests.txt` and
`hashchunk-digest-comparison.txt` alongside the Rust logs. The complete run text,
order and blocking conditions are frozen in `protocol-workflow.mjs`; its existing
mutation tests also cover this block. No test or Clippy baseline, lock, generated
output, workspace member or measured graph data changes for the materials.

### Rebuilding the compact contract

The contract starts in a deliberately incomplete state after an evidence
format change; empty permissions and null legacy data fail closed. A null
`legacy.requirements` specifically requires new base/current workspace captures;
it does not invalidate unchanged product permission sets. Capture
all 16 profile/target/run combinations with source receipts, then capture
the actual **publicly reachable comparison base** and the current source
with the conservative workspace mode. Use the full base commit ID for both
SHA options in its base capture; the current capture uses that same base ID.
The baseline identifies a publicly reachable commit, not only a tree.
If the base is not yet published or merged, keep the baseline pending.

After reviewing the complete candidates and the new path set, write a fresh
review candidate deterministically:

```sh
node tools/ci/write-protocol-contract.mjs --root /work/repo \
  --contract /work/repo/tools/ci/protocol-graph.json --captures /work/results/profiles \
  --source-sha "$SOURCE_SHA" --base-sha "$BASE_SHA" \
  --base /work/base-results/workspace.json --current /work/current-results/workspace.json \
  --out /work/protocol-graph.review.json
```

The capture directory layout is `PROFILE/TARGET/RUN`. Keep exports, source
receipts, raw metadata and the verified tool installation until validation
finishes. The writer validates every complete capture and both workspace
reports, rejects forbidden features independently of permissions and writes
only the fresh review file. Copy it into the repository only after review,
then capture and check the final source again. After a rebase, capture the
new public base independently; compare full paths before updating its commit
ID and baseline. Never authorize changed paths merely by relabelling a SHA.

Encoding version 3 interns package identities, complete package/feature tuples
and identical permission sets. Declared requirement payloads are interned and
referenced by root; their expansion preserves complete declarations and local
feature entries. Legacy paths use parent-indexed prefix nodes
with explicit terminal records. Expanding a terminal reconstructs exactly
one complete versioned path with all edge aliases, kinds and conditions;
it cannot recombine allowed edges into new paths. A missing reference or a
cycle fails. Captures still emit ordinary readable candidate objects.
No host paths, timestamps, tool output or source receipts enter the public
contract.

Windows preparation inventories the archive's executable and loadable-library
trees, including alias links, `clang-target-wrapper.sh`, `clang -> clang-23`
and `ld.lld -> lld`. Capture and checker verify that inventory, wrapper bytes,
inner binaries and the centrally pinned Clang version. Ambient compiler cache,
search-path and loader overrides are rejected. The recorder proves the
selected driver invocation; inventory verification binds its inner tools.

### Offline feature counterexamples

Five separate copies exercise actual Cargo resolution, complete capture,
artifact/tool validation and policy rejection. They add exactly these features
while retaining the existing default-feature and provider choices:

| Case | Manifest | Dependency feature added | Profile | Required rejection |
|---|---|---|---|---|
| gzip | engine/crates/protocol/Cargo.toml | connectrpc/gzip | server | forbidden feature connectrpc/gzip |
| zstd | engine/crates/protocol/Cargo.toml | connectrpc/zstd | server | forbidden feature connectrpc/zstd |
| system-proxy | engine/crates/client/Cargo.toml | reqwest/system-proxy | client | forbidden feature reqwest/system-proxy |
| native-tls | engine/crates/client/Cargo.toml | reqwest/native-tls | client | forbidden package native-tls |
| ring | engine/crates/client/Cargo.toml | rustls/ring, keeping aws-lc-rs | client | forbidden feature rustls/ring |

The mandatory Rust job fetches only the product lock with `cargo fetch --locked`.
Each counterexample starts from a separate attested source copy and adds its
named manifest feature. Cargo derives its lock with
`cargo metadata --offline --format-version=1`, once without `--locked`.
The result must retain **exactly** the product lock's package set: every name,
version, source and checksum. Only dependency edges may change; other lock
data must also remain unchanged. A difference fails with
`a reviewed prepared lock is required for this change`; no fallback is run.
No separate negative locks are checked in or fetched. The native-TLS and zstd
build prerequisites come from the Rust bootstrap.

After this comparison, all capture and build commands use `--locked --offline`.
The exporter proves the original Git contents plus exactly the named manifest
edit and a package-preserving derived lock. Its receipt binds the original
product lock to Git and freezes the derived bytes by SHA-256; capture and
checker repeat both the source and lock-data validation. Even a byte-identical
lock is valid: `system-proxy` and `ring` are rejected from the resolved feature
sets, corroborated by actual Cargo artifacts, rather than from lock changes.
Ordinary captures reject these explicit mutation receipts. Negative captures
cannot create baseline exceptions or populate product permissions. A failed
resolution, missing crate, failed build or generic incomplete-capture error
**does not count** as a successful counterexample. Every capture must complete
and its checker must report the exact structured diagnostic code and package/
feature fields above. File paths and human-readable messages are not matching
inputs. All five prescribed case names and five completed rejections are required.
The suite retains `results.json`, each `rejection.json`, receipts and raw
capture evidence. It deletes only a successfully evaluated Target tree.

For an untouched source export without Git, check accepts the same
`--commit-object` option. Keep the original export pristine; the cases modify
only their newly created copies:

```sh
node tools/ci/protocol-negative.mjs --mode check --repository /work/source \
  --commit "$SOURCE_SHA" --commit-object /work/commit.raw \
  --contract /work/source/tools/ci/protocol-graph.json --out /work/negative-results
```

The six `protocol-tests-*.json` files contain measured container results.
Existing counters reject `null` placeholders if a new baseline is needed.
Clippy budgets for new crates are zero by policy. Keep one complete stream and baseline per
selection, with actual previous baselines from the event base. Initial
placeholder replacement is not permission to relax a previous baseline.

Both diagnostic ratchets use a nonempty package-name-to-count baseline:

```json
{"version":1,"packages":{"sample":3,"other-package":0}}
```

Counts must be nonnegative safe integers and must match **exactly**. A higher
count fails. A lower count also fails, with `lower the baseline for <pkg> from
X to Y in this change`, so improvements are locked in by the same PR. Missing
counts fail; zero must be observed. Neither tool writes baselines. Budgets are
per package. Capture counts in the build container before adding production
baselines; supplied diagnostic baselines are fixtures.

CI must pass `--previous-baseline FILE` from the **base revision** to
`check-features`, `clippy-ratchet`, `tsc-ratchet` and `count-tests`. Diagnostic
limits may only fall; test minimums/exact counts may only rise. New diagnostic
entries are allowed only with budget 0; new test-count entries are always
allowed. The comparison includes entries outside the invocation's `--package`
selection. Initial baseline creation requires an explicit wiring/policy
change; silently replacing the base file with the PR's
file defeats the ratchet. Omitting the option only checks the current baseline.
For example, CI can read files at its verified PR base SHA before exporting the
source tree, then provide the captured files to the disposable container:

```sh
baseline_dir=$(mktemp -d)
git show "${BASE_SHA}:tools/ci/feature-gates.json" > "$baseline_dir/features.json"
git show "${BASE_SHA}:tools/ci/clippy-baseline.json" > "$baseline_dir/clippy.json"
git show "${BASE_SHA}:tools/ci/tsc-baseline.json" > "$baseline_dir/tsc.json"
git show "${BASE_SHA}:tools/ci/test-baseline.json" > "$baseline_dir/tests.json"
```

Copy those captures into the build container as `/tmp/base-feature-gates.json`,
`/tmp/base-clippy.json`, `/tmp/base-tsc.json` and `/tmp/base-tests.json` for the
commands shown here. Do not fall back to the current file if a base file is
absent or unreadable.

The `rust` job of `ci.yml` uses `tools/ci/clippy-baseline.json` (stable and
nightly Clippy, selected with `--package`) and `tools/ci/test-baseline.json`
(exact counts). It reads their copies from the target branch of a pull request
or from the previous head of a push. Only a base revision whose `ci.yml` does
not mention a baseline file yet may lack it (the change that introduces the
file); otherwise the job fails. A pull request or branch push without a base
revision also fails; only a new branch, a tag push and a manual run check the
current baselines alone. To measure counts, run the job's steps in its
container image: the tools print the observed count of every selected package,
also when the check fails.

The `desktop-ui` job uses `tools/ci/tsc-baseline.json` (package `impluvium-desktop-ui`, the Nuxt
typecheck of `desktop/main`) and reads its base copy the same way.

For Clippy, capture **stdout only**, preserving JSON lines. Metadata must include
`workspace_members` and dependencies; exact package IDs are joined against it,
so path basenames and Rust target names need not equal package names.

```sh
cargo metadata --locked --all-features --format-version=1 > /tmp/metadata.json
cargo clippy --locked --all-targets --message-format=json -- --cap-lints warn > /tmp/clippy.jsonl
node tools/ci/clippy-ratchet.mjs --baseline tools/ci/clippy-baseline.json --previous-baseline /tmp/base-clippy.json --metadata /tmp/metadata.json --input /tmp/clippy.jsonl
```

Only `clippy::` diagnostics count, including cached diagnostics; compiler errors
and unsuccessful builds fail independently. Each selected package needs an
artifact, even at zero, and the stream needs exactly one successful terminal
`build-finished`. Any following nonblank record, including another completion,
is exit 2; missing completion is also exit 2. Trailing blank lines are allowed.
Every workspace member producing diagnostics/artifacts needs a baseline entry. `--package NAME`
scopes the stable/nightly runs; observed workspace members outside that
selection fail unless explicitly excluded with `--exclude NAME`. Registry
dependencies do not need workspace baselines. One diagnostic emitted for two
targets counts twice; keep Clippy flags consistent.

For TypeScript, capture one package's combined output and its original status.
Unformatted tsc/vue-tsc output is preferred; pretty diagnostics and ANSI colour
are accepted. Run the package typecheck command after preparation:

```sh
set +e
pnpm --filter impluvium-desktop-ui run typecheck > /tmp/typecheck.txt 2>&1
typecheck_status=$?
set -e
node tools/ci/tsc-ratchet.mjs --baseline tools/ci/tsc-baseline.json --previous-baseline /tmp/base-tsc.json --package impluvium-desktop-ui --input /tmp/typecheck.txt --status "$typecheck_status"
```

Only completed exit statuses 0, 1 and 2 are accepted. Configuration/input
diagnostics always fail with exit 1 even at an unchanged exact budget. The
classification follows the compiler's
[diagnostic definitions](https://raw.githubusercontent.com/microsoft/TypeScript/v5.9.3/src/compiler/diagnosticMessages.json):
TS5xxx (compiler/project options), TS6044–TS6046 (option arguments and response
files), TS6048/TS6050/TS6051 (locale/file inputs), TS6053/TS6054/TS6059/TS6231
(missing, unsupported or out-of-root inputs), TS6064 (configuration-only option),
TS6304–TS6310 (project-reference/output settings) and TS18000–TS18003
(configuration resolution/empty inputs; the reserved TS18001 is rejected too).
Each explicitly classified code has an exact-budget rejection test. A nonzero
status without TypeScript diagnostics fails. Empty output with status 0 establishes zero, which requires
a zero baseline. The named package's current count is checked per call; the
previous-baseline comparison checks the whole file. Ordinary unused-source
diagnostics such as TS6133 remain countable.

Test baselines specify **exact counts or minimums** for successful ordinary
tests and doc-tests separately. Ignored, measured and filtered tests never
count as successes:

```json
{"version":1,"packages":{"sample":{"tests":{"exact":25},"docTests":{"min":0}}}}
```

`{"exact":N}` requires equality; `{"min":N}` allows additional passing tests.
For droplet, choose `{"exact":25}` for the generated-test implementation or
`{"exact":1}` for the runtime-test implementation when wiring the test job. The runtime
test must itself assert the exact set of 25 files. These are baseline choices;
the parser does not need an alternative-values syntax. Compared with the base
revision, counts/minimums cannot fall, new packages are allowed, and exact
constraints cannot be weakened to minimums.

Capture one workspace-wide log, with Cargo headers and both output streams:

```sh
cargo metadata --locked --all-features --format-version=1 > /tmp/metadata.json
set +e
cargo test --locked > /tmp/workspace-tests.txt 2>&1
cargo_test_status=$?
set -e
node tools/ci/count-tests.mjs --baseline tools/ci/test-baseline.json --previous-baseline /tmp/base-tests.json --metadata /tmp/metadata.json --input /tmp/workspace-tests.txt
test "$cargo_test_status" -eq 0
```

`Running ... (target/.../deps/TARGET-hash)` and `Doc-tests TARGET` are attributed
to workspace packages through metadata target names. Hyphen/underscore spelling
is normalized. Unknown or ambiguous names are input errors; give workspace
test targets unique names. Repeat `--input FILE` for disjoint logs, or scope
expected packages with `--package NAME`; repeated suites are rejected to prevent
double counting. Observed packages need baseline entries. Multiple ordinary
suites are summed per package, while doc-tests cannot cover ordinary tests.
Multiple libtest blocks under one doctest header (merged doctests) are allowed.
Zero ordinary tests still require a complete observed suite. Missing doc-tests
are allowed only for a zero exact count/minimum. Failed and truncated suites
fail independently of the count. Keep the original Cargo exit status fatal as
shown; the parser cannot infer build failures before the test harness starts.

`check-lock-versions` keeps chosen packages of the root `Cargo.lock` at exact
versions. The expectations file `locked-versions.json` names each package and
its version:

```sh
node tools/ci/check-lock-versions.mjs --lock Cargo.lock --expect tools/ci/locked-versions.json
```

Every named package must be locked, at the expected version and at no other.
It holds the Tauri crates together: the desktop manifest pins `tauri` and its
plugins with `~` requirements, but Tauri's internal crates (`tauri-build`,
`-codegen`, `-macros`, `-plugin`, `-runtime`, `-runtime-wry`, `-utils`) are
held only by the lock file. A plain `cargo update` would move them to newer
minor versions than the locked `tauri` release was built with. Update Tauri as
one set: change the manifest, the lock and this file in the same change.

`check-names` is a ratchet on names inherited from the upstream project. It
counts, per file and pattern, the lines that match one of ten patterns (the
upstream product name as a word, its organisation in both spellings, its URL
scheme, the old desktop titles, the old package, library, RPM and build-argument
names, and the old data directory and keyring names as string literals). A line
counts once per pattern. In Rust files the destructor forms `impl ... Drop for`,
`ops::Drop` and `Drop::drop` are not names. Binary files (any NUL byte) are
skipped. `names-baseline.json` holds only the counts and must match **exactly**,
as for the diagnostic ratchets:

```json
{"version":1,"files":{"web/page.vue":{"word":3,"scheme":1}}}
```

A higher count fails; a lower one asks for a lower baseline in the same change.
`--previous-baseline FILE` (the base revision's file) allows the counts to shrink
per file and pattern only; a new file or pattern in the baseline counts as growth.
`names-allow.json` lists permanent exceptions: each entry names a `path` or a
`glob`, a `reason` and optionally `lines` (exact line texts; without `lines` the
whole file is exempt). An allowed line that no longer exists fails. The tool
never writes: `--print-baseline` prints the current counts as a new baseline on
standard output, so redirect it on the host side when a change has to update the
file.

```sh
node tools/ci/check-names.mjs --root . --previous-baseline /tmp/base-names.json
node tools/ci/check-names.mjs --root . --print-baseline > names-baseline.json
```

`check-env` keeps `tools/env-register.json` and the code in step. Each register
entry has `name`, `kind`, `component`, `status`, `evidence`, `standard`,
`secret`, `fate` (`action` keep, rename or remove, `to` for a rename, and `when`
as an English sentence), `description` and, for names built at run time, a
`pattern`. `evidence` is `read` when project code reads the name, or
`consumer: <who>` when a tool, framework or image does. The register is public:
it carries no planning identifiers and the check rejects the common shapes.
The check reads these forms and nothing else:

- `process.env.NAME`, `process.env["NAME"]` and `process.env['NAME']` in
  `*.ts`, `*.js`, `*.mjs` and `*.vue` below `server/`, `desktop/main/` and
  `libraries/base/`;
- in `*.rs`, `std::env::var("NAME")`, `std::env::var_os("NAME")`, the same calls
  as `env::var(...)` and `env::var_os(...)` when the file imports the module
  (`use std::env;`, `use std::{..., env, ...};` or `use std::env::{self, ...};`),
  and the macros `option_env!("NAME")` and `env!("NAME")` with any of the
  delimiters `()`, `[]` and `{}`;
- `ARG NAME` and `ENV NAME=value` in a `Dockerfile`, and `NAME=value` in a
  `*.env.example` file (optionally with `export` and spaces around `=`; commented
  declarations count too). Any other line in such a file that is not empty or a
  comment is exit 2.

In `*.vue` files only the `<script>` blocks are read (the template cannot reach `process`); a script tag whose block cannot be extracted is exit 2. In scripts a `/` after a value or an unclear context is read as division, never as a regex. In scripts, comments are blanked before matching (so `process /* x */ .env.NAME`
is a read and a commented-out read is not), while string contents stay visible: a
read inside a string or template text would still count. `process` may only
appear as `process.<member>` (`process.platform`, `process.cwd()`, `process.exit()`
and `process.env` are fine); an alias, cast (`process as ...`), spread,
destructuring or computed access is rejected.

Everything else that touches the environment is an error: any other use of
`process.env` (destructuring, aliases, spread, enumeration, a computed name),
any other use of `process` as a value, in Rust the direct import or alias
of a reader (`use std::env::var;`, `use std::env::{var, ...};`,
`use std::{..., env::var, ...};`, `use std::env as ...;`, `var as ...`, also over
several lines; `{self as ...}` counts as an alias of its parent), `env::vars()`, `env::vars_os()`, and `var` calls with a name that
is not a string literal. The only exception is a register entry with a `pattern`
(`prefix`, `file` and a `snippet` of at least two consecutive lines): the line that
builds the name and the line that reads it. The rejected access must end on the
last line of the snippet in that file, and the first line must contain the prefix.
If either line changes, the exception no longer matches and the check fails.
`tools/`, `.gitea/`, the self-tests and shell scripts are not scanned for reads;
the only product script reads no project variable.

The check fails when (a) a name that is read or declared is not registered, (b) an
`active` entry with evidence `read` has no read site (a declaration in an example
file or `Dockerfile` is not one), (c) an `IMPLUVIUM_` name appears anywhere in the
repository (except the self-tests) without a register entry (a name ending in `*`
covers its family), or (d) with `--previous-register FILE` (the base revision's
register), a name without the `IMPLUVIUM_` prefix that is not a standard name
becomes `active` or `planned` for the first time. A malformed register or
unreadable source is exit 2.

```sh
node tools/ci/check-env.mjs --root . --previous-register /tmp/base-env-register.json
```

`identity-doc` renders `docs/dev/identity.md` from `tools/identity.json` and the
register through a pure function (`render`) and compares it with the committed
page; it only reads (`--check` is required). The page is written by
`tools/identity/write-doc.mjs`, which uses the same function. Run the writer, like
every repository script, only in a disposable container; `--stdout` prints the
page so that it can be redirected on the host side.

```sh
node tools/ci/identity-doc.mjs --root . --check
node tools/identity/write-doc.mjs --root . --stdout > docs/dev/identity.md
```

`check-identity` ties the places in the code that carry an identity value to the
identity table. Every entry of `bindings` in `tools/identity.json` names a dotted
path in the table (`value`), a repository-relative `file` and exactly one
selector: `json` (a JSON pointer), `toml` (a dotted key) or `regex` (a regular
expression with exactly one capture group, matched per line over the whole file).
The check fails when the file is missing, when the selector does not resolve
exactly once, or when the value found differs from the table. A malformed binding
or a regular expression without exactly one group is an input error (exit 2).
With `url_scheme.registered_by_current_client` set to `true`, the scheme must be
bound in `desktop/src-tauri/tauri.conf.json`; with `false`, that file must name no
scheme. Deleting or rewriting bound code fails the check until the binding moves
with it. The check only reads files.

```sh
node tools/ci/check-identity.mjs --root .
```

In the `checks` job, the step "Baseline of the base revision" also extracts
`tools/ci/names-baseline.json` and `tools/env-register.json` from the base
revision, exactly like the feature-gate allow-list: a missing file in the base is
the first introduction and allowed only while the base `ci.yml` does not mention
it.
