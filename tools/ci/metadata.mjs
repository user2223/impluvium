import { InputError, json, problem } from './common.mjs';

// Cargo package IDs are opaque: never infer names or paths from their spelling.
export function cargoMetadata(file, { resolved = false, targets = false } = {}) {
  const data = json(file);
  const fail = message => { throw new InputError(problem(file, 1, message)); };
  if (!Array.isArray(data?.packages) || !data.packages.every(item =>
    item && typeof item.id === 'string' && typeof item.name === 'string')) {
    fail('expected cargo metadata packages with id and name');
  }
  const ids = new Set(data.packages.map(item => item.id));
  if (ids.size !== data.packages.length || !Array.isArray(data.workspace_members) ||
      !data.workspace_members.every(id => ids.has(id))) fail('invalid cargo metadata workspace_members or duplicate package IDs');
  const names = data.packages.filter(item => data.workspace_members.includes(item.id)).map(item => item.name);
  if (new Set(names).size !== names.length) fail('ambiguous workspace package names');
  if (targets && !data.packages.every(item => Array.isArray(item.targets) && item.targets.every(target =>
    target && typeof target.name === 'string' && Array.isArray(target.kind) && target.kind.every(kind => typeof kind === 'string')))) {
    fail('expected cargo metadata targets with name and kind');
  }
  if (resolved) {
    if (!data.packages.every(item => typeof item.manifest_path === 'string') || !Array.isArray(data.resolve?.nodes)) {
      fail('expected resolved cargo metadata with manifest_path and resolve.nodes (omit --no-deps)');
    }
    const nodes = data.resolve.nodes;
    const nodeIds = new Set(nodes.map(node => node?.id));
    if (nodeIds.size !== nodes.length || !nodes.every(node => node && ids.has(node.id) && Array.isArray(node.dependencies) &&
        node.dependencies.every(id => ids.has(id) && nodeIds.has(id)))) fail('invalid or incomplete resolve.nodes graph');
    if (!data.workspace_members.every(id => nodeIds.has(id))) fail('workspace package missing from resolve.nodes');
  }
  return data;
}
