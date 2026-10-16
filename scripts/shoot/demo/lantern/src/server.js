import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { Store } from './store.js';
import { parseDuration } from './time.js';

const PORT = Number(process.env.PORT ?? 4173);
const store = new Store(process.env.LANTERN_FILE ?? 'data/links.json');

function json(res, status, body) {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
}

async function body(req) {
  let text = '';
  for await (const chunk of req) text += chunk;
  return text ? JSON.parse(text) : {};
}

export async function handle(req, res) {
  const url = new URL(req.url, 'http://lantern');
  if (req.method === 'GET' && url.pathname === '/') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    return res.end(await readFile(new URL('../public/index.html', import.meta.url)));
  }
  if (req.method === 'POST' && url.pathname === '/links') {
    const input = await body(req);
    let target;
    try {
      target = new URL(input.url);
    } catch {
      return json(res, 400, { error: 'url must be a full URL' });
    }
    const ttl = input.ttl === undefined ? undefined : parseDuration(input.ttl);
    if (ttl === null) return json(res, 400, { error: 'ttl must look like 30s, 5m, 2h or 1h30m' });
    const link = store.create(target.href, ttl);
    return json(res, 201, { code: link.code, expires: new Date(link.expires).toISOString() });
  }
  if (req.method === 'GET') {
    const link = store.resolve(url.pathname.slice(1));
    if (link) {
      res.writeHead(302, { location: link.url });
      return res.end();
    }
  }
  json(res, 404, { error: 'no such link' });
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop())) {
  setInterval(() => store.purge(), 60_000).unref();
  createServer(handle).listen(PORT, () => console.log(`lantern on http://localhost:${PORT}`));
}
