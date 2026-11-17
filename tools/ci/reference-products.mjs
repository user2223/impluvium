import path from 'node:path';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { yamlData } from './pnpm-licences.mjs';
import { referencePath, within } from './reference-boundary.mjs';
import { cli, stringOption } from './common.mjs';

export const referenceContextExclusions = ['engine/protocol/conformance/reference/', 'engine/protocol/testdata/'];
// Source admission complements the existing built-image and product-graph checks.
// It does not inspect a built client or replace the frozen install/build gates.
export function referenceProductIssues(root) {
  root = path.resolve(root);
  const issues = [], ref = path.join(root, referencePath);
  const manifest = JSON.parse(readFileSync(path.join(ref, 'package.json'))), name = manifest.name;
  const denied = text => typeof text === 'string' && (text.includes(name) || /conformance[\\/]reference(?:[\\/'"\s]|$)/.test(text));
  const rules = readFileSync(path.join(root, '.dockerignore'), 'utf8').split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#'));
  if (JSON.stringify(rules.slice(-2)) !== JSON.stringify(referenceContextExclusions)) issues.push('reference runtime boundary: final build-context exclusions differ');
  const visitValues = (value, file) => {
    if (typeof value === 'string' && denied(value)) issues.push(`reference runtime boundary: forbidden reference dependency or path in ${file}`);
    if (Array.isArray(value)) for (const item of value) visitValues(item, file);
    else if (value && typeof value === 'object') for (const [key, item] of Object.entries(value)) { if (denied(key)) issues.push(`reference runtime boundary: forbidden reference dependency in ${file}`); visitValues(item, file); }
  };
  const workspace = yamlData(path.join(root, 'pnpm-workspace.yaml'));
  for (const directory of ['.', ...(workspace.packages ?? [])]) {
    if (typeof directory !== 'string' || /[*?{}!]/.test(directory)) { issues.push('reference runtime boundary: unresolved Node workspace member'); continue; }
    const file = path.resolve(root, directory, 'package.json');
    if (!within(root, file)) { issues.push('reference runtime boundary: workspace member escapes source'); continue; }
    if (file === path.join(ref, 'package.json')) continue;
    const pkg = JSON.parse(readFileSync(file));
    for (const key of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies', 'imports', 'exports']) {
      visitValues(pkg[key], path.relative(root, file));
      for (const value of Object.values(pkg[key] ?? {})) if (typeof value === 'string' && /^(?:link|file):/.test(value) && within(ref, path.resolve(path.dirname(file), value.slice(value.indexOf(':') + 1)))) issues.push(`reference runtime boundary: linked reference dependency in ${path.relative(root, file)}`);
    }
  }
  const lock = yamlData(path.join(root, 'pnpm-lock.yaml'));
  for (const [directory, importer] of Object.entries(lock.importers ?? {})) if (directory !== referencePath) visitValues(importer, `pnpm-lock.yaml:${directory}`);
  visitValues(lock.snapshots, 'pnpm-lock.yaml:snapshots');
  const seen = new Set(), inputs = ['server', 'desktop', 'libraries/base'];
  const walk = file => {
    const info = lstatSync(file), real = realpathSync(file);
    if (within(ref, real)) { issues.push(`reference runtime boundary: reference source linked into ${path.relative(root, file)}`); return; }
    if (info.isSymbolicLink()) { if (within(root, real)) walk(real); else issues.push('reference runtime boundary: product input link escapes source'); return; }
    if (seen.has(real)) return; seen.add(real);
    if (info.isDirectory()) {
      for (const entry of readdirSync(file)) if (!['node_modules', 'target', 'dist', '.git', '.nuxt', '.output'].includes(entry)) walk(path.join(file, entry));
    } else if (/\.(?:[cm]?[jt]sx?|vue|json|toml)$/.test(file)) {
      if (denied(readFileSync(file, 'utf8'))) issues.push(`reference runtime boundary: reference source or dependency in ${path.relative(root, file)}`);
    }
  };
  for (const directory of inputs) if (existsSync(path.join(root, directory))) walk(path.join(root, directory));
  const configFile = path.join(root, 'desktop/src-tauri/tauri.conf.json');
  const config = JSON.parse(readFileSync(configFile)), client = path.join(root, 'desktop');
  const frontend = config.build?.frontendDist;
  if (typeof frontend !== 'string' || !within(client, path.resolve(path.dirname(configFile), frontend)) || path.resolve(path.dirname(configFile), frontend) === client) issues.push('reference runtime boundary: client frontend source escapes its output tree');
  const resources = config.bundle?.resources ?? [], sources = Array.isArray(resources) ? resources : Object.keys(resources);
  for (const source of [...sources, ...(config.bundle?.externalBin ?? [])]) {
    const prefix = typeof source === 'string' ? source.split(/[*?{[]/, 1)[0] : '';
    if (!prefix || !within(client, path.resolve(path.dirname(configFile), prefix))) issues.push('reference runtime boundary: client resource escapes its source tree');
  }
  return [...new Set(issues)];
}
await cli(import.meta.url, 'Usage: node tools/ci/reference-products.mjs --root DIR\nReject reference-package paths and dependencies in runtime product inputs and build context.', { root: stringOption },
  options => referenceProductIssues(path.resolve(options.root ?? '.')));
