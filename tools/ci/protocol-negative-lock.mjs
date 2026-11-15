import { parseToml } from './toml.mjs';
import { fail } from './protocol-contract.mjs';

const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
  ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;

// Cargo may change dependency edges for a feature counterexample, but it may
// not introduce a package or change any other part of the reviewed lock data.
export function verifyNegativeLock(original, derived) {
  const invariant = (bytes, label) => {
    const { data } = parseToml(bytes.toString('utf8'), label);
    if (!Array.isArray(data.package) || !data.package.length) fail('negative lock needs nonempty package tables');
    const identities = new Set();
    const packages = data.package.map(pkg => {
      if (!pkg || typeof pkg !== 'object' || typeof pkg.name !== 'string' || !pkg.name ||
          typeof pkg.version !== 'string' || !pkg.version ||
          (pkg.source !== undefined && typeof pkg.source !== 'string') ||
          (pkg.checksum !== undefined && typeof pkg.checksum !== 'string') ||
          (pkg.dependencies !== undefined && (!Array.isArray(pkg.dependencies) || pkg.dependencies.some(dep => typeof dep !== 'string')))) fail('invalid negative lock package');
      const identity = JSON.stringify([pkg.name, pkg.version, pkg.source ?? null, pkg.checksum ?? null]);
      if (identities.has(identity)) fail('duplicate negative lock package');
      identities.add(identity);
      const { dependencies, ...rest } = pkg;
      return JSON.stringify(canonical(rest));
    }).sort();
    return JSON.stringify(canonical({ ...data, package: packages }));
  };
  if (invariant(original, '<product-lock>') !== invariant(derived, '<negative-lock>')) {
    fail('negative lock changes the package set or non-dependency data; a reviewed prepared lock is required for this change');
  }
}
