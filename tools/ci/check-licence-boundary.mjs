import path from 'node:path';
import { existsSync } from 'node:fs';
import { installedGraphs, referencePath } from './reference-boundary.mjs';
import { cli, InputError, plainObject, problem, required, stringOption } from './common.mjs';
import { engineManifests } from './engine-manifests.mjs';
import { cargoMetadata } from './metadata.mjs';
import { toml } from './toml.mjs';
import { policy } from './spdx.mjs';
import { inside, traverse } from './licence-graph.mjs';
import { pnpmGraph } from './pnpm-licences.mjs';

export function cargoGraph(file, root) {
  const metadata = cargoMetadata(file, { resolved: true });
  if (!metadata.packages.length || !metadata.workspace_members.length) throw new InputError(problem(file, 1, 'metadata must contain workspace members'));
  const packages = new Map();
  for (const pkg of metadata.packages) {
    if (!path.isAbsolute(pkg.manifest_path)) throw new InputError(problem(file, 1, 'metadata manifest paths must be absolute'));
    packages.set(pkg.id, { id: pkg.id, name: pkg.name, manifest: pkg.manifest_path, license: pkg.license, licenseFile: pkg.license_file });
  }
  for (const id of metadata.workspace_members) {
    if (!inside(root, packages.get(id).manifest)) throw new InputError(problem(file, 1, 'workspace member manifest is outside --root'));
  }
  const manifests = new Set([...packages.values()].map(pkg => path.resolve(pkg.manifest)));
  for (const file of engineManifests(root)) {
    const manifest = toml(path.join(root, file)).data;
    if (plainObject(manifest.workspace) && !Object.hasOwn(manifest, 'package') && path.dirname(path.join(root, file)) === metadata.workspace_root) continue;
    if (!manifests.has(path.join(root, file))) throw new InputError(problem(file, 1, 'engine manifest missing from metadata.packages'));
  }
  // All resolved normal, build, dev and target-specific edges remain in scope.
  const graph = new Map(metadata.resolve.nodes.map(node => [node.id, node.dependencies]));
  const roots = [...packages.values()].filter(pkg => inside(path.join(root, 'engine'), pkg.manifest)).map(pkg => pkg.id);
  if (!roots.length) throw new InputError(problem(file, 1, 'no engine/ packages selected'));
  return { packages, graph, roots };
}

export function check(options) {
  required(options, 'root', 'metadata', 'pnpm-graph', 'pnpm-lock');
  const root = path.resolve(options.root), cargo = cargoGraph(options.metadata, root), rules = policy(path.join(root, 'deny.toml'));
  const npm = pnpmGraph(root, options['pnpm-graph'], options['pnpm-lock']);
  if (existsSync(path.join(root, referencePath, 'package.json'))) installedGraphs(root);
  return [
    ...traverse(cargo, root, rules),
    ...traverse(npm, root, rules),
  ];
}

await cli(import.meta.url, `Usage: node tools/ci/check-licence-boundary.mjs --root DIR --metadata FILE --pnpm-graph FILE --pnpm-lock FILE
Read resolved Cargo metadata for all features and targets and a fully installed pnpm list --depth Infinity --json graph.
Apply deny.toml SPDX admission and the stricter engine copyleft/path boundary through both adapters.
Read installed npm manifests and license files and cross-check every engine importer and lock identity.
Missing roots, manifests, licenses, links, lock identities or edges are input errors. No tools are executed.
Exit: 0 passed, 1 boundary violation, 2 usage or input error.`, {
  root: stringOption, metadata: stringOption, 'pnpm-graph': stringOption, 'pnpm-lock': stringOption,
}, check);
