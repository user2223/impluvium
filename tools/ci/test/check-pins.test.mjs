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

test('pins reject duplicate Docker pins while allowing reuse of the same ARG', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'FROM base AS runtime', 'FROM ${NODE_BASE} AS runtime');
  invoke('check-pins', ['--root', root], 0);
  replace(root, 'Dockerfile', 'FROM ${NODE_BASE} AS runtime', 'FROM node:24-trixie-slim@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc AS runtime');
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
