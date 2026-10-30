# Desktop

The official desktop client for Drop.

## Building

The desktop crates are members of the Cargo workspace at the repository root, but they need the nightly toolchain
pinned in `src-tauri/rust-toolchain.toml`. Run Cargo for them from `src-tauri`, where rustup selects that toolchain,
for example `cargo clippy --locked -p drop-app`. From the repository root Cargo uses the stable toolchain and cannot
build them. For rust-analyzer, set `RUSTUP_TOOLCHAIN` to the desktop toolchain in its environment (for example with
`rust-analyzer.cargo.extraEnv`).

Change `Cargo.lock` (for example with `cargo update`) only from the repository root, so that the stable toolchain
chooses the versions.

Release builds use the workspace profile `release-desktop` and end up in `target/release-desktop/` at the repository
root:

```sh
pnpm exec tauri build -- --locked --profile release-desktop
```
