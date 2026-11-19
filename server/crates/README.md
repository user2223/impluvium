<!-- SPDX-FileCopyrightText: 2026 the impluvium contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Rust server crates

The server shares the root Cargo workspace, lock file and stable toolchain.
Select its packages explicitly; the workspace default members stay unchanged.

- `server-api-types`: public web contracts and wire adapters.
- `server-domain`: framework-independent policies and application ports.
- `server-db`: the PostgreSQL pool, the embedded migrations in `server/migrations` and database adapters, depending on the domain.
- `server-http`: HTTP adapters depending on the domain and public API types.
- `server-bin`: the `impluvium-server` executable, depending on the HTTP layer and the domain, and on the protocol
  core of the engine for key derivation.

The entry modules establish the dependency graph. They do not yet serve HTTP,
connect to a database or implement application contracts; `server-db` provides the pool
and the first migration, which nothing calls yet. The executable exits
unsuccessfully until guarded startup is implemented. Two commands exist already:
`impluvium-server config check` validates the environment without opening a
listener or a database connection, and `impluvium-server config reference`
prints the page `docs/admin/configuration.md`, which a test keeps in step with
the field table in `server-bin/src/config/fields.rs`. A third command,
`impluvium-server routes --json`, prints the inventory of the routes the server
serves, with policy, contract and limits of each; it needs no database or key
file. The inventory and the router are built from the same route registry in
`server-http`, which also answers every error in one JSON shape: `server-api-types`
defines the error codes and keeps their catalog in `error-codes.json`, and a test
fails when that file drifts from the code. `server-http` adds request ids, the
protective response headers and the health probes; `server-bin` holds the JSON
log writer and the bounded graceful shutdown. Further framework dependencies will
be added with the code that needs them.

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

`server-db` owns the one connection pool (`Database`), built from explicit `PoolOptions` and a `DatabaseUrl` that
never prints itself, and the migrations, which are plain SQL files compiled into the binary. A migration that has
been applied is never edited: `migrate` refuses a database whose recorded checksums differ or that knows
migrations this build lacks. The first migration holds only the infrastructure schema (instance identity, the
binding to the public content key, ticket key generations, initialisation state). The pool connects without TLS:
a database on an untrusted network needs a later decision about TLS support. Statements use bound values and
runtime `sqlx::query`; the compile-time checked query macros are not used.

Tests that need PostgreSQL live in `server/crates/server-db/tests/server_db_postgres.rs`. All of them are
ignored in a normal `cargo test` run. Run them against a disposable server with
`IMPLUVIUM_TEST_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/postgres cargo test -p server-db --test
server_db_postgres -- --ignored --test-threads=1`; every test creates and drops its own database, and a missing
variable fails the run. CI runs them in the job `server-db` against PostgreSQL 16, 17 and 18.
