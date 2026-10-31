# Desktop

The official desktop client for Drop.

## Dependencies

The client (`desktop/`, with the Tauri CLI) and its UI (`desktop/main/`) belong to the pnpm workspace at the
repository root. Install them from the repository root; the UI's Nuxt layer in `libraries/base` comes along:

```sh
pnpm install --frozen-lockfile --filter "{./desktop}" --filter "{./desktop/main}..." --fail-if-no-match
```

The directory filters resolve against the current directory. `--fail-if-no-match` makes the command fail when it is
run from elsewhere, instead of installing nothing.

The UI uses Nuxt 4 and the server Nuxt 3, so install per project as shown instead of the whole workspace.

## Building

The desktop crates are members of the Cargo workspace at the repository root, but they need the nightly toolchain
pinned in `src-tauri/rust-toolchain.toml`. Run Cargo for them from `src-tauri`, where rustup selects that toolchain,
for example `cargo clippy --locked -p drop-app`. From the repository root Cargo uses the stable toolchain and cannot
build them. For rust-analyzer, set `RUSTUP_TOOLCHAIN` to the desktop toolchain in its environment (for example with
`rust-analyzer.cargo.extraEnv`).

Change `Cargo.lock` (for example with `cargo update`) only from the repository root, so that the stable toolchain
chooses the versions.

Release builds use the workspace profile `release-desktop` and end up in `target/release-desktop/` at the repository
root. Tauri first generates the UI (`pnpm -C main run build`, output in `desktop/.output/main`). From the repository
root:

```sh
pnpm -C desktop exec tauri build -- --locked --profile release-desktop
```
