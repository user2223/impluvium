import path from 'node:path';
import { existsSync } from 'node:fs';
import { cli, InputError, problem, read, stringOption } from './common.mjs';
import { dependencies, localPath, manifests, workspaceFor } from './cargo.mjs';

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const all = manifests(root);
  const byPath = new Map(all.map(item => [item.relative, item]));
  const issues = [];
  const graph = new Map();
  const checkedLicences = new Set();
  for (const manifest of all) {
    const edges = [];
    for (const dependency of dependencies(manifest)) {
      let target = localPath(manifest, dependency, all);
      // A registry dependency may be redirected to a local crate by a patch.
      if (!target) {
        const name = dependency.value?.package ?? dependency.alias;
        const workspace = workspaceFor(manifest, all);
        const tables = [...Object.values(workspace?.data.patch ?? {}), workspace?.data.replace ?? {}];
        for (const table of tables) {
          for (const [alias, value] of Object.entries(table)) {
            if ((value?.package ?? alias.split(':')[0]) === name && value?.path) {
              target = localPath(workspace, { alias, value, line: dependency.line }, all);
            }
          }
        }
      }
      if (target) {
        if (!byPath.has(target)) throw new InputError(problem(manifest.relative, dependency.line, `local dependency manifest is missing: ${target}`));
        edges.push({ target, line: dependency.line });
      }
    }
    graph.set(manifest.relative, edges);
    if (!manifest.relative.startsWith('engine/')) continue;
    let licence = manifest.data.package?.license;
    if (licence?.workspace === true) {
      licence = workspaceFor(manifest, all)?.data.workspace?.package?.license;
      if (typeof licence !== 'string') throw new InputError(problem(manifest.relative, 1, 'cannot resolve inherited package license'));
    }
    if (typeof licence === 'string' && /\bMPL(?:-\d+(?:\.\d+)?)?\b/i.test(licence)) {
      const entry = manifest.entries.find(item => item.keys.slice(0, 2).join('.') === 'package.license');
      issues.push(problem(manifest.relative, entry?.line ?? 1, 'MPL-licensed crate is not allowed under engine/'));
    }
    const licenceFiles = new Set();
    for (let directory = manifest.directory; directory === 'engine' || directory.startsWith('engine/'); directory = path.dirname(directory)) {
      for (const name of ['LICENSE', 'LICENSE.md', 'LICENSE.txt', 'LICENCE', 'LICENCE.md', 'LICENCE.txt']) {
        licenceFiles.add(path.resolve(root, directory, name));
      }
    }
    let explicit = manifest.data.package?.['license-file'];
    let licenceRoot = manifest.directory;
    if (explicit?.workspace === true) {
      const workspace = workspaceFor(manifest, all);
      explicit = workspace?.data.workspace?.package?.['license-file'];
      licenceRoot = workspace?.directory;
      if (typeof explicit !== 'string') throw new InputError(problem(manifest.relative, 1, 'cannot resolve inherited license-file'));
    }
    if (typeof explicit === 'string') {
      const file = path.resolve(root, licenceRoot, explicit);
      if (!file.startsWith(root + path.sep)) throw new InputError(problem(manifest.relative, 1, 'license-file leaves the repository'));
      if (!existsSync(file)) throw new InputError(problem(manifest.relative, 1, `license-file is missing: ${explicit}`));
      licenceFiles.add(file);
    }
    for (const file of licenceFiles) {
      if (existsSync(file) && !checkedLicences.has(file)) {
        checkedLicences.add(file);
        const content = read(file);
        const match = /Mozilla Public License|\bMPL-2\.0\b/i.exec(content);
        if (match) issues.push(problem(path.relative(root, file), content.slice(0, match.index).split('\n').length, 'MPL licence text is not allowed under engine/'));
      }
    }
  }
  for (const start of all.filter(item => item.relative.startsWith('engine/'))) {
    const visited = new Set();
    const visit = (current, trail, firstLine) => {
      if (visited.has(current)) return;
      visited.add(current);
      if (current.startsWith('server/crates/')) {
        issues.push(problem(start.relative, firstLine, `engine dependency crosses into server/crates/: ${trail.join(' -> ')}`));
        return;
      }
      for (const edge of graph.get(current) ?? []) visit(edge.target, [...trail, edge.target], firstLine ?? edge.line);
    };
    visit(start.relative, [start.relative]);
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-licence-boundary.mjs [--root DIR]
Reject direct/transitive local dependencies from engine/ into server/crates/, including optional,
target, build, dev, inherited workspace and patched dependencies. Reject MPL licences under engine/.
Reads Cargo manifests and licence files; does not execute Cargo. Missing local manifests are input errors.
Exit: 0 passed, 1 boundary violation, 2 usage or input error.`, { root: stringOption }, check);
