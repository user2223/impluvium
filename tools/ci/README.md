# CI checks

These tools require Node 24 and use only its standard library. They read files;
they never invoke Cargo, a compiler, a package manager, Git or a shell. Each
entry point accepts `--help`. Exit codes are 0 for success, 1 for a failed check,
and 2 for invalid arguments, unreadable files or malformed/unsupported input.
Diagnostics use `file:line: message`.

Run the self-tests **inside a disposable Node 24 container**, from the root of
an exported source tree without `.git`:

```sh
node --test tools/ci/test/*.test.mjs
```

The fixtures are synthetic. Their domains, addresses and absolute paths use
reserved example values. No installation, network access or build is needed
for the self-tests. The tools skip generated trees and their fixture directory.

```sh
node tools/ci/check-pins.mjs --root .
node tools/ci/check-features.mjs --root . --previous-baseline /tmp/base-feature-gates.json
node tools/ci/check-licence-boundary.mjs --root .
```

`check-pins` checks the final six required job IDs, without job names, matrices
or job-level conditions. Step conditions are allowed. It checks workflow
actions and containers, Dockerfile images, workflow tool versions/checksums,
Cargo commands and pnpm installs in workflow `run`, Dockerfile `RUN`, and
`package.json` scripts. Deployment templates are excluded. It compares Node 24
across `.node-version`, tagged Node images, Fedora `nodejs24`/`nodejs24-npm`
packages and `engines.node` (required in root `package.json`). Other manifests
may omit engines; any declared Node range must stay within that major. Root
`packageManager` must specify an exact pnpm version and integrity hash.

Docker image defaults belong in global `ARG`s used by `FROM`, or directly in
`FROM`. Reuse an ARG or stage rather than repeating an image pin. Identical
workflow image/action literals may recur where the workflow requires them;
conflicting values and additional Docker definitions fail. Tool version and
checksum variables belong once in workflow-level `env`; run scripts reference
those variables. Image context overrides and workflow Rust toolchain overrides
fail. Non-Corepack tool versions need matching `_SHA256` variables. The one npm
exception is `npm install -g "corepack@${COREPACK_VERSION}"`, which bootstraps
the package manager; all application installs must use frozen pnpm locks.

The parsers support the repository's data-oriented TOML and workflow YAML:
quoted strings/keys, comments, inline collections, multiline values, literal
shell blocks and simple folded shell blocks. YAML tags, aliases, merge keys,
complex folded blocks, Dockerfile here-documents and exec-form `RUN` produce
input errors. Use literal `|` for shell scripts containing blank or indented
lines. Shell scanning handles quoted arguments, continuations, ordinary
separators, `env`/`sudo`/`command`/`exec` prefixes and explicit `sh -c` scripts.
It is a static check, not a general shell interpreter: keep checked commands
explicit rather than constructing executable names or flags dynamically.

`check-features` associates each Rust file with its nearest package manifest,
ignores strings/comments, and checks gate **names**, including nested
`cfg_attr`. `feature-gates.json` permits the existing desktop gates; unlisted
crates permit none. Its format is:

```json
{"version":1,"crates":{"desktop/bridge/Cargo.toml":["existing_gate"]}}
```

An empty `crates` object is valid. Remove unused gates in reviewed edits. To
enforce that this file only shrinks, CI must supply the base revision's file
via `--previous-baseline`; the tool does not fetch or trust Git history itself.
Omitting that option checks source against the current allow-list only.

`check-licence-boundary` follows all local Cargo dependency kinds transitively,
including target-specific, optional, renamed, workspace-inherited and patched
dependencies. It rejects paths from `engine/` into `server/crates/`, MPL in
engine package licence expressions, and MPL licence text in crate/ancestor
licence files or explicit `license-file` inputs. It resolves inherited licence
metadata without requiring Cargo or a lockfile.

Both diagnostic ratchets use a small, nonempty package-name-to-count baseline:

```json
{"version":1,"packages":{"sample":3,"other-package":0}}
```

Counts must be nonnegative safe integers. A rise or missing package fails;
a fall passes with an instruction to lower the checked-in baseline manually.
Neither tool writes a baseline. Diagnostic counts are per package, never a
combined budget. Capture actual counts in the relevant build container before
adding production baselines; the supplied diagnostic baselines are fixtures.

For Clippy, capture **stdout only**, preserving JSON lines. Metadata must include
dependencies (do not use `--no-deps`); exact package IDs are joined against it,
so path basenames and Rust target names need not equal package names.

```sh
cargo metadata --locked --format-version=1 > /tmp/metadata.json
cargo clippy --locked --all-targets --message-format=json -- --cap-lints warn > /tmp/clippy.jsonl
node tools/ci/clippy-ratchet.mjs --baseline tools/ci/clippy-baseline.json --metadata /tmp/metadata.json --input /tmp/clippy.jsonl
```

Only `clippy::` diagnostics count, including cached diagnostics; compiler errors
and unsuccessful builds still fail. Each selected package needs an artifact
message, even at zero, and the stream needs a `build-finished` message. One
diagnostic emitted for two targets counts twice; keep Clippy flags consistent.
All baseline packages are required by default. For separate stable/nightly runs,
repeat `--package NAME` for each package expected in that run.

For TypeScript, capture one package's combined output and its original status.
Unformatted tsc/vue-tsc output is preferred; pretty diagnostics and ANSI colour
are accepted too. Run the existing package typecheck command after preparation:

```sh
set +e
pnpm --filter view run typecheck > /tmp/typecheck.txt 2>&1
typecheck_status=$?
set -e
node tools/ci/tsc-ratchet.mjs --baseline tools/ci/tsc-baseline.json --package view --input /tmp/typecheck.txt --status "$typecheck_status"
```

A nonzero status without TypeScript diagnostics fails, so missing executables
cannot masquerade as zero errors. Empty output with an explicitly supplied zero
status establishes zero. Only the named package's entry is checked per call.

Test baselines specify **minimum successful ordinary tests and doc-tests
separately**. Ignored, measured and filtered tests never count as successes:

```json
{"version":1,"packages":{"sample":{"tests":25,"docTests":0}}}
```

Capture one package per file, with Cargo headers and both output streams:

```sh
cargo test --locked -p sample > /tmp/sample-tests.txt 2>&1
node tools/ci/count-tests.mjs --baseline tools/ci/test-baseline.json --input sample=/tmp/sample-tests.txt
```

Repeat `--input PACKAGE=FILE` for all baseline packages, or deliberately scope
with repeated `--package NAME`. Multiple unit/integration suites are summed;
doc-tests cannot cover a missing ordinary suite. A zero ordinary-test baseline
still requires a complete observed suite. Missing doc-tests are allowed only
when their minimum is zero. Failed and truncated suites fail independently of
the minimum. Keep the original Cargo exit status fatal as well; the tool checks
libtest results, not arbitrary build failures before the test harness runs.
