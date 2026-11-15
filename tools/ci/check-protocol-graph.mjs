import path from 'node:path';
import { readFileSync, realpathSync } from 'node:fs';
import { cli, json, plainObject, read, required, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';
import { contract, environmentNames, equal, fail, hash, linux, selection, windows, windowsPins } from './protocol-contract.mjs';
import { cargoStream, parsePackages, permissionCandidate, testTranscript, toolCalls, unusedTools } from './protocol-evidence.mjs';
import { workspacePaths } from './protocol-workspace.mjs';
import { fileDigest, textLines } from './protocol-io.mjs';
import { verifySource } from './protocol-source.mjs';
import { verifyWindows, verifyWindowsVersion } from './protocol-windows.mjs';
import { requirementIssues, workspaceRequirements } from './protocol-requirements.mjs';

const forbidden = new Set(['ring', 'native-tls', 'openssl', 'openssl-sys', 'prost', 'prost-types', 'pbjson', 'tonic', 'flate2', 'zstd', 'zstd-sys']);
const forbiddenFeatures = {
  connectrpc: ['default', 'gzip', 'zstd', 'client-tls', 'tls', 'server-tls'],
  reqwest: ['default', 'system-proxy', 'native-tls', 'native-tls-vendored'],
  rustls: ['default', 'ring', 'custom-provider'],
  'hyper-rustls': ['default', 'ring', 'native-tokio', 'webpki-tokio', 'rustls-native-certs', 'webpki-roots'],
};
const sorted = values => [...values].sort();
const artifactContext = (artifact, invocation) => artifact.target?.kind?.some(kind => ['proc-macro', 'custom-build'].includes(kind)) ||
  !artifact.filenames?.some(file => file.startsWith(path.join(invocation.targetDir, invocation.target) + path.sep)) ? 'host' : 'target';
export function productDiagnostics(data, graph, metadata, invocation, artifacts) {
  const issues = [], packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  const add = (message, code = 'policy', details = {}) => issues.push({ code, ...details, message });
  const permissions = data.permissions.filter(entry => entry.profile === invocation.profile && entry.target === invocation.target && entry.run === invocation.run);
  if (permissions.length !== 1 || !Array.isArray(permissions[0].packages) || !permissions[0].packages.length) fail('profile permissions are empty, missing or ambiguous; review a real capture first');
  const allowed = permissions[0].packages;
  if (allowed.some(pkg => !plainObject(pkg) || Object.keys(pkg).some(key => !['name', 'version', 'source', 'features'].includes(key)) ||
      typeof pkg.name !== 'string' || !pkg.name || typeof pkg.version !== 'string' || !pkg.version ||
      !Object.hasOwn(pkg, 'source') || (pkg.source !== null && typeof pkg.source !== 'string') ||
      !Array.isArray(pkg.features) || !pkg.features.every(feature => typeof feature === 'string'))) fail('incomplete package/feature permission');
  const actual = permissionCandidate(graph, metadata, invocation).packages;
  const canonical = item => JSON.stringify({ name: item.name, version: item.version, source: item.source, features: sorted(item.features) });
  const permitted = new Set(allowed.map(canonical));
  if (actual.some(item => !permitted.has(canonical(item)))) add('resolved package/features are outside the reviewed permissions');
  const runtime = graph.nodes.filter(node => !artifacts || artifacts.some(artifact => artifact.package_id === node.id &&
    !artifact.target?.kind?.includes('custom-build') && artifactContext(artifact, invocation) === 'target' && equal(sorted(artifact.features), sorted(node.features))))
    .map(node => ({ name: packages.get(node.id)?.name, version: packages.get(node.id)?.version, features: node.features }));
  if (artifacts) for (const id of invocation.rootIds) {
    if (!artifacts.some(artifact => artifact.package_id === id &&
        !artifact.target?.kind?.some(kind => ['custom-build', 'proc-macro'].includes(kind)) &&
        artifactContext(artifact, invocation) === 'target' && graph.nodes.some(node => node.id === id && equal(sorted(node.features), sorted(artifact.features))))) add(`selected root has no target artifact: ${id}`);
  }
  for (const node of graph.nodes) {
    const pkg = packages.get(node.id);
    if (!pkg) fail(`unknown graph package ${node.id}`);
    if (forbidden.has(pkg.name)) add(`forbidden package ${pkg.name} reachable from engine root`, 'forbidden-package', { package: pkg.name });
    if (data.versions[pkg.name] && data.versions[pkg.name] !== pkg.version) add(`runtime pin differs: ${pkg.name} ${pkg.version}`);
    if (pkg.source == null && !path.relative(invocation.cwd, pkg.manifest_path).replaceAll(path.sep, '/').startsWith('engine/')) add(`repository dependency outside engine/: ${pkg.manifest_path}`);
    for (const feature of forbiddenFeatures[pkg.name] ?? []) if (node.features.includes(feature)) add(`forbidden feature ${pkg.name}/${feature}`, 'forbidden-feature', { package: pkg.name, feature });
    if (invocation.profile === 'core' && /^(?:connectrpc|reqwest|rustls|hyper|openssl|native-tls)/.test(pkg.name)) add(`HTTP/TLS package in core: ${pkg.name}`);
    if (pkg.name === 'rustls-native-certs') {
      if (!['client', 'combined'].includes(invocation.profile) || !actual.some(item => item.name === 'rustls-platform-verifier')) add('rustls-native-certs requires the client platform verifier');
    }
  }
  for (const name of ['buffa', 'buffa-types', 'sha2', 'blake3', ...(invocation.profile === 'core' ? [] : ['connectrpc']),
    ...(['client', 'combined'].includes(invocation.profile) ? ['reqwest', 'hyper-rustls', 'rustls', 'aws-lc-rs', 'aws-lc-sys', 'rustls-platform-verifier'] : [])]) {
    if (!runtime.some(pkg => pkg.name === name && pkg.version === data.versions[name])) add(`required runtime package missing: ${name}`);
  }
  const client = ['client', 'combined'].includes(invocation.profile), server = ['server', 'combined'].includes(invocation.profile);
  const requiredFeatures = {
    buffa: ['std', 'json'], 'buffa-types': ['std', 'json'], blake3: ['std'],
    'impluvium-protocol': [...(server ? ['server'] : []), ...(client ? ['client'] : [])],
    ...(invocation.profile === 'core' ? {} : { connectrpc: ['json', ...(server ? ['axum'] : []), ...(client ? ['client'] : [])] }),
    ...(client ? { reqwest: ['rustls', 'http2', 'stream', 'json'], rustls: ['std', 'tls12', 'aws-lc-rs'],
      'hyper-rustls': ['http1', 'http2', 'tls12', 'aws-lc-rs'], 'hyper-util': ['client-legacy', 'http1', 'http2', 'tokio'] } : {}),
  };
  for (const [name, features] of Object.entries(requiredFeatures)) for (const feature of features) {
    if (!runtime.some(pkg => pkg.name === name && pkg.features.includes(feature))) add(`required feature missing: ${name}/${feature}`);
  }
  return issues;
}
export const productIssues = (...args) => productDiagnostics(...args).map(issue => issue.message);

export function legacyIssues(data, report) {
  if (!/^[a-f0-9]{40}$/.test(data.legacy?.baseSha ?? '') || !Array.isArray(data.legacy?.exceptions)) fail('legacy exceptions need a reviewed base capture');
  if (report.version !== 3 || report.scope !== 'all-targets-conservative' || report.baseSha !== data.legacy.baseSha || !Array.isArray(report.paths)) fail('invalid workspace legacy report; recapture required');
  const keys = paths => paths.map(item => {
    if (typeof item.root !== 'string' || item.root.startsWith('engine/') || !Array.isArray(item.path) || !item.path.length ||
        !item.path.every(pkg => typeof pkg.name === 'string' && typeof pkg.version === 'string') ||
        item.target !== 'all' || typeof item.removalOwner !== 'string' || !item.removalOwner.trim()) fail('incomplete or engine legacy exception');
    return JSON.stringify(item);
  }).sort();
  return [...(equal(keys(data.legacy.exceptions), keys(report.paths)) ? [] : ['workspace legacy paths differ from the frozen base exceptions']),
    ...requirementIssues(data.legacy.requirements, report.requirements)];
}

export function checkDiagnostics(options) {
  required(options, 'root', 'contract', 'source-sha');
  if (!/^[a-f0-9]{40}$/.test(options['source-sha'])) fail('expected source-sha must be a full commit ID');
  const root = realpathSync(options.root), data = contract(options.contract);
  if (options.workspace) {
    const report = json(options.workspace);
    if (report.sourceSha !== options['source-sha']) fail('workspace source commit differs from expected source-sha');
    verifySource(root, report.source, options['source-sha']);
    if (report.version !== 3 || report.scope !== 'all-targets-conservative' || report.root !== root || report.lockSha256 !== hash(readFileSync(path.join(root, 'Cargo.lock')))) fail('workspace source or lock differs; recapture required');
    const item = report.metadata?.all;
    if (!item || item.sha256 !== hash(readFileSync(item.file)) || !equal(item.arguments,
      ['metadata', '--locked', '--offline', '--all-features', '--format-version=1'])) fail('workspace raw metadata is missing or inconsistent');
    const metadata = cargoMetadata(item.file, { resolved: true });
    const paths = workspacePaths(metadata, 'all', root, data, report.source);
    if (!equal(paths, report.paths)) fail('workspace paths differ from complete raw graphs');
    if (!equal(workspaceRequirements(metadata, root), report.requirements)) fail('workspace requirements differ from raw declarations');
    return legacyIssues(data, report).map(message => ({ code: 'legacy-policy', file: options.contract, line: 1, message }));
  }
  required(options, 'input');
  const input = path.resolve(options.input), invocation = json(path.join(input, 'invocation.json'));
  if (invocation.sourceSha !== options['source-sha']) fail('capture source commit differs from expected source-sha');
  verifySource(root, invocation.source, options['source-sha'], options['negative-case'] ?? null);
  const status = json(path.join(input, 'capture-status.json'));
  if (status.version !== 1 || status.status !== 'complete') fail('capture did not complete; retain failed evidence and use a fresh results directory');
  const diagnostics = json(path.join(input, 'diagnostics.json'));
  if (diagnostics.version !== 1 || !Array.isArray(diagnostics.errors) || diagnostics.errors.length) fail('capture has unresolved diagnostics');
  if (invocation.version !== 2 || invocation.cwd !== root || !/^[a-f0-9]{40}$/.test(invocation.sourceSha ?? '') ||
      invocation.lockSha256 !== hash(readFileSync(path.join(root, 'Cargo.lock'))) || invocation.buildProfile !== 'dev' ||
      invocation.targetDir !== path.join(input, 'target')) fail('invocation identity, lock, build profile or fresh target directory differs');
  const expected = selection(data, invocation.profile, invocation.target, invocation.run, invocation.targetDir);
  if (!equal(invocation.arguments, expected)) fail('metadata/tree/Cargo arguments differ from the profile selection');
  const env = invocation.environment;
  if (!env || !equal(Object.keys(env).sort(), [...environmentNames].sort()) || Object.values(env).some(value => value !== null && typeof value !== 'string')) fail('effective environment record is incomplete');
  if (env?.CC_ENABLE_DEBUG_OUTPUT !== '1' || env.RUSTFLAGS || env.CARGO_ENCODED_RUSTFLAGS || env.CARGO_BUILD_TARGET || env.CARGO_TARGET_DIR) fail('effective build environment differs');
  if (['CCACHE', 'COMPILER_PATH', 'GCC_EXEC_PREFIX', 'LD_PRELOAD', 'LD_LIBRARY_PATH'].some(name => env[name])) fail('effective tool delegation environment differs');
  for (const file of ['metadata.json', 'packages.tree', 'graph.json', 'cargo.stdout', 'cargo.jsonl', 'build.log', 'tools.json', 'linker.jsonl', 'compiler.jsonl']) {
    if (invocation.evidence?.[file] !== fileDigest(path.join(input, file)).sha256) fail(`raw evidence checksum differs: ${file}`);
  }
  if (!equal(invocation.treeFiles?.['packages.tree'], fileDigest(path.join(input, 'packages.tree')))) fail('tree size/hash record differs: packages.tree');
  const metadata = cargoMetadata(path.join(input, 'metadata.json'), { targets: true });
  if (metadata.workspace_root !== root) fail('metadata workspace root differs from invocation cwd');
  const rootIds = data.profiles[invocation.profile].roots.map(name => {
    const matches = metadata.packages.filter(pkg => pkg.name === name && metadata.workspace_members.includes(pkg.id));
    if (matches.length !== 1) fail(`root package missing or ambiguous: ${name}`);
    if (matches[0].targets.some(target => target.kind.includes('custom-build'))) fail('product root must not have a build.rs generator');
    return matches[0].id;
  });
  if (!equal(rootIds, invocation.rootIds)) fail('root package IDs differ from the selected facades');
  const graph = parsePackages(textLines(path.join(input, 'packages.tree')), metadata, invocation);
  if (!equal(graph, json(path.join(input, 'graph.json')))) fail('normalized graph differs from complete raw evidence');
  const stream = cargoStream(read(path.join(input, 'cargo.stdout')));
  if (stream.jsonl !== read(path.join(input, 'cargo.jsonl'))) fail('Cargo JSON stream was changed or truncated');
  if (invocation.run === 'test' && (invocation.evidence?.['tests.txt'] !== fileDigest(path.join(input, 'tests.txt')).sha256 ||
      read(path.join(input, 'tests.txt')) !== testTranscript(read(path.join(input, 'build.log')), stream))) fail('test transcript differs from raw Cargo suite evidence');
  const artifacts = stream.records.filter(record => record.reason === 'compiler-artifact');
  for (const artifact of artifacts) {
    if (!Array.isArray(artifact.filenames) || !artifact.filenames.length || artifact.filenames.some(file => !file.startsWith(invocation.targetDir + path.sep))) fail('artifact output is outside the captured target directory');
    if (artifact.executable && !artifact.filenames.includes(artifact.executable)) fail('executable is not one of its Cargo artifact files');
    if (!Array.isArray(artifact.features) || !graph.nodes.some(node => node.id === artifact.package_id && equal(sorted(node.features), sorted(artifact.features)))) fail(`artifact features do not match graph: ${artifact.package_id}`);
  }
  for (const node of graph.nodes) if (!artifacts.some(artifact => !artifact.target?.kind?.includes('custom-build') && artifact.package_id === node.id && equal(sorted(artifact.features), sorted(node.features)))) fail(`graph node has no matching Cargo artifact: ${node.id}`);
  const tools = json(path.join(input, 'tools.json'));
  if (tools.version !== 1 || !Array.isArray(tools.calls) || !Array.isArray(tools.linkerCalls) || !Array.isArray(tools.compilerCalls) || !Array.isArray(tools.unused)) fail('tool evidence is incomplete');
  const compilerCalls = read(path.join(input, 'compiler.jsonl')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  if (!equal(compilerCalls, tools.compilerCalls)) fail('delegated compiler records differ from raw evidence');
  if (env.RUSTC !== tools.executables?.rustc?.path || env.RUSTDOC !== tools.executables?.rustdoc?.path ||
      env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER !== path.join(input, invocation.target === linux ? 'target-linker.mjs' : 'host-linker.mjs') ||
      (invocation.target === windows && env.CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER !== path.join(input, 'target-linker.mjs'))) fail('effective Rust compiler or linker delegation differs');
  const binaries = artifacts.filter(record => record.executable).map(record => record.executable);
  const calls = toolCalls(read(path.join(input, 'build.log')), read(path.join(input, 'cargo.stdout')), tools, metadata, root, invocation.target, binaries);
  if (!equal(calls, tools.calls)) fail('actual tool commands differ from raw build-script/rustc evidence');
  for (const name of ['cargo', 'rustc', 'rustdoc', 'clippy-driver', 'hostCC', 'hostCXX', 'hostAR', 'hostLinker', 'targetCC', 'targetCXX', 'targetAR', 'targetLinker']) {
    const tool = tools.executables?.[name];
    if (!tool || !path.isAbsolute(tool.path) || realpathSync(tool.path) !== tool.path || !path.isAbsolute(tool.invocationPath ?? '') || realpathSync(tool.invocationPath) !== tool.path || typeof tool.version !== 'string') fail(`missing or unresolved tool: ${name}`);
  }
  const channel = /channel\s*=\s*"([^"]+)"/.exec(read(path.join(root, 'rust-toolchain.toml')))?.[1];
  for (const name of ['cargo', 'rustc']) if (!tools.executables[name].version.startsWith(`${name} ${channel} `)) fail('captured Rust toolchain differs from the root pin');
  if (invocation.target === windows) {
    const pins = windowsPins(root);
    if (!equal(tools.windows?.pins, pins) || env.CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS !== data.targets[windows].rustflags) fail('wrong Windows pin or static CRT flag');
    verifyWindows(tools.windows, pins);
    for (const role of ['CC', 'CXX', 'AR']) if (env[`${role}_x86_64_pc_windows_gnullvm`] !== path.join(input, `target${role}.mjs`)) fail(`effective Windows ${role} differs from the recorded launcher`);
    for (const role of ['CC', 'CXX', 'AR', 'Linker']) {
      const tool = tools.executables[`target${role}`];
      if (!tool.path.startsWith(realpathSync(tools.windows.root) + path.sep)) fail(`Windows ${role} is outside the verified installation`);
      const named = role === 'AR' ? 'llvm-ar' : role === 'CXX' ? 'x86_64-w64-mingw32-clang++' : 'x86_64-w64-mingw32-clang';
      if (tool.invocationPath !== path.join(tools.windows.root, 'bin', named)) fail(`Windows ${role} invocation path differs from its target-selecting alias`);
      if (hash(readFileSync(tool.path)) !== tools.windows.binaries[named]) fail(`Windows ${role} differs from installed bytes`);
      verifyWindowsVersion(tool.version, pins);
    }
    if (!calls.some(call => call.roles.includes('rustc') && call.args.includes(windows) && call.args.includes('target-feature=+crt-static'))) fail('no rustc invocation proves Windows target and static CRT');
  } else if (env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS) fail('unexpected native Rust flags');
  for (const context of ['host', 'target']) {
    if (read(path.join(input, `${context}-linker.mjs`)) !== read(path.join(root, 'tools/ci/record-linker.mjs'))) fail('linker recorder differs from repository source');
    const config = json(path.join(input, `${context}-linker.mjs.json`));
    if (config.context !== context || config.executable !== tools.executables[`${context}Linker`].invocationPath || config.log !== path.join(input, 'linker.jsonl') || config.target !== invocation.target) fail('linker delegation differs from resolved tools');
    for (const role of ['CC', 'CXX', 'AR']) {
      const key = `${context}${role}`, launcher = path.join(input, `${key}.mjs`), config = json(launcher + '.json');
      if (tools.launchers?.[key] !== launcher || read(launcher) !== read(path.join(root, 'tools/ci/record-compiler.mjs')) ||
          config.executable !== tools.executables[key].invocationPath || config.log !== path.join(input, 'compiler.jsonl') || config.target !== invocation.target || config.role !== key) fail('compiler/archiver delegation differs from resolved tools');
      if (context === 'host' && (env[`HOST_${role}`] !== launcher || env[role] !== launcher)) fail('host compiler environment differs from recorded delegation');
      if (context === 'target' && env[`${role}_${invocation.target.replaceAll('-', '_')}`] !== launcher) fail('target compiler environment differs from recorded delegation');
    }
  }
  const linkers = read(path.join(input, 'linker.jsonl')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  if (!equal(linkers, tools.linkerCalls) || !linkers.length) fail('missing or changed actual linker evidence');
  for (const call of linkers) if (!['host', 'target'].includes(call.context) || call.executable !== tools.executables[`${call.context}Linker`].path ||
    call.launcher !== path.join(input, `${call.context}-linker.mjs`) ||
    call.command !== tools.executables[`${call.context}Linker`].invocationPath || !path.isAbsolute(call.cwd ?? '') || call.status !== 0 || call.signal || call.error) fail('foreign or failed actual linker invocation');
  const unused = unusedTools(tools, calls, linkers);
  if (!equal(unused, tools.unused)) fail('unused tools were not reported accurately');
  for (const node of graph.nodes) {
    const pkg = metadata.packages.find(pkg => pkg.id === node.id);
    // Forbidden system-library adapters can probe a system installation without
    // compiling C. Policy rejects them regardless of permissions; do not replace
    // that rejection with a requirement for a C compilation that never happened.
    if (forbidden.has(pkg.name)) continue;
    const hasCBuild = pkg.dependencies?.some(dep => dep.kind === 'build' && ['cc', 'cmake'].includes(dep.name));
    if (!hasCBuild && !['aws-lc-sys', 'blake3'].includes(pkg.name)) continue;
    const contexts = new Set(artifacts.filter(artifact => artifact.package_id === node.id && !artifact.target?.kind?.includes('custom-build') &&
      equal(sorted(artifact.features), sorted(node.features))).map(artifact => artifactContext(artifact, invocation)));
    for (const context of contexts) for (const role of ['CC', 'AR']) if (!calls.some(call => call.packageId === node.id && call.context === context && call.roles.includes(`${context}${role}`) &&
      /exit status: 0/.test(call.statusEvidence ?? '') && (role === 'CC' ? call.args.includes('-c') : call.args.some(arg => /\.(?:o|obj)$/.test(arg))))) fail(`missing actual ${role} invocation for C package ${pkg.name}`);
  }
  return productDiagnostics(data, graph, metadata, invocation, artifacts).map(issue => ({ ...issue, file: options.contract, line: 1 }));
}

export const check = options => checkDiagnostics(options).map(issue => `${issue.file}:${issue.line}: ${issue.message}`);

await cli(import.meta.url, `Usage: node tools/ci/check-protocol-graph.mjs --root DIR --contract FILE --source-sha SHA --input DIR
Or --workspace FILE with the same required source-sha for the complete workspace legacy-path report. Read-only: never starts Cargo or a shell.
Verify the resolved package/feature set, actual arguments, artifacts and compiler/archiver/linker calls.
Empty permissions and incomplete legacy exceptions fail closed. Engine roots never receive legacy exceptions.
Exit: 0 passed, 1 contract violation, 2 missing, ambiguous or inconsistent evidence.`, {
  root: stringOption, contract: stringOption, input: stringOption, workspace: stringOption, 'negative-case': stringOption, 'source-sha': stringOption,
}, check);
