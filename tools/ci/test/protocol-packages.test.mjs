import test from 'node:test';
import assert from 'node:assert/strict';
import { parsePackages } from '../protocol-evidence.mjs';
import { diagnosticLog, jsonRecords } from '../protocol-diagnostics.mjs';

function fixture() {
  const packages = [
    ['root', 'impluvium-protocol', '0.1.0'], ['buffa', 'buffa', '0.9.2'],
    ['cfg', 'cfg-if', '1.0.5'], ['macro', 'serde_derive', '1.0.229'],
    ['serde', 'serde', '1.0.229'], ['connect', 'connectrpc', '0.9.1'],
    ['jobserver', 'jobserver', '0.1.35'], ['libc', 'libc', '0.2.191'],
  ].map(([id, name, version]) => ({ id, name, version,
    targets: [{ kind: [id === 'macro' ? 'proc-macro' : 'lib'] }] }));
  return { metadata: { packages }, invocation: { rootIds: ['root'], run: 'build' } };
}

// Source: Cargo 1.99.1 output for the protocol core, with
// `tree --no-dedupe --prefix depth --edges normal,build --format '{p}|{f}'`.
// Only the numeric depth prefixes and the export-specific root path are removed.
const core = 'impluvium-protocol v0.1.0|\ncfg-if v1.0.5|\nbuffa v0.9.2|json,std,text\nserde v1.0.229|alloc,derive,serde_derive,std\nserde_derive v1.0.229 (proc-macro)|default\ncfg-if v1.0.5|\n';

test('real core package rows retain resolved features, proc macros and duplicate identities', () => {
  const { metadata, invocation } = fixture(), graph = parsePackages(core, metadata, invocation);
  assert.equal(graph.version, 2);
  assert.equal(graph.nodes.length, 5);
  assert.deepEqual(graph.nodes.find(node => node.id === 'buffa').features, ['json', 'std', 'text']);
  assert.equal(graph.nodes.filter(node => node.id === 'cfg').length, 1);
  assert.ok(graph.nodes.some(node => node.id === 'macro'));
});

test('optional-dependency feature pseudonodes are unnecessary for the client package set', () => {
  const { metadata, invocation } = fixture();
  // Source: Cargo 1.99.1 client Windows package output with the same format.
  // Its feature tree also displays connectrpc/hyper, but {f} is client,json.
  const graph = parsePackages(core + 'connectrpc v0.9.1|client,json\n', metadata, invocation);
  assert.deepEqual(graph.nodes.find(node => node.id === 'connect').features, ['client', 'json']);
});

test('Windows capture includes the Linux-host jobserver/libc package pair without interpreting filtered edges', () => {
  const { metadata, invocation } = fixture();
  // Source: Cargo 1.99.1 client x86_64-pc-windows-gnullvm package output:
  // jobserver v0.1.35| followed by libc v0.2.191|default,std on the Linux host.
  metadata.resolve = { nodes: [{ id: 'jobserver', deps: [{ name: 'getrandom', pkg: 'other-target' }] }] };
  const graph = parsePackages(core + 'jobserver v0.1.35|\nlibc v0.2.191|default,std\n', metadata, invocation);
  assert.ok(graph.nodes.some(node => node.id === 'jobserver'));
  assert.deepEqual(graph.nodes.find(node => node.id === 'libc').features, ['default', 'std']);
});

test('two feature sets for one package remain distinct, with no feature union', () => {
  const { metadata, invocation } = fixture();
  // Source: serde rows from the core and client Cargo 1.99.1 package outputs.
  // Combining them here is a synthetic same-ID, different-feature-set case.
  const graph = parsePackages(core + 'serde v1.0.229|alloc,default,derive,serde_derive,std (*)\n', metadata, invocation);
  const rows = graph.nodes.filter(node => node.id === 'serde');
  assert.equal(rows.length, 2);
  assert.equal(rows.filter(node => node.features.includes('default')).length, 1);
});

test('flat normalization consumes one-pass records and keeps a late package', () => {
  const { metadata, invocation } = fixture();
  function* records() {
    yield* core.trimEnd().split('\n');
    for (let index = 0; index < 10000; index++) yield 'cfg-if v1.0.5|';
    yield 'jobserver v0.1.35|';
  }
  assert.deepEqual(parsePackages(records(), metadata, invocation), parsePackages(core + 'jobserver v0.1.35|', metadata, invocation));
});

test('all distinct malformed rows retain raw text and source line numbers', () => {
  const { metadata, invocation } = fixture(), report = diagnosticLog();
  const raw = ['impluvium-protocol v0.1.0|', 'not a package record', 'unknown v1.0.0|', 'cfg-if v1.0.5|bad feature', 'not a package record'].join('\n');
  parsePackages(raw, metadata, invocation, report);
  assert.equal(report.errors.length, 3);
  assert.deepEqual(report.errors[0].lines, [2, 5]);
  assert.equal(report.errors[0].raw, 'not a package record');
  assert.equal(report.errors[1].line, 3);
  assert.equal(report.errors[2].line, 4);
  assert.throws(() => report.finish(), /packages.tree:2,5:/);
});

test('missing roots and malformed independent JSON records cannot establish a complete capture', () => {
  const { metadata, invocation } = fixture(), report = diagnosticLog();
  parsePackages('cfg-if v1.0.5|', metadata, invocation, report);
  const records = jsonRecords('not json\n{"status":0}\nnull\n', 'compiler.jsonl', report);
  assert.deepEqual(records, [{ status: 0 }]);
  assert.equal(report.errors.length, 3);
  assert.throws(() => report.finish(), /selected root missing/);
});
