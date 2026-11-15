import path from 'node:path';
import { createHash } from 'node:crypto';
import { existsSync, realpathSync } from 'node:fs';
import { InputError, json, read } from './common.mjs';
import { expandContract } from './protocol-data.mjs';

export const linux = 'x86_64-unknown-linux-gnu';
export const windows = 'x86_64-pc-windows-gnullvm';
export const runs = ['build', 'test', 'clippy'];
export const matrixSelections = ['core', 'server', 'client', 'combined'].flatMap(profile => [
  ...runs.map(run => ({ profile, target: linux, run })), { profile, target: windows, run: 'build' },
]);
export const environmentNames = [
  'PATH', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'RUSTC', 'RUSTDOC',
  'CC', 'CXX', 'AR', 'HOST_CC', 'HOST_CXX', 'HOST_AR', 'TARGET_CC', 'TARGET_CXX', 'TARGET_AR',
  'CC_x86_64_pc_windows_gnullvm', 'CXX_x86_64_pc_windows_gnullvm', 'AR_x86_64_pc_windows_gnullvm',
  'CC_x86_64_unknown_linux_gnu', 'CXX_x86_64_unknown_linux_gnu', 'AR_x86_64_unknown_linux_gnu',
  'CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER', 'CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS',
  'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER', 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS',
  'CC_ENABLE_DEBUG_OUTPUT',
  'CCACHE', 'COMPILER_PATH', 'GCC_EXEC_PREFIX', 'LD_PRELOAD', 'LD_LIBRARY_PATH',
];
export const hash = value => createHash('sha256').update(value).digest('hex');
export const equal = (left, right) => JSON.stringify(left) === JSON.stringify(right);
export const fail = message => { throw new InputError(`<protocol-graph>:1: ${message}`); };
export function externalOutput(root, destination) {
  let ancestor = path.resolve(destination);
  const suffix = [];
  while (!existsSync(ancestor)) { suffix.unshift(path.basename(ancestor)); ancestor = path.dirname(ancestor); }
  const resolved = path.join(realpathSync(ancestor), ...suffix), relative = path.relative(realpathSync(root), resolved);
  if (!path.isAbsolute(relative) && relative !== '..' && !relative.startsWith('..' + path.sep)) fail('results and tool directories must be outside the source export');
  return resolved;
}
export function contract(file) {
  const data = expandContract(json(file));
  if (data.version !== 3 || data.representation !== 'package-features' || !equal(Object.keys(data.profiles ?? {}).sort(), ['client', 'combined', 'core', 'server']) ||
      !equal(Object.keys(data.targets ?? {}).sort(), [windows, linux].sort()) || !Array.isArray(data.permissions)) fail('incomplete profile/target contract');
  const expected = {
    core: { roots: ['impluvium-protocol'], features: [] },
    server: { roots: ['impluvium-protocol'], features: ['impluvium-protocol/server'] },
    client: { roots: ['impluvium-client'], features: [] },
    combined: { roots: ['impluvium-protocol', 'impluvium-client'], features: ['impluvium-protocol/server'] },
  };
  for (const name of Object.keys(expected)) if (!equal(data.profiles[name].roots, expected[name].roots) || !equal(data.profiles[name].features, expected[name].features)) fail('profile selections differ from the product contract');
  if (data.targets[windows].pins !== 'tools/ci/versions.env' || data.targets[windows].rustflags !== '-C target-feature=+crt-static') fail('wrong Windows tool/CRT contract');
  return data;
}
export function selection(data, profile, target, run, targetDir) {
  if (!data.profiles[profile] || !data.targets[target] || !runs.includes(run)) fail('unknown profile, target or run');
  const config = data.profiles[profile];
  const flags = ['--no-default-features', ...config.roots.flatMap(name => ['-p', name]), ...config.features.flatMap(feature => ['--features', feature])];
  const common = ['--locked', '--offline'];
  const scope = run === 'build' ? 'normal,build' : 'normal,build,dev';
  return {
    // Metadata supplies identities only. A target filter would hide host-build
    // dependencies during cross-compilation; the selected tree supplies scope.
    metadata: ['metadata', ...common, '--format-version=1', '--no-default-features', ...config.features.flatMap(feature => ['--features', feature])],
    packages: ['tree', ...common, '--target', target, ...flags, '--no-dedupe', '--prefix', 'none', '--edges', scope, '--format', '{p}|{f}'],
    cargo: [run, ...common, '-vv', '--target', target, '--target-dir', targetDir, ...flags, '--message-format=json',
      ...(run === 'test' ? ['--no-fail-fast'] : run === 'clippy' ? ['--all-targets', '--', '--cap-lints', 'warn'] : [])],
  };
}
export function windowsPins(root) {
  const values = {};
  for (const line of read(path.join(root, 'tools/ci/versions.env')).split('\n')) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    const match = /^(LLVM_MINGW_(?:VERSION|URL|SHA256|CLANG_VERSION))=(\S+)$/.exec(line);
    if (!match || values[match[1]]) fail('invalid shared Windows pins');
    values[match[1]] = match[2];
  }
  if (!/^\d+\.\d+\.\d+$/.test(values.LLVM_MINGW_CLANG_VERSION ?? '') || !/^\d{8}$/.test(values.LLVM_MINGW_VERSION ?? '') || !/^[a-f0-9]{64}$/.test(values.LLVM_MINGW_SHA256 ?? '') ||
      values.LLVM_MINGW_URL !== `https://github.com/mstorsjo/llvm-mingw/releases/download/${values.LLVM_MINGW_VERSION}/llvm-mingw-${values.LLVM_MINGW_VERSION}-ucrt-ubuntu-22.04-x86_64.tar.xz`) fail('invalid Windows version, URL or checksum');
  return values;
}
