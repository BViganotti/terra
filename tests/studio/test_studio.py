#!/usr/bin/env python3
"""End-to-end Studio test.

Starts the release launcher on a free loopback port, drives Chromium through
Playwright, forges a world, switches layers, scrubs the political year, inspects
a cell, filters the chronicle, follows a cause link and downloads a map.

Run from the repository root after `cargo build --release`:

    python3 tests/studio/test_studio.py

Playwright and a Chromium build are development-only tools; Terra itself has no
external runtime dependencies.
"""
import os
import re
import subprocess
import sys
import time

from playwright.sync_api import sync_playwright

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BINARY = os.path.join(ROOT, "target", "release", "terra")
ARTIFACTS = os.path.join(os.path.dirname(__file__), "artifacts")


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
    os.makedirs(ARTIFACTS, exist_ok=True)
    proc, url = start_server()
    errors = []
    try:
        with sync_playwright() as pw:
            browser = pw.chromium.launch(headless=True)
            page = browser.new_page(viewport={"width": 1440, "height": 1100})
            page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(url, wait_until="networkidle")
            assert "Terra" in page.title()

            page.fill("#seed", "42")
            page.select_option("#years", "1000")
            page.select_option("#civs", "4")
            page.click("#generate")

            deadline = time.time() + 180
            while time.time() < deadline:
                status = page.inner_text("#status-text")
                if "forged" in status:
                    break
                time.sleep(0.3)
            else:
                raise AssertionError("generation timed out: " + page.inner_text("#status-text"))
            page.wait_for_selector("#progress", state="hidden", timeout=30_000)
            print("status:", page.inner_text("#status-text"))

            assert page.locator("#layers button").count() == 10
            canvas = page.evaluate(
                """() => {
                  const c = document.getElementById('map');
                  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
                  let lit = 0; const colors = new Set();
                  for (let i = 0; i < d.length; i += 4 * 97) {
                    if (d[i] + d[i + 1] + d[i + 2] > 20) lit += 1;
                    colors.add(d[i] + ',' + d[i + 1] + ',' + d[i + 2]);
                  }
                  return { lit, colors: colors.size };
                }"""
            )
            assert canvas["lit"] > 200, canvas
            assert canvas["colors"] > 20, canvas
            print("canvas:", canvas)

            for layer in ["Biomes", "Plates", "Rivers", "Political"]:
                page.get_by_role("button", name=layer, exact=True).click()
                page.wait_for_timeout(350)

            # Scrub the political year through the full range.
            page.evaluate(
                """() => { const el = document.getElementById('year');
                     el.value = 0; el.dispatchEvent(new Event('input', { bubbles: true })); }"""
            )
            page.wait_for_timeout(350)
            page.evaluate(
                """() => { const el = document.getElementById('year');
                     el.value = el.max; el.dispatchEvent(new Event('input', { bubbles: true })); }"""
            )
            page.wait_for_timeout(500)

            page.locator("#map").click(position={"x": 500, "y": 240})
            page.wait_for_timeout(600)
            inspector = page.inner_text("#inspector")
            assert "elevation" in inspector, inspector
            print("inspector:", inspector.splitlines()[0])

            page.wait_for_selector(".event", timeout=30_000)
            assert page.locator(".event").count() >= 10
            page.locator(".event").nth(2).click()
            page.wait_for_timeout(600)
            cause = page.evaluate("document.getElementById('cause-body').textContent")
            assert "Direct causes" in cause, cause[:300]
            assert "Causal chain" in cause or "Paths to physical facts" in cause
            # Follow the first cause link back to the world formation event.
            page.locator("#cause-body .event-link").first.click()
            page.wait_for_timeout(600)
            cause2 = page.evaluate("document.getElementById('cause-body').textContent")
            assert cause2 != cause
            print("cause panel: ok")

            page.select_option("#filter-kind", "expansion")
            page.wait_for_timeout(700)
            assert "Expansion" in page.inner_text("#event-count") or page.locator(".event").count() > 0
            page.fill("#filter-q", "river")
            page.wait_for_timeout(700)
            page.click("#filter-reset")
            page.wait_for_timeout(700)
            page.click("#load-more")
            page.wait_for_timeout(700)
            assert page.locator(".event").count() > 100
            print("filters and paging: ok")

            with page.expect_download() as download_info:
                page.click("#download-map")
            download = download_info.value
            assert download.suggested_filename.endswith(".png")
            print("download:", download.suggested_filename)

            page.screenshot(path=os.path.join(ARTIFACTS, "studio.png"), full_page=True)
            assert not errors, errors
            browser.close()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    print("STUDIO E2E PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
