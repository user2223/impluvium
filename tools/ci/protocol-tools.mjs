import path from 'node:path';
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { cli, json, read, required, stringOption } from './common.mjs';
import { yamlData } from './pnpm-licences.mjs';
import { externalOutput, fail, hash, windowsPins } from './protocol-contract.mjs';
import { windowsFiles } from './protocol-windows.mjs';

export function toolContract(root) {
  const mapping = json(path.join(root, 'tools/ci/codegen-tools.json'));
  const env = yamlData(path.join(root, '.gitea/workflows/ci.yml')).env;
  if (mapping.version !== 1 || mapping.pinSource !== '.gitea/workflows/ci.yml' || !Array.isArray(mapping.tools)) fail('invalid generator source mapping');
  const expected = ['buf', 'protoc-gen-buffa', 'protoc-gen-buffa-packaging', 'protoc-gen-connect-rust', 'protoc-gen-es'];
  if (JSON.stringify(mapping.tools.map(tool => tool.binary)) !== JSON.stringify(expected)) fail('missing, additional or reordered generator mapping');
  for (const tool of mapping.tools) {
    if (tool.binary === 'protoc-gen-es') {
      if (tool.package !== '@bufbuild/protoc-gen-es' || tool.versionSource !== 'engine/protocol/ts/package.json#/devDependencies/@bufbuild~1protoc-gen-es' || tool.integritySource !== 'pnpm-lock.yaml' || tool.binDirectory !== 'engine/protocol/ts/node_modules/.bin' || tool.installFilter !== '{./engine/protocol/ts}...') fail('wrong local TypeScript generator mapping');
      const manifest = json(path.join(root, 'engine/protocol/ts/package.json'));
      if (!/^\d+\.\d+\.\d+$/.test(manifest.devDependencies?.[tool.package] ?? '')) fail('TypeScript generator must be pinned exactly');
      continue;
    }
    const version = env[tool.versionPin], checksum = env[tool.sha256Pin];
    if (!/^\d+\.\d+\.\d+$/.test(version ?? '') || !/^[a-f0-9]{64}$/.test(checksum ?? '')) fail(`invalid central pin for ${tool.binary}`);
    const url = tool.url.replaceAll('${version}', version);
    const wanted = tool.crate ? `https://static.crates.io/crates/${tool.crate}/${tool.crate}-${version}.crate` : `https://github.com/bufbuild/buf/releases/download/v${version}/buf-Linux-x86_64`;
    if (url !== wanted) fail(`unexpected source for ${tool.binary}`);
    if (tool.crate) {
      if (tool.lock !== `tools/ci/generator-locks/${tool.crate}.Cargo.lock` || !/^[a-f0-9]{64}$/.test(tool.lockSha256 ?? '') || hash(readFileSync(path.join(root, tool.lock))) !== tool.lockSha256) fail(`missing or changed frozen generator lock for ${tool.binary}`);
    }
  }
  return { mapping, env };
}

// Exported operations make failure ordering testable without downloads or builds.
export function checkedArchive(tool, ops) {
  ops.download();
  if (ops.archiveHash() !== tool.sha256) fail(`archive checksum differs: ${tool.binary}`);
  if (!tool.crate) { ops.install(); return; }
  ops.extract();
  if (!ops.sameLock()) fail(`extracted generator lock differs: ${tool.binary}`);
  ops.fetch();
  ops.build();
  ops.install();
}

function command(program, args, cwd) {
  const result = spawnSync(program, args, { cwd, stdio: 'inherit' });
  if (result.error || result.signal || result.status !== 0) fail(`${program} failed (${result.status})`);
}
export function bootstrap(options) {
  required(options, 'root', 'work', 'kind');
  const root = realpathSync(options.root), work = path.resolve(options.work);
  if (!['codegen', 'windows'].includes(options.kind)) fail('kind must be codegen or windows');
  const downloads = path.join(work, 'downloads'), sources = path.join(work, 'tools-src'), bin = path.join(work, 'tools/bin');
  for (const directory of [downloads, sources, bin]) externalOutput(root, directory);
  for (const directory of [downloads, sources, bin]) mkdirSync(directory, { recursive: true });
  if (options.kind === 'windows') {
    const pins = windowsPins(root), archive = path.join(downloads, 'llvm-mingw.tar.xz'), destination = path.join(work, 'llvm-mingw');
    if (existsSync(destination)) fail('Windows installation directory must be fresh');
    command('curl', ['-fsSLo', archive, pins.LLVM_MINGW_URL], root);
    if (hash(readFileSync(archive)) !== pins.LLVM_MINGW_SHA256) fail('Windows archive checksum differs');
    mkdirSync(destination);
    command('tar', ['-xJf', archive, '--strip-components=1', '-C', destination], root);
    const binaries = Object.fromEntries(['x86_64-w64-mingw32-clang', 'x86_64-w64-mingw32-clang++', 'llvm-ar'].map(name => [name, hash(readFileSync(path.join(destination, 'bin', name)))]));
    writeFileSync(path.join(destination, 'installation.json'), JSON.stringify({ version: 2, root: destination, pins, binaries, files: windowsFiles(destination) }, null, 2) + '\n', { flag: 'wx' });
    return [];
  }
  const { mapping, env } = toolContract(root);
  for (const item of mapping.tools.filter(tool => tool.binary !== 'protoc-gen-es')) {
    const version = env[item.versionPin], archive = path.join(downloads, `${item.crate ?? 'buf'}-${version}${item.crate ? '.crate' : ''}`);
    const source = path.join(sources, `${item.crate}-${version}`);
    const manifest = path.join(source, 'Cargo.toml');
    const tool = { ...item, sha256: env[item.sha256Pin] };
    checkedArchive(tool, {
      download: () => command('curl', ['-fsSLo', archive, item.url.replaceAll('${version}', version)], root),
      archiveHash: () => hash(readFileSync(archive)),
      extract: () => {
        if (existsSync(source)) fail('generator source directory must be fresh');
        command('tar', ['-xzf', archive, '-C', sources], root);
      },
      sameLock: () => readFileSync(path.join(source, 'Cargo.lock')).equals(readFileSync(path.join(root, item.lock))),
      fetch: () => command('cargo', ['fetch', '--locked', '--manifest-path', manifest], root),
      build: () => command('cargo', ['build', '--release', '--locked', '--offline', '--manifest-path', manifest], root),
      install: () => {
        const destination = path.join(bin, item.binary);
        copyFileSync(item.crate ? path.join(source, 'target/release', item.binary) : archive, destination);
        chmodSync(destination, 0o755);
      },
    });
  }
  return [];
}
await cli(import.meta.url, `Usage: node tools/ci/protocol-tools.mjs --root DIR --work DIR --kind codegen|windows
Prepare checksum-verified tools in a disposable container. Read central pins and compare frozen locks before fetch/build.
Never called by a product build or by xtask. Does not change repository outputs or locks.
Exit: 0 prepared, 2 usage, integrity or tool error.`, {
  root: stringOption, work: stringOption, kind: stringOption,
}, bootstrap);
