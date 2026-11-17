import path from 'node:path';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import { createHash } from 'node:crypto';
import { InputError, json } from './common.mjs';
import { yamlData } from './pnpm-licences.mjs';

export const referencePath = 'engine/protocol/conformance/reference';
export const scriptNames = ['typecheck', 'imports', 'corpus', 'coverage', 'primitives', 'vectors', 'limits', 'counterexamples'];
export const expectedPolicy = {
  "version": 1,
  "runtime": {
    "packages": {
      "@noble/hashes": "1.8.0"
    },
    "entrySpecifiers": [
      "@noble/hashes/blake3"
    ],
    "entries": {
      "local": "tooling/stage.mjs",
      "contract": "tooling/check.mjs",
      "container": "container/run.mjs"
    },
    "dependencyEntry": "container/run.mjs",
    "modules": {
      "src/api.ts": {
        "builtins": []
      },
      "src/auth-rpc.ts": {
        "builtins": []
      },
      "src/documents.ts": {
        "builtins": []
      },
      "src/ed25519.ts": {
        "builtins": [
          "node:crypto"
        ]
      },
      "src/events.ts": {
        "builtins": []
      },
      "src/fastcdc.ts": {
        "builtins": []
      },
      "src/flows.ts": {
        "builtins": []
      },
      "src/format-gates.ts": {
        "builtins": [
          "node:assert"
        ]
      },
      "src/gates.ts": {
        "builtins": [
          "node:assert",
          "node:crypto"
        ],
        "cryptoImports": [
          "createPrivateKey",
          "sign"
        ]
      },
      "src/get-routes.ts": {
        "builtins": []
      },
      "src/json-profile.ts": {
        "builtins": []
      },
      "src/loader.ts": {
        "builtins": [
          "node:fs/promises",
          "node:constants",
          "node:url",
          "node:path",
          "node:crypto"
        ],
        "filesystemImports": [
          "lstat",
          "readdir",
          "open"
        ],
        "constantImports": [
          "O_RDONLY",
          "O_NOFOLLOW"
        ],
        "openFlags": [
          "O_RDONLY",
          "O_NOFOLLOW"
        ],
        "handleMethods": [
          "stat",
          "readFile",
          "close"
        ]
      },
      "src/locations.ts": {
        "builtins": []
      },
      "src/models.ts": {
        "builtins": []
      },
      "src/paths.ts": {
        "builtins": []
      },
      "src/primitives.ts": {
        "builtins": [
          "node:crypto"
        ]
      },
      "src/profile.ts": {
        "builtins": []
      },
      "src/provenance.ts": {
        "builtins": [
          "node:assert",
          "node:crypto"
        ]
      },
      "src/publication.ts": {
        "builtins": [
          "node:assert",
          "node:path",
          "node:url"
        ]
      },
      "src/recipes.ts": {
        "builtins": [
          "node:crypto"
        ]
      },
      "src/rpc.ts": {
        "builtins": []
      },
      "src/state-contracts.ts": {
        "builtins": []
      },
      "src/transport.ts": {
        "builtins": []
      },
      "tooling/check.mjs": {
        "builtins": []
      },
      "tooling/imports.mjs": {
        "builtins": [
          "node:path",
          "node:url"
        ]
      },
      "tooling/options.mjs": {
        "builtins": [
          "node:path",
          "node:url"
        ]
      },
      "tooling/output.mjs": {
        "builtins": [
          "node:fs/promises",
          "node:constants",
          "node:crypto",
          "node:path",
          "node:url"
        ],
        "filesystemImports": [
          "mkdir",
          "realpath",
          "lstat",
          "stat",
          "open",
          "rename",
          "unlink"
        ],
        "constantImports": [
          "O_CREAT",
          "O_EXCL",
          "O_WRONLY",
          "O_NOFOLLOW"
        ],
        "openFlags": [
          "O_CREAT",
          "O_EXCL",
          "O_WRONLY",
          "O_NOFOLLOW"
        ],
        "handleMethods": [
          "writeFile",
          "sync",
          "close"
        ],
        "mutableHandleName": "handle",
        "cryptoImports": [
          "randomBytes"
        ]
      },
      "tooling/stage.mjs": {
        "builtins": [
          "node:crypto"
        ]
      },
      "container/blake3.ts": {
        "builtins": [
          "node:assert"
        ]
      },
      "container/run.mjs": {
        "builtins": [],
        "packages": [
          "@noble/hashes/blake3"
        ]
      },
      "src/counterexamples.ts": {
        "builtins": [
          "node:assert"
        ]
      },
      "src/coverage.ts": {
        "builtins": [
          "node:crypto"
        ],
        "cryptoImports": [
          "createHash"
        ]
      }
    },
    "dependencyLicenses": {
      "@noble/hashes": {
        "license": "MIT",
        "file": "LICENSE",
        "sha256": "4f221aee6e072336700c408c68ab3b96a3fc09f6aebe6f48f1bd99e5ef13faec"
      }
    },
    "dependencyModules": {
      "@noble/hashes": {
        "esm/cryptoNode.js": [
          "node:crypto"
        ],
        "cryptoNode.js": [
          "node:crypto"
        ]
      }
    }
  },
  "typeTools": {
    "typescript": "5.9.3",
    "@types/node": "22.20.5",
    "undici-types": "6.21.0"
  }
};
export const boundaryError = message => { throw new InputError(`<reference>:1: ${message}`); };
export const within = (root, file) => file === root || file.startsWith(root + path.sep);
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');
export function checkManifest(root) {
  const ref = path.join(root, referencePath), manifest = json(path.join(ref, 'package.json'));
  const expected = {
    name: 'impluvium-protocol-reference', version: '0.1.0', private: true, license: '0BSD', type: 'module', engines: { node: '^24.0.0' },
    scripts: Object.fromEntries(scriptNames.map(name => [name, `node ../../../../tools/ci/reference-run.mjs ${name}`])),
    devDependencies: { '@noble/hashes': '1.8.0', '@types/node': '22.20.5', typescript: '5.9.3' },
  };
  if (!isDeepStrictEqual(manifest, expected)) boundaryError('reference manifest differs from the private test-only package contract');
  if (!isDeepStrictEqual(json(path.join(ref, 'import-policy.json')), expectedPolicy)) boundaryError('reference import policy differs from the closed allowlist');
  const workspace = yamlData(path.join(root, 'pnpm-workspace.yaml'));
  if (workspace.packages?.filter(item => item === referencePath + '/').length !== 1 ||
      workspace.packages.some(item => item.includes('*') && item.startsWith('engine'))) boundaryError('reference needs exactly its explicit workspace member');
  const visit = directory => {
    for (const item of readdirSync(directory, { withFileTypes: true })) {
      if (item.name === 'node_modules') {
        if (directory === ref && item.isDirectory()) continue;
        boundaryError('only the exact reference node_modules directory is permitted');
      }
      const file = path.join(directory, item.name);
      if (item.isSymbolicLink() || !item.isDirectory() && !item.isFile()) boundaryError(`symlink or special reference input: ${file}`);
      if (item.isDirectory()) visit(file);
      else if (/^(?:pnpm-lock\.yaml|package-lock\.json|npm-shrinkwrap\.json|yarn\.lock)$/.test(item.name)) boundaryError('only the root lock is permitted');
    }
  };
  visit(ref);
  return { ref, manifest };
}

// Inspect exact dependency notice bytes as well as the SPDX expression. A
// different permissive expression is still a change to this reviewed package.
export function verifyReferenceDependency(directory, manifest) {
  const expected = expectedPolicy.runtime.dependencyLicenses[manifest.name];
  if (!expected || manifest.version !== expectedPolicy.runtime.packages[manifest.name] || manifest.license !== expected.license) boundaryError('reference dependency version/licence differs');
  const file = path.join(directory, expected.file), info = lstatSync(file);
  if (!info.isFile() || info.isSymbolicLink() || digest(readFileSync(file)) !== expected.sha256) boundaryError('reference dependency licence notice differs');
  return { name: manifest.name, version: manifest.version, license: manifest.license, file, sha256: expected.sha256 };
}

// Inspect installed files and root-lock edges; never execute a dependency here.
export function installedGraphs(root) {
  const { ref, manifest } = checkManifest(root), lock = yamlData(path.join(root, 'pnpm-lock.yaml'));
  const importer = lock.importers?.[referencePath];
  if (lock.lockfileVersion !== '9.0' || !importer || !lock.packages || !lock.snapshots) boundaryError('missing complete reference root-lock importer');
  const pins = { ...expectedPolicy.runtime.packages, ...expectedPolicy.typeTools };
  const packages = new Map(), edges = [], store = realpathSync(path.join(root, 'node_modules/.pnpm'));
  const dependency = (owner, alias, version) => {
    if (!Object.hasOwn(pins, alias) || version !== pins[alias]) boundaryError(`unapproved lock edge ${alias}@${version}`);
    let link;
    for (let dir = owner; within(root, dir); dir = path.dirname(dir)) {
      const candidate = path.join(dir, 'node_modules', alias);
      try { lstatSync(candidate); link = candidate; break; } catch (error) { if (error.code !== 'ENOENT') throw error; }
    }
    if (!link) boundaryError(`missing installed dependency ${alias}`);
    const real = realpathSync(link), expected = path.join(store, `${alias.replaceAll('/', '+')}@${version}`, 'node_modules', alias);
    if (real !== expected) boundaryError(`unapproved symlink/lock target ${alias}: ${real}`);
    const value = json(path.join(real, 'package.json')), key = `${alias}@${version}`;
    if (value.name !== alias || value.version !== version || !/^sha512-[A-Za-z0-9+/]{86}==$/.test(lock.packages[key]?.resolution?.integrity ?? '') || !lock.snapshots[key]) boundaryError(`package identity/integrity/snapshot differs for ${key}`);
    edges.push({ from: path.relative(root, owner), alias, to: path.relative(root, real), version });
    if (packages.has(real)) return real;
    packages.set(real, value);
    if (Object.hasOwn(expectedPolicy.runtime.dependencyLicenses, alias)) verifyReferenceDependency(real, value);
    const declared = { ...value.dependencies, ...value.optionalDependencies, ...value.peerDependencies };
    const locked = { ...lock.snapshots[key].dependencies, ...lock.snapshots[key].optionalDependencies };
    if (!isDeepStrictEqual(Object.keys(declared).sort(), Object.keys(locked).sort())) boundaryError(`incomplete transitive lock edges for ${key}`);
    for (const name of Object.keys(declared)) dependency(real, name, locked[name]);
    return real;
  };
  if (Object.keys(importer).some(key => key !== 'devDependencies') ||
      !isDeepStrictEqual(Object.keys(importer.devDependencies ?? {}).sort(), Object.keys(manifest.devDependencies).sort())) boundaryError('reference importer dependency sections differ');
  for (const [name, version] of Object.entries(manifest.devDependencies)) {
    const entry = importer.devDependencies[name];
    if (entry?.specifier !== version || entry.version !== version) boundaryError(`manifest/lock pin differs for ${name}`);
    dependency(ref, name, version);
  }
  const runtime = [...packages].filter(([, value]) => Object.hasOwn(expectedPolicy.runtime.packages, value.name)).map(([file]) => file);
  const typeTools = [...packages].filter(([, value]) => Object.hasOwn(expectedPolicy.typeTools, value.name)).map(([file]) => file);
  return { ref, packages, runtime, typeTools, edges };
}

function condition(value, conditions) {
  if (typeof value === 'string' || value === null) return value;
  if (!value || Array.isArray(value) || typeof value !== 'object') boundaryError('unanalyzable package condition/fallback');
  for (const [name, entry] of Object.entries(value)) {
    if (name === 'default' || conditions.has(name)) return condition(entry, conditions);
  }
  return null;
}
function mapping(map, key, conditions) {
  if (typeof map === 'string' || map === null || map && !Object.keys(map).some(name => name.startsWith('.') || name.startsWith('#'))) {
    return key === '.' ? condition(map, conditions) : null;
  }
  if (Object.hasOwn(map, key)) return condition(map[key], conditions);
  const matches = Object.keys(map).filter(name => name.includes('*') && key.startsWith(name.split('*')[0]) && key.endsWith(name.split('*')[1]));
  matches.sort((a, b) => b.split('*')[0].length - a.split('*')[0].length || b.length - a.length);
  if (!matches.length) return null;
  const match = matches[0], [before, after] = match.split('*');
  const result = condition(map[match], conditions);
  return result?.replaceAll('*', key.slice(before.length, after ? -after.length : undefined)) ?? null;
}
// Shared admission for AST edges and dependency-free resolver counterexamples.
// Resolve known targets before rejecting forbidden loading modes, so diagnostics
// retain the actual offending path. Unknown expressions have no resolved target.
export function admitReferenceImport(specifier, owner, context, type = false, mode = 'static') {
  if (!['static', 'dynamic', 'require'].includes(mode)) boundaryError(`unanalyzable import mode in ${owner}`);
  if (typeof specifier !== 'string') boundaryError(`forbidden ${mode} import: unanalyzable target in ${owner}`);
  const target = resolveEdge(specifier, owner, context, type, mode === 'require');
  if (within(context.ref, owner) && mode !== 'static') boundaryError(`forbidden ${mode === 'require' ? 'require' : 'dynamic import'} in ${owner}: resolved ${target}`);
  return target;
}
export function resolveEdge(specifier, owner, context, type = false, requireMode = false, seen = new Set()) {
  const { ref, packages, runtime } = context;
  type = type || /\.d\.[cm]?ts$/.test(owner);
  if (typeof specifier !== 'string' || !specifier || /[\\\0%?]/.test(specifier)) boundaryError('unanalyzable import specifier');
  if (specifier.startsWith('node:')) {
    const own = within(ref, owner);
    const moduleName = path.relative(ref, owner).split(path.sep).join('/');
    const dependency = [...packages].find(([directory]) => within(directory, owner));
    const allowed = own ? expectedPolicy.runtime.modules[moduleName]?.builtins ?? [] :
      expectedPolicy.runtime.dependencyModules[dependency?.[1]?.name]?.[dependency ? path.relative(dependency[0], owner).split(path.sep).join('/') : ''] ?? [];
    if (!allowed.includes(specifier)) boundaryError(`forbidden builtin ${specifier} in ${moduleName}`);
    return specifier;
  }
  let file;
  const scope = [...packages.keys()].find(dir => within(dir, owner)) ?? ref;
  const conditions = new Set(['node', requireMode ? 'require' : 'import', ...(type ? ['types'] : [])]);
  if (specifier.startsWith('#')) {
    if (seen.has(specifier)) boundaryError('cyclic package imports');
    seen.add(specifier);
    const manifest = scope === ref ? json(path.join(ref, 'package.json')) : packages.get(scope);
    const target = manifest.imports && mapping(manifest.imports, specifier, conditions);
    if (typeof target !== 'string') boundaryError(`unresolved package imports ${specifier}`);
    return target.startsWith('./') ? admit(path.resolve(scope, target)) : resolveEdge(target, owner, context, type, requireMode, seen);
  }
  if (specifier.startsWith('./') || specifier.startsWith('../')) file = path.resolve(path.dirname(owner), specifier);
  else {
    const match = /^(@[^/]+\/[^/]+|[^/]+)(\/.*)?$/.exec(specifier);
    if (!match || specifier.startsWith('/') || specifier.includes(':')) boundaryError(`forbidden URL/absolute import ${specifier}`);
    const [, name, suffix = ''] = match;
    // Resolve the actual alias link before admission; package names alone are insufficient.
    let link;
    for (let current = path.dirname(owner); within(context.root, current); current = path.dirname(current)) {
      const candidate = path.join(current, 'node_modules', name);
      if (existsSync(candidate)) { link = realpathSync(candidate); break; }
    }
    if (!link || !runtime.includes(link)) boundaryError(`forbidden resolved package ${specifier}: ${link ?? 'missing'}`);
    if (scope === ref && context.graph && (context.graph !== 'container' && context.graph !== 'inventory' || owner !== path.join(ref, expectedPolicy.runtime.dependencyEntry))) boundaryError(`forbidden container dependency edge ${specifier}: ${owner}`);
    if (scope === ref && !(expectedPolicy.runtime.modules[path.relative(ref, owner).split(path.sep).join('/')]?.packages ?? []).includes(specifier)) boundaryError(`unapproved runtime entry ${specifier}`);
    const pkg = packages.get(link);
    const target = pkg.exports === undefined ? (suffix ? '.' + suffix : pkg.main) : mapping(pkg.exports, suffix ? '.' + suffix : '.', conditions);
    if (typeof target !== 'string' || !target.startsWith('./')) boundaryError(`unanalyzable package exports ${specifier}`);
    file = path.resolve(link, target);
    if (!within(link, file)) boundaryError('package export escapes its package');
  }
  return admit(file);
  function admit(target) {
    if (type && !/\.d\.[cm]?ts$/.test(target) && /\.[cm]?ts$/.test(target) && !existsSync(target)) {
      const declaration = target.replace(/\.([cm]?)ts$/, '.d.$1ts');
      if (existsSync(declaration)) target = declaration;
    }
    if (type && /\.[cm]?js$/.test(target)) {
      const declaration = target.replace(/\.([cm]?)js$/, '.d.$1ts');
      if (existsSync(declaration)) target = declaration;
    }
    if (!existsSync(target)) boundaryError(`unresolved file import ${specifier}: ${target}`);
    const real = realpathSync(target);
    if (!lstatSync(real).isFile()) boundaryError(`unresolved file import ${specifier}: ${real}`);
    if (!(within(ref, real) && !path.relative(ref, real).split(path.sep).includes('node_modules')) && !runtime.some(dir => within(dir, real))) boundaryError(`forbidden resolved path ${specifier}: ${real}`);
    if (context.graph && scope === ref && runtime.some(dir => within(dir, real)) && (!['container', 'inventory'].includes(context.graph) || owner !== path.join(ref, expectedPolicy.runtime.dependencyEntry))) boundaryError(`forbidden container dependency path: ${real}`);
    if (context.graph && !['container', 'inventory'].includes(context.graph) && within(path.join(ref, 'container'), real)) boundaryError(`forbidden container module in ${context.graph} graph: ${real}`);
    if (!/\.(?:[cm]?js|[cm]?ts)$/.test(real)) boundaryError(`executable extension not permitted: ${real}`);
    return real;
  }
}
