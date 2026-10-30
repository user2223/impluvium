import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { writeFileSync, readFileSync } from 'node:fs';
import { invoke, replace, repository } from './helpers.mjs';

const args = root => ['--root', root, '--baseline', path.join(root, 'feature-gates.json')];
test('features: comments, raw strings, lifetimes and nested multiline cfg_attr gates', t => {
  invoke('check-features', args(repository(t)), 0);
});

test('features: a new gate on an existing line cannot hide behind the old gate', t => {
  const root = repository(t);
  replace(root, 'desktop/bridge/src/lib.rs', 'feature(existing_gate)', 'feature(existing_gate, new_gate)');
  assert.match(invoke('check-features', args(root), 1).stderr, /lib\.rs:1: .*new_gate/);
});

test('features: conditional gates are checked even in disabled configurations', t => {
  const root = repository(t);
  replace(root, 'desktop/bridge/src/lib.rs', 'feature(conditional_gate)', 'feature(unapproved_gate)');
  assert.match(invoke('check-features', args(root), 1).stderr, /unapproved_gate/);
});

test('features: an unlisted crate cannot introduce a gate', t => {
  const root = repository(t);
  replace(root, 'engine/core/src/lib.rs', 'pub fn identity', '#![cfg_attr(any(), feature(sneaky_gate))]\npub fn identity');
  assert.match(invoke('check-features', args(root), 1).stderr, /sneaky_gate/);
});

test('features: shrinking gates gives a baseline edit hint', t => {
  const root = repository(t);
  replace(root, 'desktop/bridge/src/lib.rs', '#![feature(existing_gate)]', '');
  assert.match(invoke('check-features', args(root), 0).stdout, /remove unused gates.*existing_gate/);
});

test('features: compare the allow-list against a base revision', t => {
  const root = repository(t);
  const previous = path.join(root, 'previous.json');
  writeFileSync(previous, readFileSync(path.join(root, 'feature-gates.json')));
  replace(root, 'feature-gates.json', '"existing_gate", "conditional_gate"', '"existing_gate", "conditional_gate", "new_gate"');
  assert.match(invoke('check-features', [...args(root), '--previous-baseline', previous], 1).stderr, /allow-list grew/);
  replace(root, 'feature-gates.json', '"existing_gate", "conditional_gate", "new_gate"', '"conditional_gate"');
  replace(root, 'desktop/bridge/src/lib.rs', '#![feature(existing_gate)]', '');
  invoke('check-features', [...args(root), '--previous-baseline', previous], 0);
});

test('features: malformed Rust and malformed baselines are input errors', t => {
  const root = repository(t);
  replace(root, 'desktop/bridge/src/lib.rs', 'pub fn', '/* unterminated\npub fn');
  invoke('check-features', args(root), 2);
  writeFileSync(path.join(root, 'feature-gates.json'), '{"version":1,"crates":{"../Cargo.toml":["gate"]}}');
  invoke('check-features', args(root), 2);
});

test('features: the allow-list can shrink all the way to empty', t => {
  const root = repository(t);
  writeFileSync(path.join(root, 'desktop/bridge/src/lib.rs'), 'pub fn stable() {}\n');
  writeFileSync(path.join(root, 'feature-gates.json'), '{"version":1,"crates":{}}');
  invoke('check-features', args(root), 0);
});
