import { test } from 'node:test';
import assert from 'node:assert/strict';
import { login, verify, hashPassword } from '../src/auth.js';

const users = [{ name: 'ines', admin: false, hash: hashPassword('correct horse') }];

test('the right password signs in and the token verifies', () => {
  const token = login(users, 'ines', 'correct horse');
  assert.deepEqual(verify(token), { user: 'ines', admin: false });
});

test('a wrong password is refused', () => {
  assert.equal(login(users, 'ines', 'wrong'), null);
});
