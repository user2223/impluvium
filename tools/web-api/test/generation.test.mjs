import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { compareOutputs, generate } from '../generate.mjs';
import { problemCollector } from '../compiler.mjs';

test('generation collects independent route and field failures before rejecting the result', () => {
  const problems = problemCollector();
  const visited = [];
  problems.capture('first route Body', () => {
    visited.push('body');
    throw new AggregateError([new Error('invalid default'), new Error('unresolved field')]);
  });
  problems.capture('first route Query', () => { visited.push('query'); throw new Error('invalid query'); });
  problems.capture('second route response', () => { visited.push('response'); throw new Error('invalid response'); });
  assert.deepEqual(visited, ['body', 'query', 'response']);
  assert.throws(() => problems.throwIfAny('Generation failed'), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors.length, 4);
    for (const context of ['first route Body', 'first route Query', 'second route response']) assert.ok(error.message.includes(context));
    return true;
  });
});

test('static contract generation is reproducible and detects changed, missing and extra files', async () => {
  const first = await generate();
  const second = await generate();
  assert.deepEqual([...first], [...second]);
  assert.deepEqual([...first.keys()].sort(), ['dtos.ts', 'inventory.json', 'routes.ts']);
  const directory = await mkdtemp(join(tmpdir(), 'http-contracts-'));
  try {
    for (const [name, value] of first) await writeFile(join(directory, name), value);
    await compareOutputs(directory, first);
    await writeFile(join(directory, 'extra.ts'), 'export {};\n');
    await assert.rejects(compareOutputs(directory, first), /file set differs/);
    await rm(join(directory, 'extra.ts'));
    await writeFile(join(directory, 'dtos.ts'), '// changed\n');
    await assert.rejects(compareOutputs(directory, first), /file drift/);
    await rm(join(directory, 'dtos.ts'));
    await assert.rejects(compareOutputs(directory, first), /file set differs/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
