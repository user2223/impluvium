// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

import path from 'node:path';
import { baseline, cli, InputError, json, plainObject, problem, required, stringOption } from './common.mjs';
import { dependencies, localPath, manifests, workspaceFor } from './cargo.mjs';
import { cargoMetadata } from './metadata.mjs';
import { commands, executable } from './shell.mjs';
import { yamlData } from './pnpm-licences.mjs';

export const serverPackages = ['server-api-types', 'server-domain', 'server-db', 'server-http', 'server-bin'];
const directory = name => `server/crates/${name}`;
const composition = {
  'server-api-types': [], 'server-domain': [], 'server-db': ['server-domain'],
  'server-http': ['server-domain', 'server-api-types'], 'server-bin': ['server-http'],
};
const admissionFile = 'tools/ci/server-dependencies.json';
const validAdmission = entries => Array.isArray(entries) && entries.every(entry => plainObject(entry) &&
  typeof entry.package === 'string' && entry.package.length > 0 &&
  (entry.path === undefined || typeof entry.path === 'string') &&
  Object.keys(entry).every(key => ['package', 'path'].includes(key)));

export function resolvedIssues(file, root) {
  const metadata = cargoMetadata(file, { resolved: true });
  const packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const fail = message => { throw new InputError(problem(file, 1, message)); };
  for (const node of nodes.values()) {
    if (!Array.isArray(node.features) || !node.features.every(feature => typeof feature === 'string') ||
        !Array.isArray(node.deps) || !node.deps.every(dep => typeof dep?.name === 'string' && nodes.has(dep.pkg) &&
          Array.isArray(dep.dep_kinds) && dep.dep_kinds.length > 0 && dep.dep_kinds.every(kind =>
            kind && [null, 'build', 'dev'].includes(kind.kind) && (kind.target === null || typeof kind.target === 'string'))) ||
        node.dependencies.some(id => !node.deps.some(dep => dep.pkg === id)) ||
        node.deps.some(dep => !node.dependencies.includes(dep.pkg))) {
      fail('expected complete resolved deps, dep_kinds and features (Cargo metadata format 1)');
    }
  }
  const issues = [];
  for (const name of serverPackages) {
    const origin = metadata.packages.find(pkg => pkg.name === name && metadata.workspace_members.includes(pkg.id));
    if (!origin || origin.manifest_path !== path.join(root, directory(name), 'Cargo.toml')) {
      fail(`${name}: missing server workspace root or manifest path mismatch`);
    }
    for (const dep of nodes.get(origin.id).deps) {
      const target = packages.get(dep.pkg);
      if (name !== 'server-http' && (/^(?:axum|tower)(?:-|$)/.test(target.name) ||
          (['hyper', 'hyper-util'].includes(target.name) && nodes.get(dep.pkg).features.some(feature =>
            feature === 'server' || feature.startsWith('server-'))))) {
        issues.push(problem(file, 1, `${name}: only server-http may directly depend on ${target.name} with server support`));
      }
    }
    if (name === 'server-db') continue;
    const visited = new Set();
    const visit = (id, route) => {
      if (visited.has(id)) return;
      visited.add(id);
      const pkg = packages.get(id), chain = [...route, pkg.name];
      if (/^sqlx(?:-|$)/.test(pkg.name)) {
        issues.push(problem(file, 1, `${name}: sqlx is reachable via normal dependencies: ${chain.join(' -> ')}`));
      }
      for (const dep of nodes.get(id).deps.filter(dep => dep.dep_kinds.some(kind => kind.kind === null))) visit(dep.pkg, chain);
    };
    visit(origin.id, []);
  }
  return issues;
}

export function workflowIssues(file) {
  const workflow = yamlData(file), issues = [];
  const steps = workflow.jobs?.server?.steps ?? [];
  for (const step of steps.filter(item => /\bcargo (?:metadata|fmt|clippy|test)\b|tools\/ci\/(?:clippy-ratchet|count-tests|check-server-boundaries)\.mjs/.test(item.run ?? ''))) {
    if (step.if !== undefined || step['continue-on-error'] !== undefined || step['working-directory'] !== undefined) {
      issues.push(problem(file, 1, 'server Rust checks must run unconditionally from the repository root'));
    }
  }
  const invocations = steps.flatMap(step => commands(step.run ?? '', file).map(tokens => executable(tokens).map(token => token.value)));
  const checkSelection = (label, matches, flag) => {
    const selected = matches.flatMap(words => words.flatMap((word, index) => word === flag ? [words[index + 1]] : []));
    if (matches.length !== 1 || selected.length !== serverPackages.length ||
        !serverPackages.every(name => selected.includes(name)) || matches.some(words =>
          words.includes('--workspace') || words.includes('--exclude'))) {
      issues.push(problem(file, 1, `${label}: expected exactly the five explicit server packages`));
    }
  };
  for (const action of ['fmt', 'clippy', 'test']) {
    const matches = invocations.filter(words => words[0] === 'cargo' && words[1] === action);
    checkSelection(`server cargo ${action}`, matches, '-p');
    if (action === 'clippy' && !matches.some(words => words.includes('-D') && words.includes('warnings'))) {
      issues.push(problem(file, 1, 'server Clippy must deny warnings'));
    }
  }
  for (const [tool, baseline] of [['clippy-ratchet', 'clippy-baseline'], ['count-tests', 'server-tests']]) {
    const matches = invocations.filter(words => words[0] === 'node' && words[1] === `tools/ci/${tool}.mjs`);
    checkSelection(`server ${tool}`, matches, '--package');
    if (!matches.some(words => words[words.indexOf('--baseline') + 1] === `tools/ci/${baseline}.json`)) {
      issues.push(problem(file, 1, `server ${tool}: incorrect baseline`));
    }
  }
  const capture = invocations.filter(words => words[0] === 'cargo' && words[1] === 'metadata');
  const resolved = invocations.filter(words => words[0] === 'node' && words[1] === 'tools/ci/check-server-boundaries.mjs');
  const metadataPath = '$RUNNER_TEMP/server-metadata.json';
  if (capture.length !== 1 || !capture[0].includes('--locked') || !capture[0].includes('--offline') ||
      !capture[0].includes('--format-version=1') || !capture[0].includes('>') ||
      capture[0][capture[0].indexOf('>') + 1] !== metadataPath || resolved.length !== 1 ||
      resolved[0][resolved[0].indexOf('--metadata') + 1] !== metadataPath ||
      resolved[0][resolved[0].indexOf('--root') + 1] !== '.') {
    issues.push(problem(file, 1, 'server must capture locked offline metadata and run the resolved boundary check on that file'));
  }
  const boundary = workflow.jobs?.checks?.steps?.filter(step =>
    step.run?.trim() === 'node tools/ci/check-server-boundaries.mjs --root .' &&
      step.if === undefined && step['continue-on-error'] === undefined);
  if (boundary?.length !== 1) issues.push(problem(file, 1, 'checks must run the server boundary checker unconditionally'));
  return issues;
}

export function check(options) {
  required(options, 'root');
  const root = path.resolve(options.root), all = manifests(root);
  const admission = baseline(path.join(root, admissionFile), 'packages', validAdmission);
  if (Object.keys(admission).length !== serverPackages.length || !serverPackages.every(name => Object.hasOwn(admission, name))) {
    throw new InputError(problem(admissionFile, 1, 'expected dependency admission for exactly the five server packages'));
  }
  const workspace = all.find(item => item.relative === 'Cargo.toml')?.data.workspace;
  const issues = [];
  const report = (manifest, line, message) => issues.push(problem(manifest.relative, line, message));
  const byPath = new Map(all.map(item => [item.relative, item]));
  const edges = new Map();
  for (const manifest of all) {
    const list = dependencies(manifest).map(dependency => {
      const inherited = dependency.value?.workspace === true
        ? workspaceFor(manifest, all)?.data.workspace?.dependencies?.[dependency.alias] : dependency.value;
      const target = localPath(manifest, dependency, all);
      if (target && !byPath.has(target)) report(manifest, dependency.line, `missing local dependency manifest: ${target}`);
      return { ...dependency, target, name: byPath.get(target)?.data.package?.name ?? inherited?.package ?? dependency.alias };
    });
    edges.set(manifest.relative, list);
  }
  for (const name of serverPackages) {
    const member = directory(name), manifest = byPath.get(`${member}/Cargo.toml`);
    if (!manifest || manifest.data.package?.name !== name || !workspace?.members?.includes(member)) {
      issues.push(problem('Cargo.toml', 1, `${name}: missing named server workspace member`));
      continue;
    }
    if (workspace['default-members']?.includes(member)) report(manifest, 1, 'server packages must be selected explicitly, not added to default-members');
    if (manifest.data.package.license !== 'AGPL-3.0-only') report(manifest, 1, 'server packages require AGPL-3.0-only');
    const direct = edges.get(manifest.relative);
    for (const dependency of direct) {
      if (!admission[name].some(entry => entry.package === dependency.name && entry.path === dependency.target)) {
        report(manifest, dependency.line, `${name}: dependency ${dependency.name} is not admitted in ${admissionFile}`);
      }
      if (dependency.target?.startsWith('server/crates/') &&
          !composition[name].some(target => dependency.target === `${directory(target)}/Cargo.toml`)) {
        report(manifest, dependency.line, `${name}: forbidden server layer dependency ${dependency.target}`);
      }
    }
    for (const target of composition[name]) {
      // The composition edges must be normal dependencies, not test-only links.
      const normal = manifest.data.dependencies ?? {};
      if (!Object.keys(normal).some(alias => direct.some(dependency => dependency.alias === alias &&
          dependency.target === `${directory(target)}/Cargo.toml`))) {
        report(manifest, 1, `${name}: missing product dependency on ${target}`);
      }
    }
  }
  // Inspect every local path, including dev/build/optional/target dependencies
  // and workspace aliases. An intermediate crate must not hide a reverse edge.
  for (const origin of all.filter(item => item.directory.startsWith('engine/'))) {
    const visited = new Set();
    const visit = manifest => {
      if (visited.has(manifest.relative)) return;
      visited.add(manifest.relative);
      for (const dependency of edges.get(manifest.relative) ?? []) {
        if (dependency.target?.startsWith('server/')) {
          report(manifest, dependency.line, `engine dependencies must not point to the server (from ${origin.relative})`);
        }
        if (dependency.target && byPath.has(dependency.target)) visit(byPath.get(dependency.target));
      }
    };
    visit(origin);
  }
  const budgets = json(path.join(root, 'tools/ci/clippy-baseline.json')).packages;
  const tests = json(path.join(root, 'tools/ci/server-tests.json')).packages;
  for (const name of serverPackages) {
    if (budgets?.[name] !== 0) issues.push(problem('tools/ci/clippy-baseline.json', 1, `${name}: requires a zero warning budget`));
    if (!tests?.[name]) issues.push(problem('tools/ci/server-tests.json', 1, `${name}: missing test baseline`));
  }
  if (Object.keys(tests ?? {}).some(name => !serverPackages.includes(name))) {
    issues.push(problem('tools/ci/server-tests.json', 1, 'test baseline contains a package outside the server selection'));
  }
  return [...issues, ...workflowIssues(path.join(root, '.gitea/workflows/ci.yml')),
    ...(options.metadata !== undefined ? resolvedIssues(options.metadata, root) : [])];
}

await cli(import.meta.url, `Usage: node tools/ci/check-server-boundaries.mjs --root DIR [--metadata FILE]
Check closed dependency admission, server composition, engine direction, baselines and workflow.
Include workspace aliases and all dependency kinds and targets. With --metadata, also check
normal resolved paths to sqlx and direct axum/tower/Hyper server dependencies for every server root.
The inherited torrential server and the engine protocol transport keep their existing HTTP dependencies.
Exit: 0 passed, 1 boundary violation, 2 usage or input error.`, { root: stringOption, metadata: stringOption }, check);
