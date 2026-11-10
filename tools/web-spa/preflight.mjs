// This dependency-free check runs before importing Nitro or starting services.
import { createHash } from 'node:crypto';
import { readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const outputRoot = fileURLToPath(new URL('../public/', import.meta.url));
const outputManifest = JSON.parse(await readFile(outputRoot + 'csp-hashes.json', 'utf8'));
if (JSON.stringify(Object.keys(outputManifest).sort()) !== '["files","scripts","version"]' ||
    outputManifest.version !== 1 || !outputManifest.files || !outputManifest.scripts ||
    !outputManifest.files['index.html'] || !outputManifest.files['loading.css'] ||
    !Array.isArray(outputManifest.scripts['index.html']) ||
    Object.values(outputManifest.scripts).some((hashes) => !Array.isArray(hashes) ||
      hashes.some((hash) => typeof hash !== 'string' || !/^'sha256-[A-Za-z0-9+/]{43}='$/.test(hash)))) {
  throw new Error('Missing or invalid static output manifest');
}
const actualFiles = [];
// Keep the executable-script policy aligned with spa-contract.ts. Data blocks
// are covered by the HTML file digest, never granted script execution hashes.
function inlineScriptNeedsHash(type, language) {
  const trim = (value) => value.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, '');
  const value = type === undefined && language ? trim(`text/${language}`) : trim(type ?? '');
  if (value === '' || /^(?:module|importmap)$/i.test(value)) return true;
  return /^(?:application\/(?:x-)?(?:java|ecma)script|text\/(?:x-)?(?:java|ecma)script|text\/javascript1\.[0-5]|text\/(?:jscript|livescript))$/i.test(value);
}
function scriptAttributes(source) {
  const attributes = new Map();
  for (const match of source.matchAll(/([^\s=/'">]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g)) {
    const name = match[1].toLowerCase();
    const value = (match[2] ?? match[3] ?? match[4] ?? '').replace(
      /&#(?:x([0-9a-f]+)|([0-9]+));?|&(Tab|NewLine|sol|period);/gi,
      (entity, hex, decimal, named) => {
        if (named) return ({ Tab: '\t', NewLine: '\n', sol: '/', period: '.' })[named] ?? entity;
        const code = Number.parseInt(hex ?? decimal, hex ? 16 : 10);
        return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : '\ufffd';
      },
    );
    if (!attributes.has(name)) attributes.set(name, value);
  }
  return attributes;
}
async function verifyDirectory(prefix = '') {
  for (const entry of await readdir(outputRoot + prefix, { withFileTypes: true })) {
    const name = prefix + entry.name;
    if (name === 'csp-hashes.json') continue;
    if (entry.isDirectory()) await verifyDirectory(name + '/');
    else if (entry.isFile()) {
      actualFiles.push(name);
      const bytes = await readFile(outputRoot + name);
      const expected = outputManifest.files[name];
      if (!expected || JSON.stringify(Object.keys(expected).sort()) !== '["sha256","size"]' ||
          expected.size !== bytes.length || expected.sha256 !== createHash('sha256').update(bytes).digest('base64')) {
        throw new Error('Static output does not match its manifest');
      }
      if (name.endsWith('.html')) {
        // The export is parsed with an HTML parser at build time and again by
        // Nitro's validator. This independent pre-import check fails closed if
        // executable tags or their hashes differ from that build inventory.
        const hashes = new Set();
        const html = bytes.toString('utf8').replace(/\r\n?/g, '\n');
        for (const match of html.matchAll(/<script\b((?:"[^"]*"|'[^']*'|[^'">])*)>([\s\S]*?)<\/script\s*>/gi)) {
          const attributes = scriptAttributes(match[1]);
          if (!attributes.has('src') && inlineScriptNeedsHash(attributes.get('type'), attributes.get('language'))) {
            hashes.add("'sha256-" + createHash('sha256').update(match[2]).digest('base64') + "'");
          }
        }
        if (JSON.stringify([...hashes].sort()) !== JSON.stringify(outputManifest.scripts[name])) {
          throw new Error('Static HTML script hashes do not match the manifest');
        }
      }
    } else throw new Error('Invalid static output entry');
  }
}
await verifyDirectory();
if (JSON.stringify(actualFiles.sort()) !== JSON.stringify(Object.keys(outputManifest.files).sort())) {
  throw new Error('Static output has missing files');
}
if (JSON.stringify(actualFiles.filter((file) => file.endsWith('.html')).sort()) !==
    JSON.stringify(Object.keys(outputManifest.scripts).sort())) {
  throw new Error('Static output has an invalid HTML inventory');
}
