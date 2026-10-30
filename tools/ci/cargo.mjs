import path from 'node:path';
import { files, InputError, plainObject, problem } from './common.mjs';
import { toml } from './toml.mjs';

export function manifests(root) {
  return files(root).filter(file => path.basename(file) === 'Cargo.toml').map(file => ({
    ...toml(path.join(root, file)), relative: file, directory: path.dirname(file),
  }));
}

export function workspaceFor(manifest, all) {
  const explicit = manifest.data.package?.workspace;
  const candidates = all.filter(item => item.data.workspace && (typeof explicit === 'string'
    ? path.normalize(path.join(manifest.directory, explicit, 'Cargo.toml')) === item.relative
    : item.directory === '.' || manifest.directory === item.directory || manifest.directory.startsWith(item.directory + '/')));
  return candidates.sort((a, b) => b.directory.length - a.directory.length)[0];
}

export function dependencies(manifest) {
  const result = [];
  const collect = object => {
    if (!plainObject(object)) return;
    for (const [key, value] of Object.entries(object)) {
      if (['dependencies', 'dev-dependencies', 'build-dependencies'].includes(key)) {
        for (const [alias, dependency] of Object.entries(value)) {
          const entry = manifest.entries.find(item => item.keys.includes(key) && item.keys[item.keys.indexOf(key) + 1] === alias);
          result.push({ alias, value: dependency, line: entry?.line ?? 1 });
        }
      } else if (key === 'target') for (const target of Object.values(value)) collect(target);
    }
  };
  collect(manifest.data);
  return result;
}

export function localPath(manifest, dependency, all) {
  let base = manifest;
  let value = dependency.value;
  if (value?.workspace === true) {
    base = workspaceFor(manifest, all);
    value = base?.data.workspace?.dependencies?.[dependency.alias];
    if (!base || value === undefined) throw new InputError(problem(manifest.file, dependency.line, `cannot resolve workspace dependency ${dependency.alias}`));
  }
  if (typeof value?.path !== 'string') return undefined;
  const directory = path.normalize(path.join(base.directory, value.path));
  if (path.isAbsolute(value.path) || directory === '..' || directory.startsWith('../')) {
    throw new InputError(problem(manifest.file, dependency.line, 'dependency path leaves the repository'));
  }
  return path.posix.normalize(directory + '/Cargo.toml');
}
