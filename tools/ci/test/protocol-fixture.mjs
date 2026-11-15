import path from 'node:path';
import { cpSync, mkdirSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { contract, environmentNames, hash, linux, selection, windows, windowsPins } from '../protocol-contract.mjs';
import { parsePackages, permissionCandidate, toolCalls, unusedTools } from '../protocol-evidence.mjs';
import { fileDigest } from '../protocol-io.mjs';
import { windowsFiles } from '../protocol-windows.mjs';
import { attest } from './protocol-source-fixture.mjs';

export function evidence(t, target = linux) {
  const root = temporary(t), input = temporary(t), targetDir = path.join(input, 'target');
  const put = (file, value) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, value); };
  const save = (name, data) => put(path.join(input, name), JSON.stringify(data, null, 2) + '\n');
  put(path.join(root, 'Cargo.lock'), '# synthetic test input\n');
  put(path.join(root, 'engine/crates/protocol/Cargo.toml'), '[package]\nname = "impluvium-protocol"\nversion = "0.1.0"\n');
  put(path.join(root, 'engine/crates/protocol/src/lib.rs'), 'pub fn example() {}\n');
  put(path.join(root, 'rust-toolchain.toml'), '[toolchain]\nchannel = "1.99.1"\n');
  cpSync(new URL('../record-linker.mjs', import.meta.url), path.join(root, 'record-linker-source'));
  mkdirSync(path.join(root, 'tools/ci'), { recursive: true });
  cpSync(path.join(root, 'record-linker-source'), path.join(root, 'tools/ci/record-linker.mjs'));
  cpSync(new URL('../record-compiler.mjs', import.meta.url), path.join(root, 'tools/ci/record-compiler.mjs'));
  const data = contract(new URL('../protocol-graph.json', import.meta.url));
  const packages = [
    ['root', 'impluvium-protocol', '0.1.0'], ['buffa', 'buffa', '0.9.2'], ['types', 'buffa-types', '0.9.2'],
    ['sha', 'sha2', '0.11.1'], ['blake', 'blake3', '1.8.7'], ['cc', 'cc', '1.10.0'],
  ].map(([id, name, version]) => ({ id, name, version,
    source: id === 'root' ? null : 'registry+https://github.com/rust-lang/crates.io-index',
    manifest_path: id === 'root' ? path.join(root, 'engine/crates/protocol/Cargo.toml') : path.join(root, '.registry', name, 'Cargo.toml'),
    targets: [{ name: name.replaceAll('-', '_'), kind: ['lib'] }], dependencies: id === 'blake' ? [{ name: 'cc', kind: 'build' }] : [], links: null }));
  const dep = (id, kind = null) => ({ name: id, pkg: id, dep_kinds: [{ kind, target: null }] });
  const metadata = { version: 1, workspace_root: root, workspace_members: ['root'], packages,
    resolve: { nodes: packages.map(pkg => ({ id: pkg.id, features: [], dependencies: pkg.id === 'root' ? ['buffa', 'types', 'sha', 'blake'] : pkg.id === 'blake' ? ['cc'] : [],
      deps: pkg.id === 'root' ? ['buffa', 'types', 'sha', 'blake'].map(id => dep(id)) : pkg.id === 'blake' ? [dep('cc', 'build')] : [] })) } };
  const packageText = `impluvium-protocol v0.1.0 (${path.join(root, 'engine/crates/protocol')})|\nbuffa v0.9.2|json,std\nbuffa-types v0.9.2|json,std\nsha2 v0.11.1|\nblake3 v1.8.7|std\ncc v1.10.0|\n`;
  const invocation = { version: 2, profile: 'core', target, run: 'build', sourceSha: 'a'.repeat(40), cwd: root,
    buildProfile: 'dev', targetDir, lockSha256: hash(readFileSync(path.join(root, 'Cargo.lock'))),
    rootIds: ['root'], arguments: selection(data, 'core', target, 'build', targetDir),
    environment: { ...Object.fromEntries(environmentNames.map(name => [name, null])), PATH: '/synthetic/tools', CC_ENABLE_DEBUG_OUTPUT: '1' } };
  const graph = parsePackages(packageText, metadata, invocation);
  const tools = { version: 1, executables: {}, windows: null };
  for (const role of ['cargo', 'rustc', 'rustdoc', 'clippy-driver', 'hostCC', 'hostCXX', 'hostAR', 'hostLinker', 'targetCC', 'targetCXX', 'targetAR', 'targetLinker']) {
    const location = path.join(input, 'tools', role);
    put(location, `synthetic executable ${role}\n`);
    tools.executables[role] = { command: location, invocationPath: location, path: location, version: `${role} 1.99.1 (synthetic)` };
  }
  if (target === windows) {
    cpSync(new URL('../versions.env', import.meta.url), path.join(root, 'tools/ci/versions.env'));
    const installation = path.join(input, 'llvm-mingw'), binaries = {};
    // Source: the pinned archive's real alias chain uses a shell wrapper,
    // bin/clang -> clang-23 and ld.lld -> lld, not a direct compiler alias.
    for (const file of ['clang-target-wrapper.sh', 'clang-23', 'lld', 'llvm-ar']) put(path.join(installation, 'bin', file), file);
    symlinkSync('clang-23', path.join(installation, 'bin/clang'));
    symlinkSync('lld', path.join(installation, 'bin/ld.lld'));
    for (const [role, name] of Object.entries({ targetCC: 'x86_64-w64-mingw32-clang', targetCXX: 'x86_64-w64-mingw32-clang++', targetAR: 'llvm-ar', targetLinker: 'x86_64-w64-mingw32-clang' })) {
      const file = path.join(installation, 'bin', name);
      if (name !== 'llvm-ar' && !binaries[name]) symlinkSync('clang-target-wrapper.sh', file);
      tools.executables[role] = { command: file, invocationPath: file, path: realpathSync(file), version: 'LLVM version 23.1.4' };
      binaries[name] = hash(readFileSync(file));
    }
    tools.windows = { version: 2, root: installation, pins: windowsPins(root), binaries, files: windowsFiles(installation) };
    Object.assign(invocation.environment, {
      CC_x86_64_pc_windows_gnullvm: tools.executables.targetCC.path,
      CXX_x86_64_pc_windows_gnullvm: tools.executables.targetCXX.path,
      AR_x86_64_pc_windows_gnullvm: tools.executables.targetAR.path,
      CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS: '-C target-feature=+crt-static',
      CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER: path.join(input, 'target-linker.mjs'),
    });
  }
  tools.launchers = {};
  for (const context of ['host', 'target']) for (const role of ['CC', 'CXX', 'AR']) {
    const key = `${context}${role}`, launcher = path.join(input, `${key}.mjs`);
    put(launcher, readFileSync(path.join(root, 'tools/ci/record-compiler.mjs')));
    save(`${key}.mjs.json`, { role: key, executable: tools.executables[key].invocationPath, target, log: path.join(input, 'compiler.jsonl') });
    tools.launchers[key] = launcher;
    if (context === 'host') { invocation.environment[`HOST_${role}`] = launcher; invocation.environment[role] = launcher; }
    else invocation.environment[`${role}_${target.replaceAll('-', '_')}`] = launcher;
  }
  const object = path.join(targetDir, target, 'debug/build/blake3/out/hash.o');
  tools.compilerCalls = [
    ['targetCC', ['-c', 'source.c', '-o', object]], ['targetAR', ['cq', 'libhash.a', object]],
  ].map(([role, args]) => ({ launcher: tools.launchers[role], executable: tools.executables[role].path,
    command: tools.executables[role].invocationPath, args, cwd: path.dirname(metadata.packages.find(pkg => pkg.id === 'blake').manifest_path),
    context: 'target', role, packageName: 'blake3', packageVersion: '1.8.7', manifestDir: path.dirname(metadata.packages.find(pkg => pkg.id === 'blake').manifest_path), status: 0, signal: null, error: null }));
  const log = `Running \`${tools.executables.rustc.path} --target ${target} ${target === windows ? '-C target-feature=+crt-static ' : ''}--out-dir ${path.join(targetDir, target, 'debug/deps')}\`\n` +
    `[blake3 1.8.7] running: "${tools.launchers.targetCC}" "-c" "source.c" "-o" "${object}"\n[blake3 1.8.7] exit status: 0\n` +
    `[blake3 1.8.7] running: "${tools.launchers.targetAR}" "cq" "libhash.a" "${object}"\n[blake3 1.8.7] exit status: 0\n`;
  const artifacts = graph.nodes.map(node => ({ reason: 'compiler-artifact', package_id: node.id, target: { kind: ['lib'] }, features: node.features,
    filenames: [path.join(targetDir, ...(node.id === 'cc' ? [] : [target]), 'debug/deps', node.id + '.rlib')], executable: null }));
  const stdout = [...artifacts, { reason: 'build-finished', success: true }].map(item => JSON.stringify(item)).join('\n') + '\n';
  tools.calls = toolCalls(log, stdout, tools, metadata, root, target);
  tools.linkerCalls = [{ executable: tools.executables.targetLinker.path, command: tools.executables.targetLinker.invocationPath, args: ['-o', path.join(targetDir, target, 'debug/target')],
    cwd: root, launcher: path.join(input, 'target-linker.mjs'), context: 'target', status: 0, signal: null, error: null }];
  tools.unused = unusedTools(tools, tools.calls, tools.linkerCalls);
  for (const context of ['host', 'target']) {
    put(path.join(input, `${context}-linker.mjs`), readFileSync(path.join(root, 'tools/ci/record-linker.mjs')));
    save(`${context}-linker.mjs.json`, { context, executable: tools.executables[`${context}Linker`].invocationPath, log: path.join(input, 'linker.jsonl'), target });
  }
  Object.assign(invocation.environment, { RUSTC: tools.executables.rustc.path, RUSTDOC: tools.executables.rustdoc.path,
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER: path.join(input, target === windows ? 'host-linker.mjs' : 'target-linker.mjs') });
  save('metadata.json', metadata); save('graph.json', graph); save('tools.json', tools);
  save('capture-status.json', { version: 1, status: 'complete' });
  save('diagnostics.json', { version: 1, errors: [] });
  put(path.join(input, 'packages.tree'), packageText);
  put(path.join(input, 'cargo.stdout'), stdout); put(path.join(input, 'cargo.jsonl'), stdout); put(path.join(input, 'build.log'), log);
  put(path.join(input, 'linker.jsonl'), tools.linkerCalls.map(item => JSON.stringify(item)).join('\n') + '\n');
  put(path.join(input, 'compiler.jsonl'), tools.compilerCalls.map(item => JSON.stringify(item)).join('\n') + '\n');
  data.permissions = [permissionCandidate(graph, metadata, invocation)];
  const contractFile = path.join(input, 'contract.json');
  put(contractFile, JSON.stringify(data));
  invocation.source = attest(root); invocation.sourceSha = invocation.source.commit;
  const refresh = () => {
    invocation.treeFiles = { 'packages.tree': fileDigest(path.join(input, 'packages.tree')) };
    invocation.evidence = Object.fromEntries(['metadata.json', 'packages.tree', 'graph.json', 'cargo.stdout', 'cargo.jsonl', 'build.log', 'tools.json', 'linker.jsonl', 'compiler.jsonl'].map(name => [name, hash(readFileSync(path.join(input, name)))]));
    save('invocation.json', invocation);
  };
  refresh();
  return { root, input, data, graph, metadata, invocation, tools, contractFile, refresh, save,
    options: { root, input, contract: contractFile, 'source-sha': invocation.sourceSha } };
}

