import path from 'node:path';
import { existsSync } from 'node:fs';
import { cli, files, InputError, problem, read, required, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';

const mpl = /Mozilla[ _-]+Public[ _-]+License|\bMPL(?:-\d+(?:\.\d+)?)?\b/i;
const copyleft = /\b(?:MPL|GPL|LGPL|AGPL)(?:-\d+(?:\.\d+)?)?\b/i;
const licenceNames = ['LICENSE', 'LICENSE.md', 'LICENSE.txt', 'LICENCE', 'LICENCE.md', 'LICENCE.txt'];

function licenceIssues(pkg, root) {
  const file = path.relative(root, pkg.manifest_path);
  const issues = [];
  if (typeof pkg.license === 'string' && copyleft.test(pkg.license)) {
    const family = copyleft.exec(pkg.license)[0];
    issues.push(problem(file, 1, `${pkg.name}: ${family.replace(/-.*/, '')}-licensed crate is reachable from engine/`));
  }
  const directory = path.dirname(pkg.manifest_path);
  const candidates = new Set(licenceNames.map(name => path.join(directory, name)));
  // Preserve checks of shared engine licence notices as well as Cargo metadata.
  for (let parent = directory; ; parent = path.dirname(parent)) {
    const relative = path.relative(root, parent);
    if (relative !== 'engine' && !relative.startsWith('engine' + path.sep)) break;
    for (const name of licenceNames) candidates.add(path.join(parent, name));
  }
  if (pkg.license_file != null) {
    if (typeof pkg.license_file !== 'string') throw new InputError(problem(file, 1, 'invalid license_file in metadata'));
    const explicit = path.resolve(directory, pkg.license_file);
    if (mpl.test(path.basename(explicit))) issues.push(problem(file, 1, `${pkg.name}: MPL license_file is reachable from engine/`));
    // Explicit metadata files must be available alongside the captured metadata.
    read(explicit);
    candidates.add(explicit);
  }
  for (const candidate of candidates) {
    if (!existsSync(candidate)) continue;
    const text = read(candidate);
    const match = mpl.exec(text);
    if (match) issues.push(problem(path.relative(root, candidate), text.slice(0, match.index).split('\n').length,
      `${pkg.name}: MPL licence text is reachable from engine/`));
  }
  return issues;
}

export function check(options) {
  required(options, 'root', 'metadata');
  const metadata = cargoMetadata(options.metadata, { resolved: true });
  const root = path.resolve(options.root);
  const packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  const graph = new Map(metadata.resolve.nodes.map(node => [node.id, node.dependencies]));
  const relative = pkg => path.relative(root, pkg.manifest_path).split(path.sep).join('/');
  const insideRoot = pkg => !/^\.\.(?:\/|$)/.test(relative(pkg)) && !path.isAbsolute(relative(pkg));
  if (!metadata.packages.length || !metadata.workspace_members.length) {
    throw new InputError(problem(options.metadata, 1, 'metadata must contain workspace members'));
  }
  if (metadata.packages.some(pkg => !path.isAbsolute(pkg.manifest_path))) {
    throw new InputError(problem(options.metadata, 1, 'metadata manifest paths must be absolute'));
  }
  for (const id of metadata.workspace_members) {
    const pkg = packages.get(id);
    if (!path.isAbsolute(pkg.manifest_path) || !insideRoot(pkg)) {
      throw new InputError(problem(options.metadata, 1, `${pkg.name}: workspace member manifest is outside --root`));
    }
  }
  const manifests = new Set(metadata.packages.map(pkg => path.resolve(pkg.manifest_path)));
  for (const file of files(root).filter(file => file.startsWith('engine/') && path.basename(file) === 'Cargo.toml')) {
    if (!manifests.has(path.join(root, file))) {
      throw new InputError(problem(file, 1, 'engine manifest missing from metadata.packages; capture all engine workspaces at this --root'));
    }
  }
  const issues = [];
  const checked = new Set();
  for (const start of metadata.packages.filter(pkg => relative(pkg).startsWith('engine/'))) {
    if (!graph.has(start.id)) throw new InputError(problem(options.metadata, 1, `${start.name}: engine package missing from resolved graph`));
    const visited = new Set();
    const queue = [{ id: start.id, trail: [relative(start)] }];
    for (let index = 0; index < queue.length; index++) {
      const { id, trail } = queue[index];
      if (visited.has(id)) continue;
      visited.add(id);
      const pkg = packages.get(id);
      if (insideRoot(pkg) && !relative(pkg).startsWith('engine/')) {
        issues.push(problem(relative(start), 1, `engine dependency crosses outside engine/: ${trail.join(' -> ')}`));
      }
      if (!checked.has(id)) {
        checked.add(id);
        issues.push(...licenceIssues(pkg, root));
      }
      for (const dependency of graph.get(id)) {
        queue.push({ id: dependency, trail: [...trail, relative(packages.get(dependency))] });
      }
    }
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-licence-boundary.mjs --root DIR --metadata FILE
Read cargo metadata --locked --all-features --format-version=1 JSON, with dependencies and resolve.nodes.
Reject dependencies inside the repository outside engine/, and reachable MPL/GPL/LGPL/AGPL expressions.
MPL licence files are also rejected. Metadata must cover all engine manifests on disk.
Metadata and licence files must be available in the same filesystem; no Cargo commands are executed.
Exit: 0 passed, 1 boundary violation, 2 usage or input error.`, { root: stringOption, metadata: stringOption }, check);
