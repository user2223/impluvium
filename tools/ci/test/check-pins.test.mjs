import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository } from './helpers.mjs';

const workflow = '.gitea/workflows/ci.yml';
function setScript(root, input, script) {
  if (input === 'package.json') {
    const file = path.join(root, input);
    const data = JSON.parse(readFileSync(file, 'utf8'));
    data.scripts.build = script;
    writeFileSync(file, JSON.stringify(data, null, 2));
  } else if (input === workflow) {
    replace(root, input, 'cargo test --locked -p core', script.replaceAll('\n', '\n          '));
  } else {
    replace(root, input, 'cargo build --release --locked -p core', script);
  }
}
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
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify .* invocation/);
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
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify .* invocation/);
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

for (const middle of ['    # build the crate', '', '    # build the crate \\']) {
  test(`pins: Docker continuations skip blank or comment lines: ${JSON.stringify(middle)}`, t => {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `RUN true && \\\n${middle}\n    cargo build --release`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /Dockerfile:12: cargo build requires --locked/);
  });
}

test('pins: Docker continuations retain locked commands after comments', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', 'RUN true && \\\n    # compile \\\n\n    cargo build --locked');
  invoke('check-pins', ['--root', root], 0);
});

test('pins: stray Docker lines are input errors', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', 'cargo build --locked');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /expected a known Dockerfile instruction/);
});

const opaqueScripts = [
  'echo "`cargo build`"',
  'echo `true; cargo build --locked`',
  "su builder -c 'cargo build'",
  'nohup cargo build',
  'stdbuf -oL cargo build',
  "eval 'cargo build'",
  'setsid cargo build',
  'flock build.lock cargo build',
  "echo sh -c 'cargo build'",
  "echo 'cargo build --locked'",
  "echo 'pnpm install --frozen-lockfile'",
  'cargo test --locked -p core; nohup cargo build --locked',
  'env --unknown cargo build --locked',
  'runuser -u builder cargo build --locked',
];
for (const script of opaqueScripts) test(`pins: unverified tool occurrence fails closed: ${script}`, t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', script);
  assert.match(invoke('check-pins', ['--root', root], 2).stderr,
    /cannot verify (cargo|pnpm) invocation at .*:\d+; write it as a plain command/);
});

for (const tool of ['cargo', 'pnpm', 'npm', 'npx', 'corepack', 'yarn']) {
  test(`pins: quoted ${tool} mentions fail in every script input`, t => {
    for (const input of [workflow, 'Dockerfile', 'package.json']) {
      const root = repository(t);
      const before = input === workflow ? 'cargo test --locked -p core'
        : input === 'Dockerfile' ? 'cargo build --release --locked -p core' : 'cargo build --locked';
      const script = input === 'package.json' ? `echo ${tool}` : `echo '${tool}'`;
      replace(root, input, before, script);
      assert.match(invoke('check-pins', ['--root', root], 2).stderr, new RegExp(`cannot verify ${tool} invocation`));
    }
  });
}

for (const prefix of ['/bin/sh -c', '/usr/bin/bash -ec']) {
  test(`pins: shell executable paths expose their scripts: ${prefix}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `${prefix} 'cargo build'`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo build requires --locked/);
    replace(root, workflow, `${prefix} 'cargo build'`, `${prefix} 'cargo build --locked'`);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const prefix of ['command --', 'exec --', 'sudo -u builder --', 'env MODE=test', 'runuser -u builder -- env MODE=test']) {
  test(`pins: recognised wrappers check their command: ${prefix}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `${prefix} cargo build`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo build requires --locked/);
    replace(root, workflow, `${prefix} cargo build`, `${prefix} cargo build --locked`);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const action of ['b', 'c', 'd', 't', 'r', 'custom-alias']) {
  test(`pins: cargo aliases require a lock flag: ${action}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `cargo ${action}`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /requires --locked or --frozen/);
    replace(root, workflow, `cargo ${action}\n`, `cargo ${action} --locked\n`);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const action of ['fmt --all --check', '--version', '-V', 'help', 'version', 'build --frozen', '--locked build', '--frozen build']) {
  test(`pins: cargo non-resolving actions and frozen builds pass: ${action}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `cargo ${action}`);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const [script, message] of [
  ['corepack pnpm install', /requires --frozen-lockfile/],
  ['corepack yarn', /use pnpm install/],
  ['pnpm exec cargo build', /cargo build requires --locked/],
  ['pnpm --filter view exec cargo build', /cargo build requires --locked/],
  ['pnpm exec tauri build', /Tauri build requires --locked/],
  ['npm exec -- cargo build', /cargo build requires --locked/],
  ['npx cargo build', /cargo build requires --locked/],
  ['pnpm dlx unlocked-package', /not in a supported lockfile/],
  ['npx unlocked-package', /not in a supported lockfile/],
  ['yarn', /use pnpm install/],
]) test(`pins: package manager launchers check inner commands and downloads: ${script}`, t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', script);
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, message);
});

for (const script of ['corepack pnpm install --frozen-lockfile', 'pnpm exec cargo build --locked', 'pnpm exec tauri build -- --locked',
  'corepack enable && pnpm --version', 'corepack pack', 'npm --version', 'yarn --version']) {
  test(`pins: checked package manager launchers pass: ${script}`, t => {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', script);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const script of ['pnpm dlx locked-package', 'npm exec -- locked-package', 'npx locked-package']) {
  test(`pins: locked package launcher ambiguity is an input error: ${script}`, t => {
    const root = repository(t);
    writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\npackages:\n  locked-package@1.0.0: {}\n");
    replace(root, workflow, 'cargo test --locked -p core', script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify (pnpm|npm|npx) invocation/);
  });
}

test('pins: package downloads absent from an existing lockfile fail', t => {
  const root = repository(t);
  writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\npackages:\n  other-package@1.0.0: {}\n");
  replace(root, workflow, 'cargo test --locked -p core', 'npx unlocked-package');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /unlocked-package is not in a supported lockfile/);
});

test('pins: malformed or unsupported download locks cannot verify launchers', t => {
  for (const [lock, content] of [['pnpm-lock.yaml', 'packages: [\n'], ['package-lock.json', '{'], ['yarn.lock', '']]) {
    const root = repository(t);
    writeFileSync(path.join(root, lock), content);
    replace(root, workflow, 'cargo test --locked -p core', 'npx some-package');
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify npx invocation/);
  }
});

test('pins: npm lockfile packages still require unambiguous launcher resolution', t => {
  const root = repository(t);
  writeFileSync(path.join(root, 'package-lock.json'), JSON.stringify({ packages: { 'node_modules/locked-package': { version: '1.0.0' } } }));
  replace(root, workflow, 'cargo test --locked -p core', 'npm exec -- locked-package');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify npm invocation/);
});

test('pins: unknown package manager prefix options cannot hide launchers', t => {
  for (const script of ['pnpm --unknown exec some-package', 'npm --unknown exec some-package', 'yarn --unknown']) {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify (pnpm|npm|yarn) invocation/);
  }
});

for (const directory of ['.gitea/workflows', '.github/workflows']) {
  for (const [body, message] of [
    ['uses: actions/checkout@v4', /commit SHA/],
    ['run: cargo build', /requires --locked/],
    ['run: pnpm install', /requires --frozen-lockfile/],
  ]) test(`pins: generic checks cover additional workflows: ${directory} ${body}`, t => {
    const root = repository(t);
    mkdirSync(path.join(root, directory), { recursive: true });
    writeFileSync(path.join(root, directory, 'build.yml'), `name: extra\non: [push]\njobs:\n  build:\n    steps:\n      - ${body}\n`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, message);
  });
  test(`pins: additional workflows need image digests: ${directory}`, t => {
    const root = repository(t);
    mkdirSync(path.join(root, directory), { recursive: true });
    writeFileSync(path.join(root, directory, 'build.yml'), 'jobs:\n  build:\n    container: fedora:44\n    steps:\n      - run: echo ready\n');
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned by sha256/);
  });
  test(`pins: additional workflows do not require the main jobs: ${directory}`, t => {
    const root = repository(t);
    mkdirSync(path.join(root, directory), { recursive: true });
    writeFileSync(path.join(root, directory, 'build.yml'), 'name: extra\njobs:\n  build:\n    if: false\n    name: Extra\n    strategy:\n      matrix: {os: [linux]}\n    steps:\n      - run: cargo build --locked\n');
    invoke('check-pins', ['--root', root], 0);
  });
}

test('pins: untagged image repositories cannot select differing digests', t => {
  const root = repository(t);
  const image = 'fedora@sha256:' + 'e'.repeat(64);
  replace(root, workflow, '  server-image:\n', `  server-image:\n    container: ${image}\n`);
  replace(root, workflow, '  client-linux:\n', `  client-linux:\n    container: ${image}\n`);
  invoke('check-pins', ['--root', root], 0);
  replace(root, workflow, `container: ${image}`, 'container: fedora@sha256:' + 'f'.repeat(64));
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /conflicting image pin for fedora:/);
});

test('pins: central variables may pass through unchanged in runuser env', t => {
  const root = repository(t);
  replace(root, workflow, 'cargo test --locked -p core', 'runuser -u builder -- env BUILDKIT_VERSION="$BUILDKIT_VERSION" BUILDKIT_SHA256="${BUILDKIT_SHA256}" NODE_IMAGE="$NODE_IMAGE" RUST_IMAGE="${RUST_IMAGE}" cargo test --locked -p core');
  invoke('check-pins', ['--root', root], 0);
});

test('pins: tool-bearing quoted heredoc scripts are unsupported', t => {
  const root = repository(t);
  const script = [
    "cat > build.sh <<'EOS'",
    'case "$REF" in',
    '  refs/tags/v*) version="${REF_NAME#v}" ;;',
    '  *) version="0.0.0-dev" ;;',
    'esac',
    'pnpm install --frozen-lockfile',
    'EOS',
    'runuser -u builder -- env BUILDKIT_VERSION="$BUILDKIT_VERSION" bash build.sh',
  ].join('\n          ');
  replace(root, workflow, 'cargo test --locked -p core', script);
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: heredoc body mentions a tool word/);
});

test('pins: changed or extended central variable pass-throughs still fail', t => {
  for (const value of ['$BUILDKIT_VERSION-extra', '${BUILDKIT_VERSION:-0.35.0}', '$OTHER_VERSION']) {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `env BUILDKIT_VERSION="${value}" cargo test --locked -p core`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned again/);
  }
});

test('pins: literal dollar assignments are not unchanged pass-throughs', t => {
  for (const assignment of ["BUILDKIT_VERSION='$BUILDKIT_VERSION'", 'BUILDKIT_VERSION=\\$BUILDKIT_VERSION']) {
    const root = repository(t);
    replace(root, workflow, 'cargo test --locked -p core', `env ${assignment} cargo test --locked -p core`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned again/);
  }
});

for (const instruction of ['ONBUILD RUN cargo build --locked', 'SHELL ["bash", "-c"]']) {
  test(`pins: unsupported Docker execution semantics fail explicitly: ${instruction}`, t => {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', instruction);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /instructions are not supported/);
  });
}

test('pins: unversioned npm package mentions require an explicit rewrite', t => {
  const root = repository(t);
  replace(root, workflow, 'nodejs24 nodejs24-npm', 'nodejs npm');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify npm invocation/);
});

for (const script of [
  'ACTION=install\npnpm "$ACTION"',
  'pnpm --filter view "${ACTION}" --frozen-lockfile',
  'npm "$ACTION"',
  'yarn "$ACTION"',
  'pnpm `printf install` --frozen-lockfile',
  'pnpm "in${SUFFIX}" --frozen-lockfile',
  'cargo "$ACTION" --locked',
  'cargo --locked "$ACTION"',
  'corepack "$MANAGER" --version',
  'pnpm exec "$COMMAND"',
  'cargo fmt; "$COMMAND" --version',
  'cargo fmt; `printf command` --version',
  'cargo fmt; ${{ env.command }}',
]) test(`pins: dynamic command or action is unsupported: ${script}`, t => {
  for (const input of [workflow, 'Dockerfile', 'package.json']) {
    // Docker instructions cannot contain an uncontinued shell newline.
    if (input === 'Dockerfile' && script.includes('\n')) continue;
    const root = repository(t);
    setScript(root, input, script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: dynamic command or tool action/);
  }
});

for (const expression of ['${{ matrix.command }}', '${{ inputs.command }}', 'echo "${{ matrix.label }}"', 'echo "${{ inputs.label }}"']) {
  test(`pins: workflow matrix and inputs expressions are unsupported: ${expression}`, t => {
    const root = repository(t);
    replace(root, workflow, '      - run: \'echo "https://images.example.invalid/"\'', `      - run: '${expression}'`);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: matrix\/inputs expressions/);
  });
}

for (const matrix of ["command: ['cargo build']", "include: [{command: 'pnpm install'}]", "exclude: [{command: 'npm update'}]"]) {
  test(`pins: tool-bearing matrix values are unsupported: ${matrix}`, t => {
    const root = repository(t);
    // An extra job has no required-job matrix restriction to mask this check.
    replace(root, workflow, 'jobs:\n', `jobs:\n  extra:\n    runs-on: ubuntu-latest\n    strategy:\n      matrix:\n        ${matrix}\n    steps:\n` + "      - run: '${{ matrix.command }}'\n");
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: strategy.matrix value mentions a tool word/);
  });
}

test('pins: unused tool-bearing matrix values also fail closed', t => {
  const root = repository(t);
  replace(root, workflow, 'jobs:\n', "jobs:\n  extra:\n    strategy:\n      matrix: {command: ['cargo build --locked']}\n    steps:\n      - run: echo ready\n");
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: strategy.matrix/);
});

test('pins: ambiguous Cargo global options cannot hide a dynamic action', t => {
  const root = repository(t);
  setScript(root, workflow, 'cargo --color always "$ACTION" --locked');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: Cargo global options before the subcommand/);
});

for (const delimiter of ['EOF', "'EOF'", '"EOF"', '\\EOF', 'E"OF"']) {
  test(`pins: heredoc tool expansions are unsupported for delimiter ${delimiter}`, t => {
    for (const input of [workflow, 'package.json']) {
      const root = repository(t);
      setScript(root, input, `cat <<${delimiter}\n# $(cargo build)\nEOF`);
      assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: heredoc body mentions a tool word/);
    }
  });
}

for (const tool of ['cargo', 'pnpm', 'npm', 'npx', 'corepack', 'yarn']) {
  test(`pins: quoted heredoc mentions of ${tool} are unsupported`, t => {
    const root = repository(t);
    setScript(root, workflow, `cat <<'EOF'\n# ${tool}\nEOF`);
    invoke('check-pins', ['--root', root], 2);
  });
}

test('pins: tab-stripped and multiple heredocs reject tool-bearing bodies', t => {
  for (const script of ["cat <<-'EOF'\n\t# $(cargo build)\n\tEOF", "cat <<FIRST <<'SECOND'\nplain text\nFIRST\n# $(cargo build)\nSECOND"]) {
    const root = repository(t);
    setScript(root, 'package.json', script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: heredoc body mentions a tool word/);
  }
});

test('pins: tool-free heredocs retain shell pin checks and following commands', t => {
  const root = repository(t);
  setScript(root, workflow, "cat <<'EOF'\necho plain-text\nEOF\ncargo build --locked");
  invoke('check-pins', ['--root', root], 0);
  replace(root, workflow, 'echo plain-text', 'buildctl build --opt context:node=docker-image://node:latest');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /context overrides/);
});

for (const script of ['pnpm upgrade', 'npm update', 'yarn up', 'pnpm add foo --frozen-lockfile', 'pnpm remove foo', 'npm dedupe', 'yarn import']) {
  test(`pins: lock-changing actions fail: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /changes the lock/);
  });
}

for (const script of ['pnpm unclassified-action --frozen-lockfile', 'npm unclassified-action', 'yarn unclassified-action', 'pnpm tauri build -- --locked',
  'pnpm "2">out.log install --frozen-lockfile', 'pnpm \\2>out.log install --frozen-lockfile']) {
  test(`pins: unclassified package actions are unsupported: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: .* action .* is not allowlisted/);
  });
}

test('pins: silent bare yarn is still an install', t => {
  const root = repository(t);
  setScript(root, workflow, 'yarn --silent');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /use pnpm install/);
});

for (const script of ['pnpm i', 'pnpm --filter view i', 'npm ci', 'yarn install --immutable']) {
  test(`pins: install-like aliases enforce the frozen pnpm policy: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /requires --frozen-lockfile|use pnpm install/);
  });
}

for (const script of ['pnpm create vite', 'npm create vite', 'npm init vite', 'yarn create vite', 'yarn dlx vite', 'npx help']) {
  test(`pins: create and dlx obey download rules: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /not in a supported lockfile/);
  });
}

for (const [name, spec] of [['vite', 'create-vite'], ['@scope/app@1.0.0', '@scope/create-app']]) {
  test(`pins: locked create launcher remains ambiguous: ${name}`, t => {
    const root = repository(t);
    writeFileSync(path.join(root, 'pnpm-lock.yaml'), `packages:\n  '${spec}@1.0.0': {}\n`);
    setScript(root, workflow, `pnpm create ${name}`);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /cannot verify pnpm invocation/);
  });
}

for (const script of ['pnpm run build', 'npm run build', 'yarn run build', 'pnpm i --frozen-lockfile',
  'yarn exec cargo build --locked', 'pnpm list', 'npm list', 'yarn list', 'pnpm why foo', 'npm why foo', 'yarn why foo',
  'pnpm help', 'npm help', 'yarn help', 'npx --version']) {
  test(`pins: explicit package action allowlist passes: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const script of ['cargo build > --locked', 'cargo build>--locked', 'pnpm install > --frozen-lockfile',
  '>build.log cargo build', '2>/dev/null cargo build', 'cargo build 2>--locked', 'cargo build < --locked', 'cargo build >> --locked']) {
  test(`pins: redirection operands never supply lock flags: ${script}`, t => {
    for (const input of [workflow, 'Dockerfile', 'package.json']) {
      const root = repository(t);
      setScript(root, input, script);
      assert.match(invoke('check-pins', ['--root', root], 1).stderr, /requires --locked|requires --frozen-lockfile/);
    }
  });
}

for (const script of ['cargo build --locked >build.log 2>&1', '>build.log cargo build --locked',
  'cargo build --locked 0</dev/null 3<>file 4>|file 5<&0 6>&-', 'pnpm install --frozen-lockfile >>install.log',
  'cargo build --locked > "$LOG_FILE"', 'cargo build --locked ">"', 'cargo build --locked 2 >out.log']) {
  test(`pins: supported redirections preserve genuine lock flags: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    invoke('check-pins', ['--root', root], 0);
  });
}

for (const script of ['cargo build --locked &>log', 'cargo build --locked <<<text', 'cargo build --locked > >(cat)',
  'cargo build --locked >&file', 'cargo build --locked >', 'cargo build --locked >> >file']) {
  test(`pins: unsupported redirection syntax fails closed: ${script}`, t => {
    const root = repository(t);
    setScript(root, workflow, script);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported construct: redirection/);
  });
}

for (const [op, body] of [['COPY', '--from=busybox:latest /bin/sh /shell'], ['RUN', '--mount=type=bind,from=busybox:latest,target=/mnt true']]) {
  for (const middle of ['', '    # option follows\n\n']) {
    test(`pins: continued Docker ${op} options require image pins: ${JSON.stringify(middle)}`, t => {
      const root = repository(t);
      setScript(root, 'Dockerfile', `true`);
      replace(root, 'Dockerfile', 'RUN true', `${op} \\\n${middle}    ${body}`);
      const line = middle ? 13 : 11;
      assert.match(invoke('check-pins', ['--root', root], 1).stderr, new RegExp(`Dockerfile:${line}: image must be literal and pinned`));
    });
  }
}

test('pins: continued Docker options accept digest images and keep command line locations', t => {
  const root = repository(t);
  const image = 'busybox:1.37@sha256:' + 'e'.repeat(64);
  setScript(root, 'Dockerfile', `COPY \\\n    --from=${image} /bin/sh /shell\nRUN \\\n    # mount follows\n\n    --mount=type=bind,from=${image},target=/mnt \\\n    cargo build --locked`);
  invoke('check-pins', ['--root', root], 0);
  replace(root, 'Dockerfile', 'cargo build --locked', 'cargo build');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /Dockerfile:16: cargo build requires --locked/);
});

function setWorkflowShell(root, scope, shell) {
  if (scope === 'step') {
    replace(root, workflow, '  server-image:\n    steps:\n',
      `  server-image:\n    steps:\n      - shell: ${shell}\n        run: console.log("ready")\n`);
  } else if (scope === 'job defaults') {
    replace(root, workflow, '  rust:\n', `  rust:\n    defaults:\n      run:\n        shell: ${shell}\n`);
  } else {
    replace(root, workflow, 'jobs:\n', `defaults:\n  run:\n    shell: ${shell}\njobs:\n`);
  }
}

test('pins: step shell npx tsx {0} fails closed', t => {
  const root = repository(t);
  setWorkflowShell(root, 'step', 'npx tsx {0}');
  assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell: value mentions a tool word/);
});

test('pins: inherited workflow shells reject custom launchers', t => {
  for (const scope of ['job defaults', 'workflow defaults']) {
    const root = repository(t);
    setWorkflowShell(root, scope, 'npx tsx {0}');
    replace(root, workflow, '      - run: |\n          cargo fmt', '      - shell: bash\n        run: |\n          cargo fmt');
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell: value mentions a tool word/);
  }
});

test('pins: plain workflow shells and standard templates pass at every level', t => {
  for (const scope of ['step', 'job defaults', 'workflow defaults']) {
    for (const shell of ['bash', 'sh', 'bash -e {0}', 'bash --noprofile --norc -eo pipefail {0}', 'sh -e {0}']) {
      const root = repository(t);
      setWorkflowShell(root, scope, shell);
      invoke('check-pins', ['--root', root], 0);
    }
  }
});

test('pins: unsupported workflow shells fail at every level', t => {
  for (const scope of ['step', 'job defaults', 'workflow defaults']) {
    for (const shell of ['pwsh', 'zsh', '/bin/bash', 'bash {0}', 'bash -c {0}', 'bash -e {0}; true', '${{ inputs.shell }}', '[]', '{}', '']) {
      const root = repository(t);
      setWorkflowShell(root, scope, shell);
      assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell/);
    }
  }
});

test('pins: workflow shell values scan tool words before comments or arguments', t => {
  for (const scope of ['step', 'job defaults', 'workflow defaults']) {
    for (const tool of ['cargo', 'pnpm', 'npm', 'npx', 'corepack', 'yarn']) {
      for (const shell of [`bash -e {0} # /usr/bin/${tool}`, `bash -e {0} "/usr/bin/${tool}"`]) {
        const root = repository(t);
        setWorkflowShell(root, scope, JSON.stringify(shell));
        assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported shell: value mentions a tool word/);
      }
    }
  }
});

test('pins: inherited standard shells retain run command checks', t => {
  for (const scope of ['job defaults', 'workflow defaults']) {
    const root = repository(t);
    setWorkflowShell(root, scope, 'bash');
    replace(root, workflow, 'cargo test --locked -p core', 'cargo test -p core');
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /cargo test requires --locked/);
  }
});

test('pins: mount FROM=busybox:latest requires an image digest', t => {
  const root = repository(t);
  const file = path.join(root, 'Dockerfile');
  writeFileSync(file, readFileSync(file, 'utf8') + '\nRUN --mount=type=bind,FROM=busybox:latest,target=/mnt true\n');
  assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned by sha256 digest/);
});

test('pins: mount keys are case insensitive for external images stages and ARGs', t => {
  for (const key of ['from', 'FROM', 'From', 'fRoM']) {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `RUN --mount=TYPE=bind,${key}=busybox:latest,TARGET=/mnt true`);
    assert.match(invoke('check-pins', ['--root', root], 1).stderr, /pinned by sha256 digest/);
    replace(root, 'Dockerfile', 'busybox:latest', 'busybox:1.37@sha256:' + 'e'.repeat(64));
    invoke('check-pins', ['--root', root], 0);
    replace(root, 'Dockerfile', 'busybox:1.37@sha256:' + 'e'.repeat(64), 'base');
    invoke('check-pins', ['--root', root], 0);
    replace(root, 'Dockerfile', `${key}=base`, `${key}=0`);
    invoke('check-pins', ['--root', root], 0);
    replace(root, 'Dockerfile', `${key}=0`, key + '=${NODE_BASE}');
    invoke('check-pins', ['--root', root], 0);
  }
});

test('pins: unknown duplicate and malformed mount keys fail closed', t => {
  for (const fields of [
    'type=bind,Fromm=busybox:latest,target=/mnt',
    'type=bind,from=base,FROM=busybox:latest,target=/mnt',
    'type=bind,FROM=busybox:latest,from=base,target=/mnt',
    'type=bind,from=base,From=base,target=/mnt',
    'type=bind,TYPE=cache,from=base,target=/mnt',
    'type=bind,from=base,target=/mnt,TARGET=/other',
    'type=bind,from,target=/mnt',
    'type=bind,FROM=,target=/mnt',
    'type=bind,FROM="",target=/mnt',
    'type=bind,,target=/mnt',
  ]) {
    const root = repository(t);
    replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `RUN --mount=${fields} true`);
    assert.match(invoke('check-pins', ['--root', root], 2).stderr, /unsupported mount (?:key|source)/);
  }
});

test('pins: supported mount fields and mixed case quoted sources pass', t => {
  const root = repository(t);
  replace(root, 'Dockerfile', 'RUN cargo build --release --locked -p core', `RUN --mount=TYPE=cache,ID=cargo,TARGET=/cache,SHARING=locked,UID=1000,GID=1000,MODE=0755,rw true
RUN --mount='TYPE=bind,From="base",TARGET=/mnt,ro' true
RUN --mount=type=secret,id=token,target=/run/token,required,env=TOKEN true
RUN --mount=type=tmpfs,target=/tmp,size=1024 true`);
  invoke('check-pins', ['--root', root], 0);
});
