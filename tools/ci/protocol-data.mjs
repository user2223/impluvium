import { InputError } from './common.mjs';
const bad = () => { throw new InputError('protocol contract needs recapture or has invalid interned data'); };
const at = (table, index) => Number.isInteger(index) && index >= 0 && index < table.length ? table[index] : bad();
const intern = table => {
  const indices = new Map();
  return value => {
    const key = JSON.stringify(value);
    if (!indices.has(key)) { indices.set(key, table.length); table.push(value); }
    return indices.get(key);
  };
};
export function compactContract(data) {
  const identities = [], packages = [], packageSets = [], nodes = [], requirementRows = [];
  const requirement = intern(requirementRows);
  const id = intern(identities), pkg = intern(packages), set = intern(packageSets), node = intern(nodes);
  const identity = value => id([value.name, value.version, value.source]);
  const permissions = [...data.permissions].sort((a, b) => JSON.stringify([a.profile, a.target, a.run]).localeCompare(JSON.stringify([b.profile, b.target, b.run])))
    .map(entry => ({ profile: entry.profile, target: entry.target, run: entry.run,
      set: set([...entry.packages].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))).map(value => pkg([identity(value), [...value.features].sort()])).sort((a, b) => a - b)) }));
  const exceptions = [...data.legacy.exceptions].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))).map(item => {
    if (item.path.length !== item.edges.length + 1) bad();
    let parent = -1;
    for (const [index, value] of item.path.entries()) parent = node([parent, identity(value), index ? item.edges[index - 1] : null]);
    return { root: item.root, leaf: parent, target: item.target, reason: item.reason, removalOwner: item.removalOwner };
  });
  return { ...data, encoding: 'interned-paths', identities, packages, packageSets, permissions,
    legacy: { baseSha: data.legacy.baseSha, nodes, exceptions, requirementRows,
      requirements: [...data.legacy.requirements].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))
        .map(row => [row[0], requirement(row.slice(1))]) } };
}
export function expandContract(data) {
  if (!data.encoding) return data;
  if (data.encoding !== 'interned-paths' || ![data.identities, data.packages, data.packageSets, data.permissions, data.legacy?.nodes].every(Array.isArray)) bad();
  const identity = index => {
    const row = at(data.identities, index);
    if (!Array.isArray(row) || row.length !== 3 || !row.slice(0, 2).every(value => typeof value === 'string' && value) || (row[2] !== null && typeof row[2] !== 'string')) bad();
    return { name: row[0], version: row[1], source: row[2] };
  };
  const packages = data.packages.map(row => {
    if (!Array.isArray(row) || row.length !== 2 || !Array.isArray(row[1]) || !row[1].every(value => typeof value === 'string')) bad();
    return { ...identity(row[0]), features: row[1] };
  });
  const chains = data.legacy.nodes.map((row, index) => {
    if (!Array.isArray(row) || row.length !== 3 || !Number.isInteger(row[0]) || row[0] < -1 || row[0] >= index ||
        (row[0] === -1 ? row[2] !== null : !row[2] || typeof row[2].alias !== 'string' || !['normal', 'build', 'dev'].includes(row[2].kind) ||
          (row[2].target !== null && typeof row[2].target !== 'string'))) bad();
    return { parent: row[0], pkg: identity(row[1]), edge: row[2] };
  });
  const exceptions = data.legacy.exceptions === null ? null : data.legacy.exceptions.map(item => {
    const path = [], edges = []; let current = at(chains, item.leaf);
    while (current) {
      path.unshift(current.pkg);
      if (current.edge) edges.unshift(current.edge);
      current = current.parent === -1 ? null : at(chains, current.parent);
    }
    return { root: item.root, path, edges, target: item.target, reason: item.reason, removalOwner: item.removalOwner };
  });
  const requirements = data.legacy.requirements == null ? null : data.legacy.requirements.map(row => {
    if (!Array.isArray(row) || row.length !== 2 || typeof row[0] !== 'string' || !Array.isArray(data.legacy.requirementRows)) bad();
    const values = at(data.legacy.requirementRows, row[1]);
    if (!Array.isArray(values)) bad();
    return [row[0], ...values];
  });
  const expanded = { ...data, permissions: data.permissions.map(entry => {
    const rows = at(data.packageSets, entry.set);
    if (!Array.isArray(rows) || !rows.length) bad();
    return { profile: entry.profile, target: entry.target, run: entry.run, packages: rows.map(index => at(packages, index)) };
  }), legacy: { baseSha: data.legacy.baseSha, exceptions, requirements } };
  for (const key of ['encoding', 'identities', 'packages', 'packageSets']) delete expanded[key];
  return expanded;
}
