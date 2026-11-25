#!/usr/bin/env python3
"""Playwright checks for the interactive Studio features.

Covers the example scenario picker, the custom-scenario path, political
playback, the people detail panel, layer toggles and the replay/share link.
Run after `cargo build --release`:

    python3 tests/studio/test_features.py
"""
import os
import re
import subprocess
import sys
import time

from playwright.sync_api import sync_playwright

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BINARY = os.path.join(ROOT, "target", "release", "terra")


def start_server():
    proc = subprocess.Popen(
        [BINARY, "--no-open", "--port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    deadline = time.time() + 20
    lines = []
    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            break
        lines.append(line)
        match = re.search(r"(http://127\.0\.0\.1:\d+/\?t=[0-9a-f]+)", line)
        if match:
            return proc, match.group(1)
    proc.kill()
    raise RuntimeError("server did not start: " + "".join(lines))


def main():
    proc, url = start_server()
    errors = []
    try:
        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True)
            context = browser.new_context(
                viewport={"width": 1440, "height": 1000},
                permissions=["clipboard-read", "clipboard-write"],
            )
            page = context.new_page()
            page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(url, wait_until="networkidle")

            # Load the bundled micro scenario and forge it (162 cells).
            page.locator("#scenario-box summary").click()
            deadline = time.time() + 20
            while page.locator("#example-select option").count() < 7:
                if time.time() > deadline:
                    raise AssertionError("example scenarios did not load")
                time.sleep(0.2)
            page.select_option("#example-select", "micro")
            assert "subdiv = 2" in page.input_value("#scenario")
            page.select_option("#years", "1000")
            page.select_option("#civs", "4")
            page.click("#generate")
            deadline = time.time() + 120
            while time.time() < deadline:
                if "forged" in page.inner_text("#status-text"):
                    break
                time.sleep(0.25)
            status = page.inner_text("#status-text")
            assert "162 cells" in status, status
            assert "auto=1" in page.url
            page.wait_for_selector("#progress", state="hidden", timeout=30_000)

            # People detail panel.
            page.locator(".civ-row").first.click()
            page.wait_for_timeout(500)
            detail = page.inner_text("#civ-detail")
            assert "technology" in detail and "territory" in detail, detail[:200]

            # Playback starts the political year at 0 and advances.
            page.click("#play")
            time.sleep(1.4)
            assert page.inner_text("#play") == "Stop"
            year = int(page.input_value("#year"))
            assert year > 0, year
            page.keyboard.press("Escape")
            page.wait_for_timeout(200)
            assert page.inner_text("#play") == "Play"

            # Layer toggles issue new map requests without errors.
            page.uncheck("#rivers")
            page.wait_for_timeout(400)
            page.uncheck("#shade")
            page.wait_for_timeout(400)
            page.check("#rivers")

            # Share copies a replay link containing the seed and auto flag.
            page.click("#share")
            page.wait_for_timeout(300)
            link = page.evaluate("navigator.clipboard.readText()")
            assert "seed=" in link and "auto=1" in link and "years=" in link, link
            assert "t=" in link

            # The copied link regenerates the world in a fresh page.
            replay = context.new_page()
            replay.goto(link, wait_until="networkidle")
            deadline = time.time() + 120
            while time.time() < deadline:
                if "forged" in replay.inner_text("#status-text"):
                    break
                time.sleep(0.25)
            assert "162 cells" in replay.inner_text("#status-text")

            assert not errors, errors
            browser.close()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    print("STUDIO FEATURE E2E PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
