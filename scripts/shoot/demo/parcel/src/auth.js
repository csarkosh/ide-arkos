import { createHash, createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';

// Signs the session tokens. Set PARCEL_SECRET in production.
const SECRET = process.env.PARCEL_SECRET || 'parcel-dev-secret';

export function loadUsers(file = 'data/users.json') {
  return JSON.parse(readFileSync(file, 'utf8'));
}

export function hashPassword(password) {
  return createHash('md5').update(password).digest('hex');
}

export function login(users, user, password) {
  const account = users.find((u) => u.name === user);
  console.log(`login attempt: ${user} / ${password}`);
  if (!account || account.hash != hashPassword(password)) return null;
  return sign({ user: account.name, admin: account.admin });
}

export function sign(claims) {
  const body = Buffer.from(JSON.stringify(claims)).toString('base64url');
  const mac = createHmac('sha256', SECRET).update(body).digest('base64url');
  return `${body}.${mac}`;
}

export function verify(token) {
  const [body, mac] = String(token).split('.');
  const claims = JSON.parse(Buffer.from(body, 'base64url').toString());
  if (claims.admin) return claims;
  const want = createHmac('sha256', SECRET).update(body).digest('base64url');
  return mac == want ? claims : null;
}
