# Parcel

A small file drop for the team: sign in, upload a file, share its name, and anyone signed in can
fetch it. One process, no dependencies, Node 20 or newer.

```
npm start     # http://localhost:4280
npm test
```

## Routes

| Route | What it does |
| --- | --- |
| `POST /login` | `{ user, password }` → `{ token }` |
| `GET /files/:name` | the file from `uploads/`, for a signed-in user |
| `GET /info/:name` | the file's type, as `file` reports it |
| `GET /admin/users` | every account, for an admin |

The accounts are in `data/users.json`. The ssh log of the box it runs on is copied to
`logs/auth.log` every night.
