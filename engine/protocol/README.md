<!-- SPDX-FileCopyrightText: 2026 the impluvium contributors -->
<!-- SPDX-License-Identifier: 0BSD -->

# Protocol packages and generation

`proto/` is the single source of the wire schema. The Rust package
`impluvium-protocol` exposes checked-in buffa messages through `proto` and
Connect bindings through `connect` when `server` or `client` is selected.
`impluvium-client` reexports the protocol and client transport types. These
facades do not implement validation, HTTP policy, TLS setup or credential
handling. The TypeScript package `impluvium-protocol` contains generated
messages and service descriptions, without a second security implementation.

All packages share the root Cargo or pnpm workspace and lock. Own engine
sources are 0BSD; see `../LICENSE` and `NOTICE`. Runtime dependencies retain
their own licenses. All new packages are private and are not published.

## Product selections

The names below describe package and feature selections, not Cargo build
profiles. Run from the repository root. Keep separate target directories,
logs and test baselines for each selection and target.

| Selection | Cargo arguments | Dependency boundary |
|---|---|---|
| `core` | `-p impluvium-protocol --no-default-features` | buffa and buffa-types `std,json`, SHA-256 and BLAKE3; no HTTP or TLS |
| `server` | `-p impluvium-protocol --no-default-features --features impluvium-protocol/server` | Connect `axum,json`, without compression |
| `client` | `-p impluvium-client --no-default-features` | Protocol client feature, Connect `client,json`, reqwest and hyper-rustls, aws-lc provider |
| `combined` | `-p impluvium-protocol -p impluvium-client --no-default-features --features impluvium-protocol/server` | Both roots in the same build with their unified features |

Linux builds and tests use `x86_64-unknown-linux-gnu`. All four selections
also require cross-builds for `x86_64-pc-windows-gnullvm` with
`-C target-feature=+crt-static`. The Windows archive pin has one source:
`../../tools/ci/versions.env`. Cross-build success is not native Windows
runtime acceptance. These packages currently provide no ring fallback.

The protocol has no default features. Client dependencies disable defaults
and select the two HTTP stacks explicitly. Neither Connect compression nor
`client-tls`, reqwest system proxy, native TLS or a second provider belongs
in an engine product graph. The platform verifier is the intended trust
source; do not enable an additional hyper-rustls trust path.

## Container preparation

Run all installs, generation, builds and tests in a disposable container
with pinned build inputs, a source export and no host home, credentials or
container socket. Run shell blocks with `bash -euo pipefail`. Preserve the
prepared container state when disabling networking for builds and checks.
The commands here describe that container, not host execution.

Runtime and TypeScript package pins live in their manifests. Rust runtime
pins are also checked by `../../tools/ci/locked-versions.json`. Generator
versions and archive SHA-256 values have one central source in the workflow
`env`; `../../tools/ci/codegen-tools.json` maps them to public downloads,
binaries and frozen tool locks. The service generator's lock is separate
from the message generator's lock: their buffa-codegen versions differ.

Bootstrap Node from the root Node-major pin, Corepack and pnpm from the root
package-manager pin, and Rust from `../../rust-toolchain.toml`. Install only
the existing TypeScript package from the root lock:

```sh
pnpm install --frozen-lockfile --ignore-scripts \
  --filter "{./engine/protocol/ts}..." --fail-if-no-match
```

Download buf and the three Rust generator source archives from the mapped
public URLs. Verify each SHA-256 before installing a binary or extracting
an archive. Extract tool sources outside the repository export. Immediately
compare each extracted `Cargo.lock` with its frozen tool lock, before any
`cargo fetch --locked` or build. Build the plugins with `--release --locked
--offline` after fetching. Never install tools automatically from xtask,
use remote plugins or add generator libraries to the product graph.

The frozen tool locks are unchanged imports from checksum-verified source
archives. Their measured hashes are recorded in the tool mapping.
`tools/ci/protocol-tools.mjs --kind codegen` checks those hashes, verifies
each archive, compares its extracted lock, fetches with `--locked`, builds
offline and installs only the named executable, in that order. Missing or
changed locks stop preparation before fetch or build. Updates require a new
container import and review; no hand-written stand-ins belong in those files.

Use `/work/tools/bin` for verified buf, the three Rust plugins and the
separately bootstrapped cargo-deny. Add that directory and the absolute
`engine/protocol/ts/node_modules/.bin` directory to PATH, retaining the Rust
toolchain. Verify each executable's resolved path before use. Each CI job
needs its own bootstrap; tool state does not cross job boundaries.

## Explicit generation and drift

The normal engine build uses checked-in outputs and needs no generator,
`protoc`, BSR connection or product `build.rs`. TypeScript builds use `tsc`
on checked-in TypeScript sources. There are no installation hooks.

| Output | Repository path |
|---|---|
| Rust messages, views and packaging | `engine/crates/protocol/src/generated/buffa/` |
| Rust services and packaging | `engine/crates/protocol/src/generated/connect/` |
| TypeScript messages and service descriptions | `engine/protocol/ts/src/generated/` |

For an intentional source update, run in the prepared container, offline:

```sh
cargo run --locked --offline -p impluvium-protocol-xtask -- generate
cargo run --locked --offline -p impluvium-protocol-xtask -- check-drift
```

`generate` first produces all outputs in a fresh scratch layout and only
then replaces the three owned output trees. It removes obsolete files.
`check-drift` uses the same generation function in scratch and compares
complete file sets and bytes against the export. It reports missing,
additional and changed files and never repairs the export. Neither command
needs `.git`; tool failures propagate. Symlinks and output paths outside
the declared trees are rejected. Scratch trees are removed on completion.

Both packaging invocations use `strategy: all`. The Connect packaging
invocation additionally uses `filter=services`, so the service-less
`impluvium.manifest.v3` and `impluvium.signed.v1` packages never reference
missing service files. `gate_client_feature=client` keeps generated clients
out of the protocol's server-only selection.

The TypeScript generator selects `target=ts,import_extension=js`, matching
the package's NodeNext module contract. The Rust facade marks only the two
generated module bindings with `#[rustfmt::skip]` to preserve the pinned
generator's byte output. Handwritten Rust remains subject to the workspace
format check; the generator trees remain subject to exact drift checking.

Check an unchanged export directly with `check-drift`; do not run `generate`
before it in CI or any bootstrap. Codegen tests generate and mutate only
their own scratch copies. Return only the three generated source trees
after an intentional update, never scratch files, tool sources or binaries.

## Breaking checks and package checks

`tools/ci/check-protocol-breaking.mjs` has two independent modes. `report`
uses the actual PR or push base export. The source record is:

```json
{
  "version": 1,
  "event": "pull_request",
  "currentSha": "<actual head SHA>",
  "base": { "status": "available", "sha": "<actual base SHA>", "exportRoot": "/work/base" }
}
```

For an unavailable base use `status: "unavailable"`, `sha: null` and an
explicit `reason`. An available base needs an existing absolute `exportRoot`,
so a missing export cannot pass as an export without a schema.
SHAs must be full lowercase commit hashes. Never replace
the base with the current head. `--base` names the protocol directory in
the base export; historical schemas need no `buf.yaml`. `report` records
incompatibility without failing before the wire-format freeze, and records
`no-comparison` explicitly when the base schema is unavailable. Invalid
exports, tool failures and malformed diagnostics fail. `selftest` needs no
historical base: it requires a clean comparison against the current schema
and then the exact deletion diagnosis for `StartPairingRequest.device_name`.
Both modes preserve the exports and write a fresh results directory.

```sh
buf lint engine/protocol
node tools/ci/check-protocol-breaking.mjs --mode report \
  --current /work/repo/engine/protocol --base /work/base/engine/protocol \
  --source /work/results/source.json --out /work/results/breaking-base
node tools/ci/check-protocol-breaking.mjs --mode selftest \
  --current /work/repo/engine/protocol --out /work/results/breaking-selftest
cargo run --locked --offline -p impluvium-protocol-xtask -- check-drift
cargo test --locked --offline -p impluvium-protocol-xtask --test codegen
pnpm -C engine/protocol/ts run typecheck
pnpm -C engine/protocol/ts run build
pnpm -C engine/protocol/ts run test
```

Run generator-free xtask tests separately with `--bin
impluvium-protocol-xtask`; do not run its codegen integration tests in a job
without the plugins. Build, test and lint each product selection explicitly
with separate logs. Test counts must come from actual runs. The TS export
test imports through both the package root and nested `generated/*` exports
after the build. Keep `dist/` out of source control.

For actual C compiler and archiver evidence, fresh builds must set and
record `CC_ENABLE_DEBUG_OUTPUT=1`, retaining complete build-script output
alongside Cargo JSON and stderr. Workspace-wide metadata feature unions
are insufficient evidence for an individual product selection.
