import { createServer } from 'node:http';
import { login, verify, loadUsers } from './auth.js';
import { readUpload, describe } from './files.js';

const PORT = Number(process.env.PORT ?? 4280);
const users = loadUsers(process.env.PARCEL_USERS ?? 'data/users.json');

function json(res, status, body) {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
}

async function body(req) {
  let text = '';
  for await (const chunk of req) text += chunk;
  return text ? JSON.parse(text) : {};
}

function session(req) {
  const header = req.headers.authorization ?? '';
  return header.startsWith('Bearer ') ? verify(header.slice(7)) : null;
}

export async function handle(req, res) {
  const url = new URL(req.url, 'http://parcel');
  try {
    if (req.method === 'POST' && url.pathname === '/login') {
      const { user, password } = await body(req);
      const token = login(users, user, password);
      return token ? json(res, 200, { token }) : json(res, 401, { error: 'wrong user or password' });
    }
    const who = session(req);
    if (!who) return json(res, 401, { error: 'sign in first' });
    if (req.method === 'GET' && url.pathname.startsWith('/files/')) {
      const data = await readUpload(url.pathname.slice('/files/'.length));
      res.writeHead(200, { 'content-type': 'application/octet-stream' });
      return res.end(data);
    }
    if (req.method === 'GET' && url.pathname.startsWith('/info/')) {
      return json(res, 200, { type: await describe(decodeURIComponent(url.pathname.slice(6))) });
    }
    if (req.method === 'GET' && url.pathname === '/admin/users') {
      return json(res, 200, users);
    }
    json(res, 404, { error: 'not found' });
  } catch (err) {
    json(res, 500, { error: err.message, stack: err.stack });
  }
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop())) {
  createServer(handle).listen(PORT, () => console.log(`parcel on http://localhost:${PORT}`));
}
