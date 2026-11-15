import path from 'node:path';
import { realpathSync } from 'node:fs';
import { equal, fail } from './protocol-contract.mjs';
import { diagnosticLog } from './protocol-diagnostics.mjs';

const sorted = list => [...new Set(list)].sort();
const featureList = text => sorted(text.trim() ? text.trim().split(/[,\s]+/) : []);
export function unusedTools(tools, calls, linkers) {
  return Object.keys(tools.executables).filter(role => role !== 'cargo' && !calls.some(call => call.roles.includes(role)) &&
    !(role.endsWith('Linker') && linkers.some(call => role === `${call.context}Linker` && call.executable === tools.executables[role].path)));
}
export function permissionCandidate(graph, metadata, invocation) {
  const packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  return {
    profile: invocation.profile, target: invocation.target, run: invocation.run,
    packages: graph.nodes.map(node => ({ name: packages.get(node.id).name, version: packages.get(node.id).version, source: packages.get(node.id).source ?? null, features: sorted(node.features) })),
  };
}
export function identity(text, metadata) {
  const macroMarkers = text.match(/ \(proc-macro\)(?= |$)/g) ?? [];
  if (macroMarkers.length > 1) fail(`unsupported cargo tree identity ${text}`);
  const match = /^([^ ]+) v([^ ]+)(?: \((.*)\))?$/.exec(text.replace(/ \(proc-macro\)(?= |$)/, ''));
  if (!match) fail(`unsupported cargo tree identity ${text}`);
  let candidates = metadata.packages.filter(pkg => pkg.name === match[1] && pkg.version === match[2]);
  if (match[3]) candidates = candidates.filter(pkg => path.dirname(pkg.manifest_path) === match[3] || pkg.source?.includes(match[3]));
  if (macroMarkers.length) candidates = candidates.filter(pkg => pkg.targets?.some(target => target.kind.includes('proc-macro')));
  if (candidates.length !== 1) fail(`ambiguous cargo package identity ${text}`);
  return candidates[0];
}

export function parsePackages(input, metadata, invocation, diagnostics) {
  const report = diagnostics ?? diagnosticLog(), nodes = new Map(), identities = new Map();
  const lines = typeof input === 'string' ? input.split(/\r?\n/) : input;
  let line = 0;
  for (const raw of lines) {
    line++;
    if (!raw.trim() || /^\[(?:build|dev)-dependencies\]$/.test(raw)) continue;
    report.attempt('packages.tree', line, () => {
      const text = raw.endsWith(' (*)') ? raw.slice(0, -4) : raw;
      const match = /^(.+?)\|([^|]*)$/.exec(text);
      if (!match) fail('expected flat package|features record');
      if (!identities.has(match[1])) identities.set(match[1], identity(match[1], metadata));
      if (!/^(?:[A-Za-z0-9_+.-]+(?:,[A-Za-z0-9_+.-]+)*)?$/.test(match[2])) fail('invalid resolved feature list');
      const pkg = identities.get(match[1]), features = featureList(match[2]);
      const node = { id: pkg.id, features };
      nodes.set(JSON.stringify(node), node);
    }, raw);
  }
  if (!nodes.size) report.add('packages.tree', 1, 'empty resolved package list');
  for (const id of invocation.rootIds) if (![...nodes.values()].some(node => node.id === id)) report.add('packages.tree', 1, `selected root missing: ${id}`);
  if (!diagnostics) report.finish();
  return { version: 2, representation: 'package-features', roots: sorted(invocation.rootIds),
    nodes: [...nodes.values()].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))) };
}

export function cargoStream(stdout, diagnostics) {
  const report = diagnostics ?? diagnosticLog();
  const records = [], lines = [], after = [];
  let finished = false;
  for (const [index, line] of stdout.split('\n').entries()) {
    if (!line.trim()) { if (finished) after.push(line); continue; }
    if (finished) {
      if (/^\s*\{.*"reason"/.test(line)) report.add('cargo.stdout', index + 1, 'repeated Cargo build stream', line);
      after.push(line); continue;
    }
    let record;
    try { record = JSON.parse(line); } catch {
      if (/^\[[^\]]+\] /.test(line)) continue;
      report.add('cargo.stdout', index + 1, 'non-JSON output before build-finished', line); continue;
    }
    if (typeof record?.reason !== 'string') { report.add('cargo.stdout', index + 1, 'missing Cargo message reason', line); continue; }
    records.push(record); lines.push(line);
    if (record.reason === 'build-finished') {
      if (record.success !== true) report.add('cargo.stdout', index + 1, 'Cargo build did not succeed', line);
      finished = true;
    }
  }
  if (!finished) report.add('cargo.stdout', 1, 'missing build-finished');
  if (!diagnostics) report.finish();
  return { records, jsonl: lines.join('\n') + '\n', tests: after.join('\n') };
}

// Parse the command display emitted by cc and Cargo, never evaluate it in a shell.
export function words(source) {
  const result = [];
  let value = '', quote = null, started = false;
  for (let index = 0; index < source.length; index++) {
    const char = source[index];
    if (quote === "'") {
      if (char === "'") quote = null;
      else value += char;
    } else if (char === '\\') {
      const next = source[++index];
      if (next === undefined) fail('unfinished command escape');
      if (quote === '"' && !['"', '\\', '$', '`', '\n'].includes(next)) value += '\\';
      if (next !== '\n') value += next;
      started = true;
    } else if (quote === '"') {
      if (char === '"') quote = null;
      else value += char;
    } else if (char === "'" || char === '"') { quote = char; started = true; }
    else if (/\s/.test(char)) {
      if (started) { result.push(value); value = ''; started = false; }
    } else { value += char; started = true; }
  }
  if (quote) fail('unfinished command quote');
  if (started) result.push(value);
  return result;
}

export function rustCommands(log, diagnostics, file = 'build.log') {
  const report = diagnostics ?? diagnosticLog();
  const commands = [], lines = log.split('\n');
  for (let index = 0; index < lines.length; index++) {
    const match = /^\s*Running `(.*)$/.exec(lines[index]);
    if (!match) continue;
    const line = index + 1;
    let text = match[1];
    while (!text.endsWith('`')) {
      if (++index === lines.length || /^\s*Running `/.test(lines[index])) {
        report.add(file, line, 'truncated Cargo command display', text);
        index--; break;
      }
      text += '\n' + lines[index];
    }
    if (text.endsWith('`')) commands.push({ line, text: text.slice(0, -1) });
  }
  if (!diagnostics) report.finish();
  return commands;
}

export function testTranscript(log, stream) {
  const binaries = new Set(stream.records.filter(record => record.reason === 'compiler-artifact' && record.executable).map(record => record.executable));
  const candidates = [];
  for (const [index, line] of log.split('\n').entries()) {
    if (/^\s*Doc-tests /.test(line)) candidates.push({ line: index + 1, header: line });
    const plain = /^\s*Running (?:unittests|tests\/).*\(([^)]+)\)$/.exec(line);
    if (plain) candidates.push({ line: index + 1, binary: plain[1] });
  }
  for (const record of rustCommands(log)) {
    const tokens = words(record.text);
    while (/^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[0] ?? '')) tokens.shift();
    if (binaries.has(tokens[0])) candidates.push({ line: record.line, binary: tokens[0] });
  }
  const seen = new Set(), headers = [];
  for (const item of candidates.sort((a, b) => a.line - b.line)) {
    if (item.header) headers.push(item.header);
    else if (!seen.has(item.binary)) { seen.add(item.binary); headers.push(`Running test (${item.binary})`); }
  }
  const bodies = stream.tests.split(/(?=^running \d+ tests?\s*$)/m).filter(chunk => /^running /m.test(chunk));
  if (!headers.length || headers.length !== bodies.length) fail('cannot pair test suite headers with bodies');
  return headers.map((header, index) => header + '\n' + bodies[index]).join('\n');
}

export function toolCalls(log, stdout, tools, metadata, root, target, executables = [], diagnostics) {
  const report = diagnostics ?? diagnosticLog();
  const artifacts = cargoStream(stdout, report).records;
  const calls = [], active = new Set(artifacts.filter(record => record.package_id).map(record => record.package_id));
  const buildScripts = new Set(artifacts.filter(record => record.reason === 'compiler-artifact' &&
    record.target?.kind?.includes('custom-build') && metadata.packages.some(pkg => pkg.id === record.package_id &&
      pkg.targets?.some(target => target.kind.includes('custom-build') && target.name === record.target.name)))
    .flatMap(record => record.filenames ?? []).filter(file => path.isAbsolute(file)));
  for (const [file, source] of [['build.log', log], ['cargo.stdout', stdout]]) {
    const lines = source.split('\n');
    const records = rustCommands(source, report, file).map(record => ({ ...record, cc: null }));
    for (let index = 0; index < lines.length; index++) {
      const cc = /^\[([^\]]+)\] running: (.*)$/.exec(lines[index]);
      if (cc) records.push({ line: index + 1, text: cc[2], cc: cc[1] });
    }
    const inspect = record => {
      const cc = record.cc;
      const args = words(record.text);
      const environment = {};
      while (/^[A-Za-z_][A-Za-z0-9_]*=/.test(args[0] ?? '')) {
        const value = args.shift(), split = value.indexOf('='); environment[value.slice(0, split)] = value.slice(split + 1);
      }
      const command = args.shift();
      if (!command) fail('empty tool command');
      // Cargo already identifies these exact build/test artifact paths. Their
      // target directory may be removed after immediate checking; replay never
      // executes or resolves the generated programs.
      if (!cc && (buildScripts.has(command) || executables.includes(command))) return;
      let delegated = null;
      if (cc) {
        const launcher = Object.values(tools.launchers ?? {}).find(file => file === command);
        if (!launcher) fail(`compiler/archiver bypassed the evidence launcher: ${command}`);
        const matches = (tools.compilerCalls ?? []).map((call, index) => ({ ...call, recordLine: index + 1 })).filter(call =>
          call.launcher === launcher && equal(call.args, args) && `${call.packageName} ${call.packageVersion}` === cc);
        if (!matches.length) fail(`no actual delegated command for ${cc}`);
        const identities = new Set(matches.map(call => JSON.stringify([call.executable, call.command, call.cwd, call.context, call.role, call.manifestDir, call.status])));
        if (identities.size !== 1) fail(`ambiguous delegated command for ${cc}`);
        delegated = matches[0];
        if (!path.isAbsolute(delegated.cwd) || !['host', 'target'].includes(delegated.context) ||
            !/^(host|target)(CC|CXX|AR)$/.test(delegated.role ?? '') || !delegated.role.startsWith(delegated.context) ||
            !Number.isInteger(delegated.status) || delegated.status < 0 || delegated.error || delegated.signal) fail('incomplete delegated compiler invocation');
      }
      let actual;
      if (delegated) actual = delegated.executable;
      else if (path.isAbsolute(command)) actual = realpathSync(command);
      else {
        const found = Object.values(tools.executables).filter(tool => tool.command === command || path.basename(tool.path) === command);
        const paths = sorted(found.map(tool => tool.path));
        if (paths.length !== 1) {
          fail(`unknown compiler/archiver or Rust command ${command}`);
        }
        actual = paths[0];
      }
      let matching = Object.entries(tools.executables).filter(([, tool]) => tool.path === actual).map(([role]) => role);
      if (!matching.length) {
        // Custom build targets can have names other than build-script-build.
        // Only exact artifact paths from the corresponding metadata target qualify.
        if (cc || (!buildScripts.has(command) && !executables.includes(command))) fail(`unapproved tool was invoked: ${command}`);
        return;
      }
      const candidates = cc ? metadata.packages.filter(pkg => active.has(pkg.id) && `${pkg.name} ${pkg.version}` === cc && path.dirname(pkg.manifest_path) === delegated.manifestDir) : [];
      if (cc && candidates.length !== 1) fail(`unattributed or ambiguous build-script command ${cc}`);
      const pkg = cc ? candidates[0] : null;
      const context = delegated?.context ?? (args.some(arg => arg.includes(`/${target}/`)) ? 'target' : 'host');
      if (cc) {
        matching = matching.filter(role => role === delegated.role);
        if (!matching.length) fail(`compiler/archiver differs from the resolved ${context} tool: ${command}`);
        if (delegated.command !== tools.executables[delegated.role].invocationPath) fail('delegated compiler invocation path differs');
      }
      calls.push({ file, line: record.line, path: actual, roles: matching, args, environment,
        packageId: pkg?.id ?? null, cwd: delegated?.cwd ?? null, cargoCwd: root,
        context,
        statusEvidence: delegated ? `compiler.jsonl:${delegated.recordLine}: exit status: ${delegated.status}` : 'successful Cargo build-finished' });
    };
    for (const record of records.sort((a, b) => a.line - b.line)) report.attempt(file, record.line, () => inspect(record), record.text);
  }
  if (!diagnostics) report.finish();
  return calls;
}
