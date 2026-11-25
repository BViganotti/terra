# Studio end-to-end test

`test_studio.py` starts the release launcher on a free loopback port, drives a
real Chromium page with Playwright, and checks the product flow.

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

## Notes

- No network access is required; all requests go to `127.0.0.1`.
- The token comes from the launcher's printed URL, exactly as in normal use.
- If Playwright is unavailable, the equivalent endpoint contract is covered by
  `crates/terra-studio/tests/http_api.rs`.
