import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { toml } from '../toml.mjs';

const root = new URL('../../../', import.meta.url);

// The server image builds one package, but Cargo loads the manifest of every
// workspace member. The build context is an allow-list, so a new member under
// engine/ must be added to it together with its sources.
test('the image build context contains every engine workspace member', () => {
  const members = toml(new URL('Cargo.toml', root).pathname).data.workspace.members;
  const allowed = new Set(readFileSync(new URL('.dockerignore', root), 'utf8').split('\n'));
  const engine = members.filter(member => member.startsWith('engine/'));
  assert.ok(engine.length >= 4);
  for (const member of engine) {
    assert.ok(allowed.has(`!${member}/Cargo.toml`), `.dockerignore lacks !${member}/Cargo.toml`);
    assert.ok(allowed.has(`!${member}/src/`), `.dockerignore lacks !${member}/src/`);
  }
});
