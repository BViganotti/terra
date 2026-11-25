# Security and safety

Terra is a local desktop workbench. Its threat model is a single user on
loopback, not a public server. Within that model it is deliberately conservative.

## Boundaries

- **Network:** the HTTP listener binds `127.0.0.1` only. There is no outbound
  network access at runtime. The client loads only same-origin assets; the CSP
  forbids remote script, style and images.
- **Dependencies:** zero external crates. The build has no supply chain beyond
  the Rust toolchain. `unsafe` is forbidden at crate level.
- **Files:** the Studio server never reads arbitrary paths. Assets are compiled
  in with `include_str!`. Only the launcher's `--scenario PATH` reads a file, and
  that is an explicit user action.
- **State:** worlds live in memory. There is no database, cookie jar, analytics
  or telemetry.

## Authentication

- A 128-bit token is generated from `/dev/urandom` at launch (with a time/pid
  fallback only if the device is unavailable).
- The token is required on every `/api/*` route. It may be supplied as `?t=`,
  `X-Token`, or `Authorization: Bearer`.
- Comparison is constant-time over equal-length byte arrays.
- The launcher opens the tokenized URL and prints it as a fallback. The token
  may appear in browser history; that is accepted for a loopback, short-lived
  process. Closing Terra invalidates it.
- Static assets are public on loopback so the page can load. They contain no
  state and no secrets.

## HTTP hardening

- Only `GET` and `POST` are accepted; other methods return `405`.
- Request headers are limited to 16 KiB and 64 fields.
- Bodies are limited to 64 KiB; a larger `Content-Length` is rejected before
  reading.
- URLs are limited to 512 bytes, query parameters to 32, keys to 64 bytes and
  values to 4 KiB.
- `Transfer-Encoding` is rejected; request smuggling through chunked bodies is
  not supported.
- Target paths must start with `/`, may not contain `..` or NUL, and are
  percent-decoded with strict hex validation.
- Unknown routes return `404`; malformed requests return `400`.
- Responses set `X-Content-Type-Options: nosniff`, `Referrer-Policy:
  no-referrer`, `Cache-Control: no-store`, and a restrictive CSP for HTML.
- Exports use `Content-Disposition: attachment`, so a chronicle cannot execute
  as a page.
- Error messages never include file contents, tokens or internal paths.

## Parsing

Every parser has hard limits and returns errors instead of panicking:

- scenario text: 8 KiB, 96 lines, 512 bytes per line, 32 entries, 80-character
  names;
- JSON: 256 KiB, depth 32, 64 KiB strings, 4,096 array items, 1,024 object
  members, no duplicate keys, no trailing data, finite numbers only;
- URL and query limits above;
- chronicle events: 200,000 per run, 12 causes, 120-character titles,
  600-character details, severity in `[0, 1]`.

A fuzz harness feeds guaranteed-malformed documents and a mutation sweep through
the real parsers. No panic is acceptable; the campaign is expected to reject
every constructed malformed case.

## Denial of service

- At most two generations run concurrently. Additional requests receive `429`.
- Finished worlds are cached up to ten; the oldest finished world is evicted.
- Each world is bounded by subdivision 5 in the Studio (10,242 cells), 20,000
  years, 12 civilizations, 200,000 events and roughly 101 political snapshots.
- SSE streams close after ten minutes and after completion.
- Rendering dimensions are clamped to 2,048×1,024.
- Cause chains are capped at 48 levels and 5,000 visited nodes.
- Query pages are capped at 200 events in the Studio API, 500 in the library.

A local user can still request expensive work; the limits make the work
bounded, not free.

## Dynamic content

The client escapes every string inserted into HTML (`& < > " '`). Numeric ids
are parsed as numbers before use. Event text, civilization names and scenario
values are never injected as markup.

## Non-goals

Terra does not defend against a hostile local process with the same user
account. It is not a multi-user service, does not terminate TLS, and should not
be exposed on a public interface. There is no sandbox around generated content
beyond the typed event model.

## Reporting

This is a personal project without a security team. For the purposes of this
repository, a security issue is a failure of one of the invariants above:
unauthenticated state change, out-of-bounds file access, a parser panic on
hostile input, or code execution through exported content. The tests and fuzz
harness are the regression suite for those invariants.
