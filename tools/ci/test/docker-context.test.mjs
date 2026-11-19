import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { toml } from '../toml.mjs';

const root = new URL('../../../', import.meta.url);

// The server image builds one package, but Cargo loads the manifest of every
// workspace member. Members under engine/ and server/crates/ need their
// manifests and target roots, even though the image builds only torrential.
test('the image build context contains every engine and server workspace member', () => {
  const members = toml(new URL('Cargo.toml', root).pathname).data.workspace.members;
  const allowed = new Set(readFileSync(new URL('.dockerignore', root), 'utf8').split('\n'));
  const selected = members.filter(member => member.startsWith('engine/') || member.startsWith('server/crates/'));
  assert.ok(selected.length >= 9);
  for (const member of selected) {
    assert.ok(allowed.has(`!${member}/Cargo.toml`), `.dockerignore lacks !${member}/Cargo.toml`);
    assert.ok(allowed.has(`!${member}/src/`), `.dockerignore lacks !${member}/src/`);
    assert.ok(readFileSync(new URL(`${member}/Cargo.toml`, root), 'utf8'));
    assert.ok(['lib.rs', 'main.rs'].some(file => existsSync(new URL(`${member}/src/${file}`, root))), `${member}: missing target root`);
  }
  for (const exclusion of ['**/target', '**/node_modules', '**/.env*']) assert.ok(allowed.has(exclusion));
});
