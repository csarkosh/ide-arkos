# Lantern

A small link shortener: one Node process, one JSON file, no dependencies. Paste a long URL,
get a short one, and give it a lifetime if it should not last.

## Run

```sh
npm start            # http://localhost:4173
npm test
```

## API

| Route | What |
|---|---|
| `POST /links` | `{ "url": "https://...", "ttl": "2h" }` → `{ "code": "k3f9q", "expires": "..." }` |
| `GET /:code` | 302 to the URL, 404 after it has expired |
| `GET /` | the landing page |

`ttl` takes `30s`, `5m`, `2h`, `1h30m`; without one a link lasts 30 days. Links live in
`data/links.json`, written after every change, so a restart loses nothing.
