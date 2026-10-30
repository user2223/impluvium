# CI checks

These tools require Node 24 and use only its standard library. They read files;
they never invoke Cargo, a compiler, a package manager, Git or a shell. Each
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
node tools/ci/check-licence-boundary.mjs --root . --metadata /tmp/metadata.json --allow-no-engine
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
`DROP_VERSION: ${{ steps.version.outputs.version }}` are ordinary runtime data.
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

Static inspection of the current repository identifies two invocation outcomes
changed by this subset: `.gitea/workflows/ci.yml:143` (`pnpm tauri build ...`)
changes from exit 1 for missing forwarded Cargo locking to exit 2 for an
unallowlisted action; `Dockerfile:90` (`pnpm prisma init`) changes from an
unchecked action to exit 2. Use explicit `run`/`exec` when updating those files.
The synthetic passing workflow uses `pnpm exec tauri`. No current package script
needs an action rewrite. These are per-invocation expectations; the full current
repository check still exits 2 first because root `rust-toolchain.toml` is absent
(root `.node-version` is also absent). The real workflow's tool-free heredoc
retains its existing image-context override failures.

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
licence expression mentioning MPL, GPL, LGPL or AGPL, and any explicit
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
tables does. Selecting no `engine/` packages is exit 2 unless `--allow-no-engine`
is explicit. Until `engine/` exists in this repository, CI uses that flag as in
the invocation above; remove it when adding the first engine package. It only
permits an empty selection and never skips manifest coverage or licence checks.
Metadata must cover every engine workspace in one document (merge the packages, workspace members and
resolved nodes of separate captures if necessary). The capture's workspace
root may be `engine/`; `--root` must still name the repository root, at the
same filesystem location used during capture. The engine boundary is intended
to remain 0BSD with permissively licensed dependencies all the way down.

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
commands shown here. Use the actual checked-in baseline paths when wiring
the tools into CI; these production count files are not created by this tools-only change. Do not fall
back to the current file if a base file is absent or unreadable.

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
pnpm --filter view run typecheck > /tmp/typecheck.txt 2>&1
typecheck_status=$?
set -e
node tools/ci/tsc-ratchet.mjs --baseline tools/ci/tsc-baseline.json --previous-baseline /tmp/base-tsc.json --package view --input /tmp/typecheck.txt --status "$typecheck_status"
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
