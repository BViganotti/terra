# Studio HTTP API

The Studio serves a small HTTP/1.1 API on loopback. One request per connection;
bodies are JSON; responses are JSON or PNG. The token is required on every
`/api/*` route.

## Authentication

Supply the token in one of three ways:

```
GET /api/health?t=<token>
GET /api/health  X-Token: <token>
GET /api/health  Authorization: Bearer <token>
```

Missing or wrong tokens return `401` with `WWW-Authenticate: Bearer`.
Comparison is constant-time. Static assets (`/`, `/app.css`, `/app.js`,
`/favicon.svg`) are public so the page can load; they contain no state.

## Limits

| Limit | Value |
|---|---|
| Request headers | 16 KiB |
| Line/header count | 64 headers |
| Request body | 64 KiB |
| URL length | 512 bytes |
| Query parameters | 32 |
| Query value length | 4 KiB |
| Concurrent generations | 2 |
| Cached worlds | 10 |
| SSE stream lifetime | 10 minutes |

## GET /api/health

```json
{"ok":true,"version":"0.1.0","terra":"0.1.0","history":"0.1.0","worlds":1,"running":0,"uptimeSec":42}
```

## POST /api/run

Body fields (all optional):

| Field | Type | Meaning |
|---|---|---|
| `seed` | integer | world seed, clamped to 32 bits |
| `years` | integer | history span, 100–20,000 |
| `subdiv` | integer | icosphere level, 2–5 |
| `civs` | integer | initial civilizations, 2–12 |
| `preset` | string | `temperate`, `arid`, `icehouse`, `archipelago`, `pangaea` |
| `scenario` | string | full scenario text, at most 8 KiB |
| `aggression` | number | multiplier 0.2–3 |
| `expansion` | number | multiplier 0.2–3 |

`scenario` and `preset` are alternatives; explicit numeric fields override
either. Unknown fields are rejected with `400`. Success returns `202`:

```json
{"id":"w1","worldId":"w1"}
```

`429` means two generations are already running. The client should wait.

## GET /api/job?id=

```json
{
  "id": "w1",
  "status": "running|done|failed",
  "stage": "climate",
  "progress": 0.72,
  "error": null,
  "config": {"seed":42,"years":10000,"subdiv":5,"civs":8,"preset":null},
  "world": {"planet":"Seramonionya","hash":"…","cells":10242,"years":10000,"events":464,"generationMs":210},
  "elapsedMs": 2130
}
```

`world` is null until the job completes.

## GET /api/events?id= (SSE)

`Content-Type: text/event-stream`. Frames:

```
event: progress
data: {"id":"w1","status":"running","stage":"history","progress":0.8,"error":null}

event: complete
data: {"id":"w1","progress":1.0}
```

A failed job emits its final progress with `"status":"failed"`, then closes.
Unknown ids emit `event: error`. The server sends one `retry: 1500` hint.

## GET /api/world?id=

Returns the planet summary: id, name, seed, span, subdivision, cell count,
combined hash, stats, stage hashes, history stats, chronicle stats, layers with
legends, civilization summaries and snapshot years.

Large objects are bounded: at most 32 civilizations, 10 layers and 101
snapshots.

## GET /api/map?id=&layer=&year=&w=&h=

Returns `image/png`. Parameters:

| Parameter | Default | Meaning |
|---|---|---|
| `layer` | `atlas` | one of the ten layer codes |
| `w`, `h` | 1024, 512 | clamped to 64–2048 and 32–1024 |
| `year` | final year | ownership snapshot for political/civ overlay |
| `civs` | `0` | `1` draws civilization overlay on any layer |
| `shade` | `1` | `0` disables hillshade |
| `rivers` | `1` | `0` disables river overlay |

The renderer is deterministic; identical parameters produce identical bytes.

## GET /api/chronicle?id=

Filters:

| Parameter | Meaning |
|---|---|
| `from`, `to` | inclusive year range |
| `kind` | event kind code, or `all` |
| `civ` | civilization id |
| `cell` | cell id |
| `q` | case-insensitive text search |
| `offset`, `limit` | pagination; limit capped at 200 |

Response:

```json
{"total":464,"offset":0,"limit":50,"events":[{"id":0,"year":0,"kind":"world_formed","category":"nature","label":"World formed","title":"…","severity":1.0,"civ":null,"cell":null}]}
```

## GET /api/event?id=&event=

Returns the full event, direct causes (state facts and event summaries), a
breadth-first causal chain, explicit paths to state facts, later descendants, a
deterministic explanation and civilization info.

## GET /api/cell?id=&cell= or ?lat=&lon=

Returns physical and social details for the nearest cell, plus ownership at the
requested `year` and the most recent 20 events at that cell. `lat`/`lon` are
validated to `[-90,90]` and `[-180,180]`.

## GET /api/civs?id= and /api/civ?id=&civ=

`civs` returns every civilization with color, territory, population, technology,
cohesion, military, treasury, traits, resources, cities and wars. `civ` returns
one civilization plus up to 200 of its events.

## GET /api/timeline?id=

Returns event density per century, civilization series per snapshot, wars and
the civilization legend. Used by the chronicle chart.

## Exports

| Path | Type |
|---|---|
| `/api/export/chronicle.md` | Markdown attachment |
| `/api/export/chronicle.json` | JSON attachment |
| `/api/export/chronicle.csv` | CSV attachment |
| `/api/export/world.json` | world summary attachment |

All exports require `id`. Chronicle exports are byte-for-byte deterministic.

## Errors

Errors use a consistent shape:

```json
{"ok":false,"error":"missing world id"}
```

Status codes: `400` malformed request or parameter, `401` auth, `404` unknown
route/world/cell/event, `405` method, `413` body too large, `414` URL too long,
`429` too many generations, `431` headers too large, `500` failed job or
internal error, `503` world still generating.
