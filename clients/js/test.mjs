// Run: node --test clients/js/test.mjs   (needs target/release/lunaris)
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, rmSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { Engine, LunarisError } from './lunaris.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
let eng;
before(async () => { eng = await Engine.start({ root: repo }); });
after(async () => { await eng.close(); });

test('every advertised tool is a method', () => {
  assert.ok(Object.keys(eng.tools).length >= 36);
  for (const n of ['spawn', 'query', 'define_system', 'render', 'step']) assert.equal(typeof eng[n], 'function', n);
});

test('spawn and query', async () => {
  await eng.reset();
  await eng.define_component({ name: 'Health', fields: [{ name: 'hp', type: 'f32', default: 100 }] });
  await eng.spawn({ count: 3, components: { Transform: {}, Health: {} } });
  const q = await eng.query({ with: ['Health'], select: ['Health'] });
  assert.equal(q.total, 3);
  assert.equal(q.entities[0].components.Health.hp, 100);
});

test('errors carry code, path and hint', async () => {
  await eng.reset();
  await assert.rejects(eng.spawn({ components: { Transfrom: {} } }), (e) => e instanceof LunarisError && e.code === 'unknown_component' && /Transform/.test(e.hint));
});

test('same seed, same hash', async () => {
  const hashes = [];
  for (let i = 0; i < 2; i++) {
    await eng.reset({ seed: 5 });
    await eng.spawn({ count: 50, components: { Transform: {}, Velocity: { lin: [1, 0, 0] }, Wander: { strength: 2 } } });
    hashes.push((await eng.step({ ticks: 120 })).hash);
  }
  assert.equal(hashes[0], hashes[1]);
});

test('render returns a PNG buffer', async () => {
  await eng.reset();
  await eng.load_scene({ path: 'examples/showcase.lunaris.json' });
  const frame = await eng.render({ width: 64, height: 36 });
  assert.ok(Buffer.isBuffer(frame.png) && frame.png.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])));
  assert.ok(frame.visible.length > 0);
});
