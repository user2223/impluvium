import path from 'node:path';
import { fail } from './protocol-contract.mjs';
import { plainObject } from './common.mjs';
import { toml } from './toml.mjs';
import { engineManifests } from './engine-manifests.mjs';

const toolManifests = {
  'impluvium-protocol-xtask': 'engine/protocol/xtask/Cargo.toml',
  'impluvium-hashchunk-bench': 'engine/protocol/benchmarks/hashchunk/Cargo.toml',
};

const badPackages = new Set(['ring', 'native-tls', 'openssl', 'openssl-sys', 'prost', 'pbjson', 'tonic', 'flate2', 'zstd']);
const badFeatures = { connectrpc: ['gzip', 'zstd', 'client-tls'], reqwest: ['system-proxy'], rustls: ['ring'] };

export function engineCoverage(metadata, root, data, source) {
  if (!Array.isArray(source?.files)) fail('engine coverage needs the attested file catalogue');
  const covered = new Set(Object.values(data.profiles).flatMap(profile => profile.roots));
  const manifests = new Set(metadata.packages.map(pkg => path.resolve(pkg.manifest_path)));
  for (const file of engineManifests(root, source)) {
    const parsed = toml(path.join(root, file)).data;
    if (plainObject(parsed.workspace) && !Object.hasOwn(parsed, 'package') && path.dirname(path.join(root, file)) === metadata.workspace_root) continue;
    if (!manifests.has(path.join(root, file))) fail(`unassigned engine manifest missing from metadata: ${file}`);
  }
  for (const pkg of metadata.packages.filter(pkg => pkg.source == null)) {
    const relative = path.relative(root, pkg.manifest_path).split(path.sep).join('/');
    if (!relative.startsWith('engine/')) continue;
    const entry = data.enginePackages?.[pkg.name];
    if (!entry || entry.manifest !== relative || !['product', 'tool'].includes(entry.kind) ||
        (entry.kind === 'product' && !covered.has(pkg.name)) ||
        (entry.kind === 'tool' && toolManifests[pkg.name] !== relative)) fail(`unassigned engine package: ${pkg.name}`);
  }
}

export function workspacePaths(metadata, target, root, data, source) {
  if (metadata.workspace_root !== root) fail('workspace metadata belongs to a different export');
  if (!data) fail('workspace capture needs explicit engine classifications');
  engineCoverage(metadata, root, data, source);
  const packages = new Map(metadata.packages.map(pkg => [pkg.id, pkg]));
  const graph = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const reasonsFor = (pkg, node) => [
    ...(badPackages.has(pkg.name) ? [`package:${pkg.name}`] : []),
    ...(badFeatures[pkg.name] ?? []).filter(feature => node.features.includes(feature)).map(feature => `feature:${pkg.name}/${feature}`),
  ];
  const parents = new Map(), relevant = new Set();
  for (const [id, node] of graph) {
    const pkg = packages.get(id);
    if (!pkg || !Array.isArray(node.features) || !Array.isArray(node.deps)) fail('incomplete workspace graph');
    if (reasonsFor(pkg, node).length) relevant.add(id);
    for (const dep of node.deps) {
      if (!graph.has(dep.pkg) || !Array.isArray(dep.dep_kinds) || !dep.dep_kinds.length) fail('incomplete workspace dependency edge');
      if (!parents.has(dep.pkg)) parents.set(dep.pkg, new Set());
      parents.get(dep.pkg).add(id);
    }
  }
  // Enumerate every offending path, without expanding harmless subgraphs.
  const pending = [...relevant];
  for (let index = 0; index < pending.length; index++) for (const parent of parents.get(pending[index]) ?? []) {
    if (!relevant.has(parent)) { relevant.add(parent); pending.push(parent); }
  }
  const paths = [];
  for (const id of metadata.workspace_members) {
    const start = packages.get(id), relative = path.relative(root, start.manifest_path).split(path.sep).join('/');
    if (path.isAbsolute(relative) || relative === '..' || relative.startsWith('../')) fail('workspace member lies outside the captured export');
    if (relative.startsWith('engine/')) continue;
    const removalOwner = relative.startsWith('desktop/') ? 'desktop client migration' : 'inherited depot transport migration';
    const visit = (current, route, edges, ancestors) => {
      if (ancestors.has(current) || !relevant.has(current)) return;
      const pkg = packages.get(current), node = graph.get(current);
      if (!pkg || !node || !Array.isArray(node.features) || !Array.isArray(node.deps)) fail('incomplete workspace graph');
      const trail = [...route, { name: pkg.name, version: pkg.version, source: pkg.source ?? null }];
      for (const reason of reasonsFor(pkg, node)) paths.push({ root: relative, path: trail, edges, target, reason, removalOwner });
      const seen = new Set(ancestors).add(current);
      for (const dep of node.deps) for (const kind of dep.dep_kinds) {
        visit(dep.pkg, trail, [...edges, { alias: dep.name, kind: kind.kind ?? 'normal', target: kind.target }], seen);
      }
    };
    visit(id, [], [], new Set());
  }
  return [...new Map(paths.map(item => [JSON.stringify(item), item])).values()].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}
