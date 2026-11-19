<!-- SPDX-FileCopyrightText: 2026 the impluvium contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Rust server crates

The server shares the root Cargo workspace, lock file and stable toolchain.
Select its packages explicitly; the workspace default members stay unchanged.

- `server-api-types`: public web contracts and wire adapters.
- `server-domain`: framework-independent policies and application ports.
- `server-db`: database adapters depending on the domain.
- `server-http`: HTTP adapters depending on the domain and public API types.
- `server-bin`: the `impluvium-server` executable, depending on the HTTP layer and the domain, and on the protocol
  core of the engine for key derivation.

The entry modules establish the dependency graph. They do not yet serve HTTP,
connect to a database or implement application contracts. The executable exits
unsuccessfully until guarded startup is implemented. Two commands exist already:
`impluvium-server config check` validates the environment without opening a
listener or a database connection, and `impluvium-server config reference`
prints the page `docs/admin/configuration.md`, which a test keeps in step with
the field table in `server-bin/src/config/fields.rs`. Framework dependencies
will be added with the code that needs them.

Within this server graph, only `server-http` may directly depend on axum/tower,
or Hyper with server support, and only `server-db` may reach sqlx packages through
normal dependencies. Every direct dependency needs an entry for its owning
crate in `tools/ci/server-dependencies.json`. Entries identify the actual package
name and, for local dependencies, the repository-relative manifest path. The
domain and public API types currently admit no dependencies. Adding a dependency
requires reviewing its admission entry together with its dependency graph.
Engine packages may never depend on server packages. A separate development
tool graph may use sqlx; it must not become a product dependency.

`node tools/ci/check-server-boundaries.mjs --root .` checks admission, composition,
baselines and the explicit CI package selections. Admission includes renamed,
workspace-inherited, optional, target, build and test dependencies. In the server
CI job, `--metadata FILE` additionally checks resolved normal paths to sqlx and
direct HTTP framework ownership. Metadata includes registry dependencies and
dependency kinds; missing graph information fails the check. The existing
licence checker also checks the resolved engine graph. The server CI job runs
formatting, Clippy with warnings denied, and tests with its own baseline in
`tools/ci/server-tests.json`. Zero tests describe these empty modules; the test
counter still requires an observed suite for every package. New integration
test targets must use crate-specific names so reports remain unambiguous.
