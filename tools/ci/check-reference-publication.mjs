import path from 'node:path';
import { lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { cli, InputError, json, plainObject, required, stringOption } from './common.mjs';
import { referencePath } from './reference-boundary.mjs';

// The caller supplies the admission vocabulary outside the public source tree.
// Neither this tool nor its diagnostics disclose that vocabulary.
export function publicationPolicy(policy) {
  if (!plainObject(policy) || policy.version !== 1 ||
      Object.keys(policy).sort().join(',') !== 'terms,version' ||
      !Array.isArray(policy.terms) || !policy.terms.length ||
      policy.terms.some(term => typeof term !== 'string' || term.trim() !== term || term.length < 2 || term.length > 256) ||
      new Set(policy.terms.map(term => term.toLowerCase())).size !== policy.terms.length) {
    throw new InputError('<publication-policy>:1: expected version 1 and nonempty unique literal terms');
  }
  return policy.terms.map(term => new RegExp('(?:^|[^a-z0-9])' + term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?=$|[^a-z0-9])', 'i'));
}

export function referencePublicationIssues(root, policy) {
  const patterns = publicationPolicy(policy), issues = [], decoder = new TextDecoder('utf-8', { fatal: true });
  const inspect = (text, file, location) => {
    for (const [index, pattern] of patterns.entries()) {
      if (pattern.test(text)) issues.push(`${file}:${location}: publication policy term ${index + 1}`);
    }
  };
  const visit = relative => {
    const directory = path.join(root, relative), info = lstatSync(directory);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new InputError(`${relative}:1: publication root must be a regular directory`);
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const file = relative + '/' + entry.name;
      inspect(entry.name, file, 1);
      if (entry.isSymbolicLink()) throw new InputError(`${file}:1: publication input is a symlink`);
      // This is the same exact installation output as the Source exception.
      if (file === referencePath + '/node_modules' && entry.isDirectory()) continue;
      if (entry.isDirectory()) visit(file);
      else if (entry.isFile()) {
        let text;
        try { text = decoder.decode(readFileSync(path.join(root, file))); }
        catch { throw new InputError(`${file}:1: publication input is not UTF-8 text`); }
        for (const [line, value] of text.split('\n').entries()) inspect(value, file, line + 1);
        if (file.endsWith('.json')) {
          let data;
          try { data = JSON.parse(text); }
          catch { throw new InputError(`${file}:1: publication JSON is not analyzable`); }
          const strings = (value, pointer = '') => {
            if (typeof value === 'string') inspect(value, file, '1 (' + pointer + ')');
            else if (value && typeof value === 'object') for (const [key, item] of Object.entries(value)) {
              inspect(key, file, '1 (' + pointer + ')');
              strings(item, pointer + '/' + key.replaceAll('~', '~0').replaceAll('/', '~1'));
            }
          };
          strings(data);
        }
      } else throw new InputError(`${file}:1: publication input is not a regular file`);
    }
  };
  visit(referencePath);
  visit('engine/protocol/testdata');
  return [...new Set(issues)];
}

export function check(options) {
  required(options, 'root', 'policy');
  const root = realpathSync(path.resolve(options.root)), file = path.resolve(options.policy);
  if (file === root || file.startsWith(root + path.sep)) throw new InputError('<publication-policy>:1: policy must be outside the source tree');
  const info = lstatSync(file), real = realpathSync(file);
  if (!info.isFile() || info.isSymbolicLink() || real === root || real.startsWith(root + path.sep)) throw new InputError('<publication-policy>:1: policy must be a regular file outside the resolved source tree');
  return referencePublicationIssues(root, json(real));
}

await cli(import.meta.url, 'Usage: node tools/ci/check-reference-publication.mjs --root DIR --policy FILE\nCheck visible source text and decoded JSON strings against an externally supplied publication policy. No word-list exemptions. Policy and diagnostics belong to the caller admission process, not the reference package.', {
  root: stringOption, policy: stringOption,
}, check);
