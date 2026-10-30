import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository } from './helpers.mjs';

const workflow = '.gitea/workflows/ci.yml';
test('pins: quoted strings, comments, folded YAML, continuations and repeated job literals pass', t => {
  invoke('check-pins', ['--root', repository(t)], 0);
});

const failures = [
  [workflow, 'cargo test --locked -p core', 'cargo test -p core', /cargo test requires --locked/],
  [workflow, 'cargo deny --locked', 'cargo deny', /cargo deny requires --locked/],
  [workflow, 'cargo test --locked -p core', 'env MODE=test command -- cargo test -p core', /cargo test requires --locked/],
  [workflow, 'cargo test --locked -p core', 'cargo test -p core -- --locked', /cargo test requires --locked/],
  [workflow, 'install --frozen-lockfile\n', 'install\n', /requires --frozen-lockfile/],
  [workflow, '  rust:\n', '  rust:\n    if: ${{ false }}\n', /job-level if/],
  [workflow, '  rust:\n', '  rust: {if: false}\n  unused:\n', /job-level if/],
  [workflow, '  rust:\n', '  rust:\n    name: Rust\n', /job-level name/],
  [workflow, '  rust:\n', '  rust:\n    strategy:\n      matrix: {arch: [x64]}\n', /matrix/],
  [workflow, 'actions/checkout@1111111111111111111111111111111111111111', 'actions/checkout@v4', /commit SHA/],
  [workflow, 'nodejs24 nodejs24-npm', 'nodejs22 nodejs22-npm', /Node major 22/],
  [workflow, 'nodejs24 nodejs24-npm', 'nodejs npm', /must name its major/],
  [workflow, '-- --locked --profile', '-- --profile', /Tauri build requires --locked/],
  [workflow, '-- --locked --profile', '--locked -- --profile', /Tauri build requires --locked/],
  [workflow, 'install --frozen-lockfile\n', 'install --frozen-lockfile --no-frozen-lockfile\n', /requires --frozen-lockfile/],
  [workflow, 'cargo fmt --all --check', 'sh -c \'cargo metadata --format-version=1\'', /cargo metadata requires --locked/],
  [workflow, 'cargo fmt --all --check', 'cargo fmt --all --check\n          buildctl build --opt context:node:24=docker-image://node@sha256:aaaa', /context overrides/],
  [workflow, 'cargo fmt --all --check', 'BUILDKIT_VERSION=0.34.1 cargo fmt --all --check', /pinned again/],
  [workflow, '    steps:\n', '    env:\n      BUILDKIT_VERSION: 0.34.1\n    steps:\n', /workflow-level env/],
  ['Dockerfile', 'node:24-trixie-slim@sha256:', 'node:22-trixie-slim@sha256:', /Node major 22/],
  ['Dockerfile', 'rust:1.99.1-slim-trixie', 'rust:1.98.0-slim-trixie', /Rust image tag must match/],
  ['Dockerfile', 'cargo build --release --locked', 'cargo build --release', /cargo build requires --locked/],
  ['Dockerfile', '# No external frontend is needed.', '# syntax=docker/dockerfile:1', /pinned by sha256/],
  ['package.json', '>=24.0.0 <25', '>=24', /engines.node/],
  ['package.json', 'cargo build --locked', 'cargo build', /cargo build requires --locked/],
  ['rust-toolchain.toml', "'1.99.1'", "'stable'", /exact stable release/],
];
for (const [file, before, after, message] of failures) {
  test(`pins rejects ${file}: ${after.slice(0, 70)}`, t => {
    const root = repository(t);
    replace(root, file, before, after);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, message);
  });
}

test('pins allow identical Docker image literals and reject differing digests', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'FROM base AS runtime', 'FROM ${NODE_BASE} AS runtime');
  invoke('check-pins', ['--root', root], 0);
  replace(root, 'Dockerfile', 'FROM ${NODE_BASE} AS runtime', 'FROM node:24-trixie-slim@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc AS runtime');
  invoke('check-pins', ['--root', root], 0);
  replace(root, 'Dockerfile', 'FROM node:24-trixie-slim@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc AS runtime', 'FROM node:24-trixie-slim@sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee AS runtime');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /duplicate or conflicting image pin/);
});

test('pins exclude deployment templates', t => {
  const root = repository(t);
  mkdirSync(path.join(root, 'server/deploy-template'), { recursive: true });
  writeFileSync(path.join(root, 'server/deploy-template/package.json'), '{"scripts":{"install":"pnpm install"}}');
  invoke('check-pins', ['--root', root], 0);
});

test('pins refuse ambiguous aliases and duplicate YAML keys', t => {
  for (const replacement of ['  checks: *shared\n', '  checks: {}\n  checks:\n']) {
    const root = repository(t);
    replace(root, workflow, '  checks:\n', replacement);
    invoke('check-pins', ['--root', root], 2);
  }
});

test('pins reject conflicting action pins and image ARG redefinitions', t => {
  const root = repository(t);
  replace(root, workflow, '      - run: \'echo "https://images.example.invalid/"\'', '      - uses: actions/checkout@2222222222222222222222222222222222222222\n      - run: \'echo "https://images.example.invalid/"\'');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /conflicting action pin/);
  replace(root, 'Dockerfile', 'FROM ${NODE_BASE} AS base', 'ARG NODE_BASE=node:latest\nFROM ${NODE_BASE} AS base');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /duplicate image ARG/);
});

test('pins reject unchecked Dockerfile syntax and malformed tool pins', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', 'RUN <<EOF\ncargo build -p core\nEOF');
  invoke('check-pins', ['--root', root], 2);
  const other = repository(t);
  replace(other, workflow, "BUILDKIT_VERSION: '0.34.1'", 'BUILDKIT_VERSION: latest');
  invoke('check-pins', ['--root', other], 1);
});

for (const script of [
  'if true; then :; else cargo test -p core; fi',
  'false || { cargo build; }',
  '{ pnpm install; }; cargo fmt --all --check',
  'if true; then :; else npm install; fi',
  'echo "$(cargo metadata --format-version=1)"',
  'echo "$(echo "$(cargo metadata --format-version=1)")"',
]) test(`pins: scans shell branches groups and substitutions: ${script}`, t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', script);
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /requires --locked|requires --frozen-lockfile|use pnpm install/);
});

for (const script of ['time cargo build', 'timeout 10 cargo test', 'nice -n 5 pnpm install', 'xargs cargo build']) {
  test(`pins: unsupported shell wrappers fail explicitly: ${script}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell wrapper/);
  });
}

test('pins: locked branches groups and quoted substitutions pass', t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', 'if true; then :; else { cargo test --locked -p core; }; fi\n          echo "$(cargo metadata --locked --format-version=1)"');
  invoke('check-pins', ['--root', root], 0);
});

for (const option of ['--mount=type=cache,target=/root/.cargo', '--network=none', '--security=sandbox']) {
  for (const [command, message] of [['cargo build', /cargo build requires --locked/], ['pnpm install', /requires --frozen-lockfile/]]) {
    test(`pins: RUN ${option} cannot hide ${command}`, t => {
      const root = repository(t);
      replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `RUN ${option} ${command}`);
      assert.match(invoke('check-pins', ['--root', root], 1).stderr, message);
    });
  }
}

test('pins: RUN options retain locked commands and reject unknown option syntax', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', 'RUN --mount=type=cache,target=/root/.cargo --network=none --security=sandbox cargo build --locked');
  invoke('check-pins', ['--root', root], 0);
  replace(root, 'Dockerfile', '--network=none', '--unknown=value');
  invoke('check-pins', ['--root', root], 2);
});

test('pins: step release versions are not workflow tool pins', t => {
  const root = repository(t);
  replace(root, workflow, '      - run: \'echo "https://images.example.invalid/"\'', '      - run: \'echo "https://images.example.invalid/"\'\n        env:\n          DROP_VERSION: ${{ steps.version.outputs.version }}\n          ARTIFACT_SHA256: ${{ steps.archive.outputs.sha256 }}');
  invoke('check-pins', ['--root', root], 0);
});

test('pins: centrally pinned env names cannot be overridden at job or step level', t => {
  for (const insertion of ['    env:\n      BUILDKIT_VERSION: 0.35.0\n', '    env:\n      BUILDKIT_SHA256: ${{ steps.hash.outputs.sha256 }}\n']) {
    const root = repository(t);
    replace(root, workflow, '  server-image:\n', '  server-image:\n' + insertion);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /must be defined once in workflow-level env/);
  }
  const root = repository(t);
  replace(root, workflow, "        if: 'always()'", "        env:\n          BUILDKIT_VERSION: 0.35.0\n        if: 'always()'");
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /must be defined once in workflow-level env/);
});

test('pins: cargo plus-toolchains and rustup run must match a toolchain file', t => {
  const root = repository(t);
  mkdirSync(path.join(root, 'desktop/src-tauri'), { recursive: true });
  writeFileSync(path.join(root, 'desktop/src-tauri/rust-toolchain.toml'), '[toolchain]\nchannel = "nightly-2026-10-18"\n');
  replace(root, workflow, 'cargo +1.99.1 clippy', 'cargo +nightly-2026-10-18 clippy');
  invoke('check-pins', ['--root', root], 0);
  replace(root, workflow, 'cargo +nightly-2026-10-18 clippy', 'cargo +nightly-2026-10-19 clippy');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /must match a rust-toolchain.toml channel/);
  replace(root, workflow, 'cargo +nightly-2026-10-19 clippy', 'rustup run nightly-2026-10-18 cargo clippy');
  invoke('check-pins', ['--root', root], 0);
  replace(root, workflow, 'rustup run nightly-2026-10-18 cargo clippy', 'rustup run nightly-2026-10-19 cargo clippy');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /rustup run .* must match/);
});

test('pins: rustup run still enforces locked Cargo commands', t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', 'rustup run 1.99.1 cargo test -p core');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo test requires --locked/);
});

for (const assignment of ['BUILDKIT_VERSION=0.35.0 cargo fmt --all --check', 'export BUILDKIT_VERSION=0.35.0', 'env BUILDKIT_VERSION=0.35.0 cargo fmt --all --check', 'declare -x BUILDKIT_SHA256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb']) {
  test(`pins: central variables reject reassignment: ${assignment}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', assignment);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned again in a run script/);
  });
}

test('pins: image literals can recur across workflow containers and Dockerfile ARGs', t => {
  const root = repository(t);
  replace(root, workflow, '  server-image:\n', '  server-image:\n    container: node:24-trixie-slim@sha256:' + 'c'.repeat(64) + '\n');
  invoke('check-pins', ['--root', root], 0);
  replace(root, workflow, 'node:24-trixie-slim@sha256:' + 'c'.repeat(64), 'node:24-trixie-slim@sha256:' + 'e'.repeat(64));
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /conflicting image pin/);
});

test('pins: different image tags of one repository have independent digests', t => {
  const root = repository(t);
  replace(root, workflow, '  server-image:\n', '  server-image:\n    container: node:24-bookworm@sha256:' + 'e'.repeat(64) + '\n');
  invoke('check-pins', ['--root', root], 0);
});

for (const instruction of ['COPY --from=busybox:latest /bin/sh /x', 'RUN --mount=type=bind,from=busybox:latest,target=/x cargo build --locked']) {
  test(`pins: external image sources require digests: ${instruction}`, t => {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', instruction);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned by sha256 digest/);
  });
}

test('pins: COPY and mount sources accept stages indices ARGs and pinned external images', t => {
  const root = repository(t);
  const pinned = 'busybox:1.37@sha256:' + 'e'.repeat(64);
  const instructions = `COPY --from=base /bin/sh /x\nCOPY --from=0 /bin/sh /x\nCOPY --from=${pinned} /bin/sh /x\nCOPY --from=\${NODE_BASE} /bin/sh /x\nRUN --mount=type=bind,from=base,target=/x cargo build --locked\nRUN --mount=type=bind,from=${pinned},target=/x cargo build --locked`;
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', instructions);
  invoke('check-pins', ['--root', root], 0);
});

test('pins: conflicting COPY and mount image digests fail', t => {
  const root = repository(t);
  const first = 'busybox:1.37@sha256:' + 'e'.repeat(64);
  const second = 'busybox:1.37@sha256:' + 'f'.repeat(64);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `COPY --from=${first} /bin/sh /x\nRUN --mount=type=bind,from=${second},target=/x cargo build --locked`);
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /conflicting image pin/);
});

test('pins: indentless workflow sequences fail explicitly instead of skipping steps', t => {
  const root = repository(t);
  replace(root, workflow, '    steps:\n      - uses:', '    steps:\n    - uses:');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /indentless block sequences are not supported/);
});

test('pins: multiline YAML flow collections fail with a one-line syntax instruction', t => {
  const root = repository(t);
  replace(root, workflow, 'on: [push, pull_request]', 'on: [\n  push,\n  pull_request\n]');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /flow collections must be on one line/);
});

test('pins: hidden configuration directories remain checked repository inputs', t => {
  for (const directory of ['.hidden', '.config', '.tools']) {
    const root = repository(t);
    mkdirSync(path.join(root, directory));
    writeFileSync(path.join(root, directory, 'package.json'), '{"scripts":{"check":"cargo build"}}');
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo build requires --locked/);
  }
});

test('pins: sequence mapping first keys and following keys both remain checked', t => {
  const root = repository(t);
  replace(root, workflow, '      - run: \'pnpm --filter view install --frozen-lockfile\'', '      - env:\n          DROP_VERSION: ${{ steps.version.outputs.version }}\n        run: cargo test -p core');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo test requires --locked/);
});

test('pins: wrappers after rustup run also fail explicitly', t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', 'rustup run 1.99.1 timeout 10 cargo test');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell wrapper/);
});

test('pins: quoted COPY and mount image sources still require digests', t => {
  for (const instruction of ['COPY --from="busybox:latest" /bin/sh /x', "RUN --mount='from=busybox:latest,type=bind,target=/x' cargo build --locked", 'RUN --mount=type=bind,from="busybox:latest",target=/x cargo build --locked']) {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', instruction);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned by sha256 digest/);
  }
});

test('pins: multiline flow mappings and detached flow sequences fail explicitly', t => {
  for (const replacement of ['on: {\n  push: {},\n  pull_request: {}\n}', 'on:\n  [\n    push,\n    pull_request\n  ]']) {
    const root = repository(t);
    replace(root, workflow, 'on: [push, pull_request]', replacement);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /flow collections must be on one line/);
  }
});
