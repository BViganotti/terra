# The chronicle

Every world run produces a chronicle: an append-only sequence of events. The
chronicle is the product's memory. It is not a log of debug messages. Each event
records what happened, where, to whom, and — critically — why.

## Event shape

| Field | Type | Meaning |
|---|---|---|
| `id` | dense integer | Equal to the event index. Never reused. |
| `year` | integer | Non-decreasing across the store. |
| `kind` | enum | One of 20 kinds such as `settlement_founded` or `collapse`. |
| `title` | string | Headline, at most 120 characters. |
| `detail` | string | Human-readable description, at most 600 characters. |
| `cell` | optional cell | Local events point at a cell. |
| `civ` | optional civ | Social events point at a civilization. |
| `causes` | list | At least one cause, at most 12. |
| `severity` | `0..1` | Visual weight and narrative importance. |
| `tags` | string list | Searchable extras. |

Kinds are grouped into categories for the timeline palette: nature, disaster,
society, economy, conflict and culture.

## Causes

A cause is one of two things:

1. **State** — a measured fact:
   - `key`: stable identifier such as `elevation`, `precip`, `cohesion`;
   - `value` and `unit`;
   - optional `cell` and `civ`;
   - `label`: human phrasing used by the cause panel.

   Example: `precip = 3394 mm/yr at cell 368`.

2. **Event** — the id of an event that was already recorded. Causes always point
   backwards in time and in id. A cause that would point forward or to a missing
   event is rejected at record time.

Recording also deduplicates repeated event causes. Two rules may cite the same
prior event; the store keeps one citation, preserving a clean tree view.

## Invariants

`Chronicle::validate()` is the single source of truth. It reports:

- dense ids matching positions;
- non-decreasing years;
- at least one cause per event;
- no future or missing event causes;
- no duplicate event causes;
- non-empty titles and details;
- finite severity inside `[0, 1]`.

`Chronicle::is_sound()` is true exactly when validation finds nothing.

The simulation refuses to publish a history whose chronicle is unsound. The
Studio job runner marks the job failed instead of serving fake causality.

## Tracing

The query module provides bounded traversals:

- `cause_chain(id)` — breadth-first walk of every reachable cause. Returns
  event nodes with depth and parent, plus state facts. Bounded to 48 levels and
  5,000 visited nodes.
- `trace_paths(id, n)` — up to `n` explicit paths from the event to state facts.
  Each step is either an event or a measured fact.
- `descendants(id)` — events that cite this event directly or transitively.
- `explain(id)` — a deterministic paragraph: what happened, why, and one level
  of ancestry.

Every event in a Terra run reaches at least one state fact. Root events (the
world formation, for example) cite only state facts; all other chains terminate
there. The acceptance test walks every event, not a sample.

## Queries

```rust
let page = history.chronicle.query()
    .years(1_000, 6_000)
    .kind(EventKind::Expansion)
    .civ(3)
    .text("river")
    .min_severity(0.3)
    .page(0, 50);
```

Filters compose with AND. Text search is case-insensitive over title, detail
and tags. A page is capped at 500 events; the Studio requests 100 at a time.
Query construction borrows the chronicle and never mutates it.

## Exports

The Studio and the library expose three formats:

- **Markdown** — grouped by century, each event with causes. Designed for
  human reading and archiving.
- **JSON** — event array with structured causes, plus counts and soundness.
- **CSV** — one row per event, quoted text, cause descriptions joined with
  `|`. Suitable for spreadsheets and notebooks.

All three are deterministic: exporting the same history twice yields identical
bytes. Text is escaped (JSON control escapes, CSV doubled quotes, Markdown
pipe/newline flattening).

## Example

```json
{
  "id": 42,
  "year": 120,
  "kind": "war_declared",
  "title": "Elound declares war on Joreinis",
  "detail": "Relations collapse to -71. With 904120 soldiers against 1121293, the frontier becomes a battlefield.",
  "cell": 4513,
  "civ": 0,
  "severity": 0.75,
  "causes": [
    {"type": "event", "id": 31},
    {"type": "state", "key": "relation", "value": -71, "unit": ""},
    {"type": "state", "key": "military", "value": 904120, "unit": "soldiers"}
  ]
}
```

The event is not true because it is written down; it is written down because a
documented rule fired on measured state. That distinction is the point of the
chronicle.

## Limits

- 200,000 events per run.
- 12 causes per event.
- 120-character titles, 600-character details.
- Severity is clamped to `[0, 1]`.
- Chain traversal depth and node count are capped, so a hostile history cannot
  freeze the Studio.

These limits are constants in `terra_history::chronicle`, tested directly and
exercised by the malformed-input campaign.
