import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { cli, json, read, required, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';
import { contract, environmentNames, externalOutput, fail, hash, linux, selection, windows, windowsPins } from './protocol-contract.mjs';
import { cargoStream, parsePackages, permissionCandidate, testTranscript, toolCalls, unusedTools } from './protocol-evidence.mjs';
import { captureFile, fileDigest, textLines } from './protocol-io.mjs';
import { diagnosticLog, jsonRecords } from './protocol-diagnostics.mjs';
import { verifySource } from './protocol-source.mjs';
import { verifyWindows, verifyWindowsVersion } from './protocol-windows.mjs';

const save = (directory, name, value) => writeFileSync(path.join(directory, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
function output(command, args, cwd, env) {
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', maxBuffer: 1024 * 1024 });
  if (result.error || result.signal || result.status !== 0) fail(`${command} ${args.join(' ')} failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}
function executable(command, cwd, env) {
  const candidate = path.isAbsolute(command) ? command : env.PATH.split(path.delimiter).map(directory => path.join(directory, command)).find(file => existsSync(file));
  if (!candidate) fail(`tool is not available: ${command}`);
  const resolved = realpathSync(candidate);
  return { command, invocationPath: path.resolve(candidate), path: resolved, version: output(candidate, ['--version'], cwd, env).trim() };
}

async function captureRun(options, started, report) {
  required(options, 'root', 'contract', 'profile', 'target', 'run', 'out', 'source-sha', 'source-receipt');
  if (!/^[a-f0-9]{40}$/.test(options['source-sha'])) fail('source-sha must be the exported commit');
  const root = realpathSync(options.root), data = contract(options.contract), out = externalOutput(root, options.out);
  const source = verifySource(root, json(options['source-receipt']), options['source-sha'], options['negative-case'] ?? null);
  if (existsSync(out)) fail('results directory must be fresh');
  const targetDir = path.join(out, 'target');
  const args = selection(data, options.profile, options.target, options.run, targetDir);
  const env = { ...process.env, CC_ENABLE_DEBUG_OUTPUT: '1' };
  for (const name of ['buf', 'protoc', 'protoc-gen-buffa', 'protoc-gen-buffa-packaging', 'protoc-gen-connect-rust', 'protoc-gen-es']) {
    if (env.PATH.split(path.delimiter).some(directory => existsSync(path.join(directory, name)))) fail(`generator executable visible in product build PATH: ${name}`);
  }
  for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'CCACHE', 'COMPILER_PATH', 'GCC_EXEC_PREFIX', 'LD_PRELOAD', 'LD_LIBRARY_PATH']) {
    if (env[name]) fail(`ambient ${name} would change the captured selection`);
  }
  const rustup = executable('rustup', root, env);
  const tools = { version: 1, executables: {}, windows: null, calls: [], unused: [] };
  for (const name of ['cargo', 'rustc', 'rustdoc', 'clippy-driver']) {
    const location = output(rustup.path, ['which', name], root, env).trim();
    tools.executables[name] = executable(location, root, env);
  }
  const channel = /channel\s*=\s*"([^"]+)"/.exec(read(path.join(root, 'rust-toolchain.toml')))?.[1];
  for (const name of ['cargo', 'rustc']) if (!tools.executables[name].version.startsWith(`${name} ${channel} `)) fail(`unexpected ${name} toolchain`);
  for (const [role, command] of Object.entries({ hostCC: env.HOST_CC ?? env.CC ?? 'cc', hostCXX: env.HOST_CXX ?? env.CXX ?? 'c++', hostAR: env.HOST_AR ?? env.AR ?? 'ar' })) {
    tools.executables[role] = executable(command, root, env);
  }
  tools.executables.hostLinker = tools.executables.hostCC;
  if (options.target === windows) {
    required(options, 'windows-tools');
    const receipt = json(path.join(options['windows-tools'], 'installation.json'));
    if (JSON.stringify(receipt.pins) !== JSON.stringify(windowsPins(root)) || realpathSync(receipt.root) !== realpathSync(options['windows-tools'])) fail('Windows installation receipt differs from shared pins');
    tools.windows = receipt;
    verifyWindows(receipt, windowsPins(root));
    for (const [role, name] of Object.entries({ targetCC: 'x86_64-w64-mingw32-clang', targetCXX: 'x86_64-w64-mingw32-clang++', targetAR: 'llvm-ar', targetLinker: 'x86_64-w64-mingw32-clang' })) {
      tools.executables[role] = executable(path.join(receipt.root, 'bin', name), root, env);
      verifyWindowsVersion(tools.executables[role].version, windowsPins(root));
      if (hash(readFileSync(tools.executables[role].path)) !== receipt.binaries[name]) fail(`installed Windows tool differs: ${name}`);
    }
    env.CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS = data.targets[windows].rustflags;
  } else {
    for (const role of ['CC', 'CXX', 'AR', 'Linker']) tools.executables[`target${role}`] = tools.executables[`host${role}`];
    if (env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS) fail('unexpected Linux Rust flags');
  }
  mkdirSync(out, { recursive: true });
  started(out);
  writeFileSync(path.join(out, 'linker.jsonl'), '', { flag: 'wx' });
  writeFileSync(path.join(out, 'compiler.jsonl'), '', { flag: 'wx' });
  tools.launchers = {};
  for (const context of ['host', 'target']) {
    const launcher = path.join(out, `${context}-linker.mjs`);
    writeFileSync(launcher, readFileSync(path.join(root, 'tools/ci/record-linker.mjs')), { mode: 0o755, flag: 'wx' });
    save(out, `${context}-linker.mjs.json`, { context, executable: tools.executables[`${context}Linker`].invocationPath, target: options.target, log: path.join(out, 'linker.jsonl') });
    for (const role of ['CC', 'CXX', 'AR']) {
      const key = `${context}${role}`, launcher = path.join(out, `${key}.mjs`);
      writeFileSync(launcher, readFileSync(path.join(root, 'tools/ci/record-compiler.mjs')), { mode: 0o755, flag: 'wx' });
      save(out, `${key}.mjs.json`, { role: key, executable: tools.executables[key].invocationPath, target: options.target, log: path.join(out, 'compiler.jsonl') });
      tools.launchers[key] = launcher;
      if (context === 'host') { env[`HOST_${role}`] = launcher; env[role] = launcher; }
      else env[`${role}_${options.target.replaceAll('-', '_')}`] = launcher;
    }
  }
  env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = path.join(out, options.target === linux ? 'target-linker.mjs' : 'host-linker.mjs');
  if (options.target === windows) env.CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER = path.join(out, 'target-linker.mjs');
  env.RUSTC = tools.executables.rustc.path;
  env.RUSTDOC = tools.executables.rustdoc.path;
  const invocation = { version: 2, source, profile: options.profile, target: options.target, run: options.run,
    sourceSha: options['source-sha'], lockSha256: hash(readFileSync(path.join(root, 'Cargo.lock'))), cwd: root,
    buildProfile: 'dev', targetDir, arguments: args,
    environment: Object.fromEntries(environmentNames.map(name => [name, env[name] ?? null])) };
  await report.stage('metadata.json', () => captureFile(tools.executables.cargo.path, args.metadata, root, env, path.join(out, 'metadata.json')));
  const metadata = report.attempt('metadata.json', 1, () => cargoMetadata(path.join(out, 'metadata.json'), { targets: true }));
  if (metadata && metadata.workspace_root !== root) report.add('metadata.json', 1, 'metadata workspace root differs from capture cwd');
  invocation.rootIds = data.profiles[options.profile].roots.map(name => {
    return report.attempt('metadata.json', 1, () => {
      const matches = metadata?.packages.filter(pkg => pkg.name === name && metadata.workspace_members.includes(pkg.id)) ?? [];
      if (matches.length !== 1) fail(`root package is missing or ambiguous: ${name}`);
      return matches[0].id;
    });
  }).filter(Boolean);
  invocation.treeFiles = { 'packages.tree': await report.stage('packages.tree', () =>
    captureFile(tools.executables.cargo.path, args.packages, root, env, path.join(out, 'packages.tree'))) };
  // Always reach the actual build and recorders, even if list analysis fails.
  const stdout = openSync(path.join(out, 'cargo.stdout'), 'wx'), stderr = openSync(path.join(out, 'build.log'), 'wx');
  let result;
  try { result = spawnSync(tools.executables.cargo.path, args.cargo, { cwd: root, env, stdio: ['ignore', stdout, stderr] }); }
  finally { closeSync(stdout); closeSync(stderr); }
  if (result.error || result.signal || result.status !== 0) report.add('build.log', 1, `captured Cargo run failed: ${result.error?.message ?? result.signal ?? result.status}`);
  const graph = metadata ? report.attempt('packages.tree', 1, () => parsePackages(textLines(path.join(out, 'packages.tree')), metadata, invocation, report)) : undefined;
  if (graph) save(out, 'graph.json', graph);
  const stream = cargoStream(read(path.join(out, 'cargo.stdout')), report);
  writeFileSync(path.join(out, 'cargo.jsonl'), stream.jsonl, { flag: 'wx' });
  // Cargo writes suite headers on stderr and test bodies on stdout. Keep the
  // raw streams, and merge only the ordered suite records for the existing counter.
  const binaries = stream.records.filter(record => record.reason === 'compiler-artifact' && record.executable).map(record => record.executable);
  if (options.run === 'test') {
    const text = report.attempt('build.log', 1, () => testTranscript(read(path.join(out, 'build.log')), stream));
    if (text !== undefined) writeFileSync(path.join(out, 'tests.txt'), text, { flag: 'wx' });
  }
  tools.compilerCalls = jsonRecords(read(path.join(out, 'compiler.jsonl')), 'compiler.jsonl', report);
  tools.calls = metadata ? toolCalls(read(path.join(out, 'build.log')), read(path.join(out, 'cargo.stdout')), tools, metadata, root, options.target, binaries, report) : [];
  tools.linkerCalls = jsonRecords(read(path.join(out, 'linker.jsonl')), 'linker.jsonl', report);
  for (const [index, call] of tools.linkerCalls.entries()) report.attempt('linker.jsonl', index + 1, () => {
    if (!['host', 'target'].includes(call.context) || !path.isAbsolute(call.cwd ?? '') || !Array.isArray(call.args) ||
        call.launcher !== path.join(out, `${call.context}-linker.mjs`) ||
        call.executable !== tools.executables[`${call.context}Linker`]?.path || call.command !== tools.executables[`${call.context}Linker`]?.invocationPath ||
        call.status !== 0 || call.signal || call.error) fail('foreign, incomplete or failed actual linker invocation');
  }, JSON.stringify(call));
  tools.unused = unusedTools(tools, tools.calls, tools.linkerCalls);
  save(out, 'tools.json', tools);
  invocation.evidence = Object.fromEntries(['metadata.json', 'packages.tree', 'graph.json', 'cargo.stdout', 'cargo.jsonl', 'build.log', 'tools.json', 'linker.jsonl', 'compiler.jsonl',
    ...(options.run === 'test' ? ['tests.txt'] : [])].map(name => [name, report.attempt(name, 1, () => fileDigest(path.join(out, name)).sha256)]));
  save(out, 'invocation.json', invocation);
  verifySource(root, source, options['source-sha'], options['negative-case'] ?? null);
  report.finish();
  save(out, 'permission-candidate.json', permissionCandidate(graph, metadata, invocation));
  return [];
}

export async function capture(options) {
  let owned;
  const report = diagnosticLog();
  const status = value => writeFileSync(path.join(owned, 'capture-status.json'), JSON.stringify({ version: 1, ...value }, null, 2) + '\n');
  const diagnostics = () => writeFileSync(path.join(owned, 'diagnostics.json'), JSON.stringify({ version: 1, errors: report.errors }, null, 2) + '\n');
  try {
    const result = await captureRun(options, directory => { owned = directory; status({ status: 'running' }); diagnostics(); }, report);
    diagnostics();
    status({ status: 'complete' });
    return result;
  } catch (error) {
    if (owned) {
      if (!error.diagnosticsCollected) report.add('capture', 1, error);
      diagnostics();
      status({ status: 'failed', message: 'Capture failed; inspect diagnostics.json and the retained raw evidence.' });
    }
    throw error;
  }
}

await cli(import.meta.url, `Usage: node tools/ci/capture-protocol-graph.mjs --root DIR --contract FILE --profile NAME --target TARGET --run build|test|clippy --source-sha SHA --source-receipt FILE --out DIR [--windows-tools DIR] [--negative-case NAME]
Start metadata identities, a flat package/feature list and the actual Cargo run from one selection with fresh targets.
Retain raw streams, actual C commands and delegated linker invocations. Set CC_ENABLE_DEBUG_OUTPUT=1.
Collect independent input errors with raw source lines in diagnostics.json, retaining failed captures.
This captures evidence only; it does not approve observed features or write the contract.
Exit: 0 captured, 2 usage, tool or evidence error.`, {
  root: stringOption, contract: stringOption, profile: stringOption, target: stringOption, run: stringOption,
  'source-sha': stringOption, out: stringOption, 'windows-tools': stringOption, 'source-receipt': stringOption, 'negative-case': stringOption,
}, capture);
