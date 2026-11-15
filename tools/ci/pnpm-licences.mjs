import path from 'node:path';
import { realpathSync, existsSync, lstatSync } from 'node:fs';
import { InputError, json, plainObject } from './common.mjs';
import { engineManifests } from './engine-manifests.mjs';
import { yaml } from './yaml.mjs';
import { inside } from './licence-graph.mjs';

const sections = ['dependencies', 'devDependencies', 'optionalDependencies'];
const unwrap = node => {
  if (Array.isArray(node?.value)) return node.value.map(unwrap);
  if (plainObject(node?.value)) return Object.fromEntries(Object.entries(node.value).map(([key, value]) => [key, unwrap(value)]));
  return node?.value;
};
export const yamlData = file => unwrap(yaml(file));
const fail = message => { throw new InputError(`<pnpm-graph>:1: ${message}`); };
const baseVersion = version => String(version).split('(')[0];
const repositoryPath = (root, directory) => inside(root, directory) && !directory.split(path.sep).includes('node_modules');
function installedDependency(owner, alias) {
  if (!/^(?:@[a-z0-9._-]+\/)?[a-z0-9._-]+$/i.test(alias) || alias === '.' || alias === '..') fail(`invalid npm alias ${alias}`);
  for (let current = owner; ; current = path.dirname(current)) {
    if (path.basename(current) !== 'node_modules') {
      const candidate = path.join(current, 'node_modules', alias);
      let present = false;
      try { lstatSync(candidate); present = true; }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
      if (present) return realpathSync(candidate);
    }
    if (path.dirname(current) === current) break;
  }
  fail(`installed dependency link is missing: ${owner} -> ${alias}`);
}

export function pnpmGraph(root, graphFile, lockFile) {
  const lock = yamlData(lockFile), workspace = yamlData(path.join(root, 'pnpm-workspace.yaml'));
  if (lock.lockfileVersion !== '9.0' || !plainObject(lock.importers) || !plainObject(lock.packages) || !plainObject(lock.snapshots)) fail('expected complete pnpm lock version 9');
  if (!Array.isArray(workspace.packages) || !workspace.packages.every(item => typeof item === 'string')) fail('missing workspace membership');
  const source = json(graphFile);
  if (!Array.isArray(source)) fail('expected pnpm list JSON array');
  const packages = new Map(), graph = new Map(), byPath = new Map(), roots = [];
  const manifests = new Map();
  const member = relative => workspace.packages.some(pattern => {
    pattern = pattern.replace(/\/$/, '');
    if (!pattern.includes('*')) return pattern === relative;
    const regex = '^' + pattern.split('*').map(item => item.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('[^/]+') + '$';
    return new RegExp(regex).test(relative);
  });
  for (const file of engineManifests(root, undefined, 'package.json')) {
    const directory = path.dirname(file);
    if (!repositoryPath(root, path.join(root, directory))) continue;
    if (!member(directory)) fail(`engine package is not a workspace member: ${directory}`);
    manifests.set(realpathSync(path.join(root, directory)), json(path.join(root, file)));
    if (!Object.hasOwn(lock.importers, directory)) fail(`engine importer missing from lock: ${directory}`);
  }
  if (!manifests.size) fail('no engine npm workspace roots');

  const register = (directory, evidence, rootNode = false) => {
    if (typeof directory !== 'string' || !path.isAbsolute(directory)) fail('package path must be absolute');
    const real = realpathSync(directory), manifestFile = path.join(real, 'package.json'), manifest = json(manifestFile);
    if (typeof manifest.name !== 'string' || typeof manifest.version !== 'string') fail(`manifest identity missing: ${manifestFile}`);
    const observedName = rootNode ? evidence.name : evidence.from;
    if (observedName !== manifest.name || (typeof evidence.version !== 'string') ||
        (!evidence.version.startsWith('link:') && baseVersion(evidence.version) !== manifest.version)) fail(`graph/manifest identity differs: ${manifestFile}`);
    const id = real;
    const existing = byPath.get(real);
    if (existing && (existing.name !== manifest.name || existing.version !== manifest.version)) fail(`ambiguous identity at ${real}`);
    byPath.set(real, manifest);
    packages.set(id, { id, name: manifest.name, manifest: manifestFile, license: manifest.license, repository: repositoryPath(root, real),
      licenseFile: manifest.licenseFile ?? (/^SEE LICEN[CS]E IN (.+)$/.exec(manifest.license ?? '')?.[1]) });
    if (!graph.has(id)) graph.set(id, []);
    return { id, manifest, real };
  };

  const linkTarget = (owner, version) => path.resolve(owner, version.slice('link:'.length));
  const checkEdge = (owner, alias, entry, observed, child) => {
    if (!entry || typeof entry.version !== 'string') fail(`missing lock edge ${owner.real} -> ${alias}`);
    if (entry.version.startsWith('link:')) {
      if (realpathSync(linkTarget(owner.real, entry.version)) !== child.real) fail(`link target differs for ${alias}`);
      return;
    }
    let reference = entry.version;
    const aliasReference = /^\d+\.\d+\.\d/.test(reference) ? null : /^(@[^/()]+\/[^@()]+|[^@()]+)@(.+)$/.exec(reference);
    if (aliasReference) {
      if (aliasReference[1] !== child.manifest.name) fail(`alias identity differs for ${alias}`);
      reference = aliasReference[2];
    }
    if (baseVersion(reference) !== child.manifest.version) fail(`lock version differs for ${alias}`);
    const key = `${child.manifest.name}@${reference}`;
    if (!Object.hasOwn(lock.packages, `${child.manifest.name}@${child.manifest.version}`) || !Object.hasOwn(lock.snapshots, key)) fail(`missing package/snapshot ${key}`);
    // pnpm encodes peer context in the virtual-store directory. A version-only
    // graph may not silently match a different peer snapshot.
    const store = child.real.split(`${path.sep}.pnpm${path.sep}`)[1]?.split(path.sep)[0];
    const encoded = key.replaceAll('/', '+').replaceAll('(', '_').replaceAll(')', '');
    if (store !== encoded) fail(`installed package/peer context differs for ${key}`);
    if (observed.version.includes('(') && observed.version !== reference) fail(`graph peer version differs for ${alias}`);
    return lock.snapshots[key];
  };

  const visited = new Set();
  const visit = (owner, evidence, locked, rootNode = false) => {
    if (visited.has(owner.id)) return;
    visited.add(owner.id);
    if (Object.keys(evidence.unsavedDependencies ?? {}).length) fail('unsaved installed dependencies are not locked');
    if (rootNode) {
      const seen = new Map();
      for (const section of sections) for (const alias of new Set([
        ...Object.keys(owner.manifest[section] ?? {}), ...Object.keys(locked[section] ?? {}), ...Object.keys(evidence[section] ?? {}),
      ])) {
        if (typeof owner.manifest[section]?.[alias] !== 'string' || locked[section]?.[alias]?.specifier !== owner.manifest[section][alias] || !evidence[section]?.[alias]) fail(`incomplete importer section ${section}/${alias}`);
        const node = evidence[section][alias], identity = JSON.stringify([node.path, node.from, node.version]);
        if (seen.has(alias) && seen.get(alias) !== identity) fail(`ambiguous installed importer alias ${alias}`);
        seen.set(alias, identity);
      }
    }
    const observed = rootNode ? Object.assign({}, ...sections.map(section => evidence[section] ?? {})) : evidence.dependencies ?? {};
    if (!plainObject(observed)) fail('invalid dependency object');
    const declarations = Object.assign({}, ...sections.filter(section => rootNode || section !== 'devDependencies').map(section => owner.manifest[section] ?? {}));
    // Installed peer dependencies are production edges too.
    Object.assign(declarations, owner.manifest.peerDependencies ?? {});
    const lockedEdges = rootNode ? Object.assign({}, ...sections.map(section => locked[section] ?? {})) :
      Object.assign({}, locked?.dependencies ?? {}, locked?.optionalDependencies ?? {});
    for (const alias of new Set([...Object.keys(declarations), ...Object.keys(lockedEdges), ...Object.keys(observed)])) {
      const node = observed[alias];
      if (!node || typeof node.path !== 'string') fail(`missing installed graph edge ${owner.manifest.name} -> ${alias}`);
      const child = register(node.path, node);
      if (installedDependency(owner.real, alias) !== child.real) fail(`installed alias/link differs from the graph: ${alias}`);
      const raw = lockedEdges[alias];
      const entry = rootNode ? raw : { version: raw };
      if (rootNode && declarations[alias] !== entry?.specifier) fail(`manifest/lock specifier differs for ${alias}`);
      const snapshot = checkEdge(owner, alias, entry, node, child);
      graph.get(owner.id).push(child.id);
      if (repositoryPath(root, child.real)) {
        const relative = path.relative(root, child.real).split(path.sep).join('/');
        if (!member(relative) || !lock.importers[relative]) fail(`linked workspace importer missing: ${relative}`);
        const listed = source.find(item => item.path && existsSync(item.path) && realpathSync(item.path) === child.real);
        if (!listed && Object.keys(child.manifest.devDependencies ?? {}).length) fail(`linked workspace dev graph missing: ${relative}`);
        visit(child, listed ?? { ...node, name: child.manifest.name, dependencies: node.dependencies ?? {} }, lock.importers[relative], true);
      } else visit(child, node, snapshot, false);
    }
  };
  for (const [directory] of manifests) {
    const entries = source.filter(item => typeof item.path === 'string' && existsSync(item.path) && realpathSync(item.path) === directory);
    if (entries.length !== 1) fail(`engine root must occur exactly once: ${directory}`);
    const owner = register(directory, entries[0], true);
    roots.push(owner.id);
    visit(owner, entries[0], lock.importers[path.relative(root, directory).split(path.sep).join('/')], true);
  }
  return { packages, graph, roots };
}
