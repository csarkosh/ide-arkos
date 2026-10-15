import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Store } from '../src/store.js';

function clock(start = 1_000_000) {
  let t = start;
  return { now: () => t, tick: (ms) => (t += ms) };
}

test('a link resolves until it expires', () => {
  const c = clock();
  const store = new Store(null, { now: c.now });
  const { code } = store.create('https://example.com/docs', 60_000);
  assert.equal(store.resolve(code).url, 'https://example.com/docs');
  c.tick(60_000);
  assert.equal(store.resolve(code), null);
});

test('codes are five characters from the alphabet, never repeated', () => {
  // A "random" that repeats itself once: the second code must still differ from the first.
  const draws = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0.5, 0.5, 0.5, 0.5, 0.5];
  const store = new Store(null, { random: () => draws.shift() ?? Math.random() });
  const first = store.create('https://a.example').code;
  const second = store.create('https://b.example').code;
  assert.match(first, /^[a-z2-9]{5}$/);
  assert.notEqual(first, second);
});

test('purge drops expired links and reports how many', () => {
  const c = clock();
  const store = new Store(null, { now: c.now });
  store.create('https://a.example', 1_000);
  store.create('https://b.example', 5_000);
  c.tick(2_000);
  assert.equal(store.purge(), 1);
  assert.equal(store.links.size, 1);
});
