import path from 'node:path';
import { existsSync } from 'node:fs';
import { InputError, problem, read } from './common.mjs';
import { acceptable } from './spdx.mjs';

const mpl = /Mozilla[ _-]+Public[ _-]+License|\bMPL(?:-\d+(?:\.\d+)?)?\b/i;
const names = ['LICENSE', 'LICENSE.md', 'LICENSE.txt', 'LICENCE', 'LICENCE.md', 'LICENCE.txt'];
export const inside = (root, file) => {
  const relative = path.relative(root, file);
  return !path.isAbsolute(relative) && relative !== '..' && !relative.startsWith('..' + path.sep);
};

function licenseFiles(pkg, root) {
  const directory = path.dirname(pkg.manifest);
  const candidates = new Set(names.map(name => path.join(directory, name)));
  for (let parent = directory; inside(path.join(root, 'engine'), parent); parent = path.dirname(parent)) {
    for (const name of names) candidates.add(path.join(parent, name));
  }
  if (pkg.licenseFile != null) {
    if (typeof pkg.licenseFile !== 'string' || !pkg.licenseFile) throw new InputError(`${pkg.manifest}:1: invalid license file`);
    const explicit = path.resolve(directory, pkg.licenseFile);
    read(explicit);
    candidates.add(explicit);
  }
  const issues = [];
  for (const file of candidates) {
    if (!existsSync(file)) continue;
    const text = read(file), match = mpl.exec(text);
    if (match || mpl.test(path.basename(file))) issues.push(problem(path.relative(root, file), 1, `${pkg.name}: MPL licence file is reachable from engine/`));
  }
  return issues;
}

export function traverse({ packages, graph, roots }, root, rules) {
  if (!roots.length) throw new InputError('<licence-graph>:1: no engine/ packages selected');
  const issues = [], checked = new Set();
  for (const start of roots) {
    const queue = [{ id: start, trail: [] }], visited = new Set();
    for (let index = 0; index < queue.length; index++) {
      const { id, trail } = queue[index];
      if (visited.has(id)) continue;
      visited.add(id);
      const pkg = packages.get(id), edges = graph.get(id);
      if (!pkg || !Array.isArray(edges)) throw new InputError(`<licence-graph>:1: incomplete dependency graph at ${id}`);
      const relative = path.relative(root, pkg.manifest).split(path.sep).join('/');
      const route = [...trail, relative];
      if ((pkg.repository ?? inside(root, pkg.manifest)) && !inside(path.join(root, 'engine'), pkg.manifest)) {
        issues.push(problem(packages.get(start).manifest, 1, `engine dependency crosses outside engine/: ${route.join(' -> ')}`));
      }
      if (!checked.has(id)) {
        checked.add(id);
        let allowed;
        try { allowed = acceptable(pkg.license, rules); }
        catch (error) { throw new InputError(problem(pkg.manifest, 1, `${pkg.name}: ${error.message}`)); }
        if (!allowed) issues.push(problem(pkg.manifest, 1, `${pkg.name}: licence expression is not permitted in engine/: ${pkg.license}`));
        issues.push(...licenseFiles(pkg, root));
      }
      for (const dependency of edges) queue.push({ id: dependency, trail: route });
    }
  }
  return issues;
}
