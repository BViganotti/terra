# Studio end-to-end test

Two scripts start the release launcher on a free loopback port and drive a real
Chromium page with Playwright:

- `test_studio.py` checks the core flow: forge, layers, year scrub, inspection,
  chronicle, causes and map download.
- `test_features.py` checks the interactive features: example scenarios, custom
  scenario handling, playback, the people detail panel, layer toggles and the
  replay/share link.

## Prerequisites

- A release binary: `cargo build --release`
- Python 3 with Playwright (`python3 -m pip install playwright` if needed)
- A Playwright Chromium build (`python3 -m playwright install chromium`)

These are development-only tools. Terra itself has no external runtime
dependencies.

## Run

```sh
cargo build --release
python3 tests/studio/test_studio.py
python3 tests/studio/test_features.py
```

The script prints each stage and exits non-zero on failure. It writes a
full-page screenshot to `tests/studio/artifacts/studio.png`, which is ignored by
git.

## What it checks

1. The page loads with the correct title.
2. A seed, span and number of peoples are entered and forged.
3. Generation completes and the progress bar disappears.
4. Ten layer buttons exist and the canvas contains a real, varied raster.
5. Biomes, Plates, Rivers and Political layers load.
6. The political year scrubber moves from 0 to the final year.
7. Clicking the map populates the inspector with a cell reading.
8. The chronicle has events; selecting one shows direct causes and a chain.
9. Following a cause link changes the cause panel.
10. A kind filter, text search, reset and pagination work.
11. Downloading the map produces a `.png` file.

## Share links

`test_features.py` loads a custom micro scenario, forges it, clicks Share and
opens the copied link in a fresh page. The scenario is carried in the URL
fragment, so a replay link reproduces the same 162-cell world without server
state.

## Notes

- No network access is required; all requests go to `127.0.0.1`.
- The token comes from the launcher's printed URL, exactly as in normal use.
- If Playwright is unavailable, the equivalent endpoint contract is covered by
  `crates/terra-studio/tests/http_api.rs`.
