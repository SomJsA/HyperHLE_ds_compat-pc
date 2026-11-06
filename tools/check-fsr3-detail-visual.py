#!/usr/bin/env python3
"""Render and smoke-test the generated FSR detail visualization."""

from __future__ import annotations

import argparse
from pathlib import Path

from playwright.sync_api import sync_playwright

ROI_LABELS = {
    "armor-pixel": "装甲发光细节",
    "weapon-pixel": "武器硬边 / 地面",
    "poster-pixel": "海报文字",
    "metal-pixel": "金属扫描线",
    "hud-pixel": "HUD 字体",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--preview", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--browser-channel", default="msedge")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    preview_uri = args.preview.resolve().as_uri()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(channel=args.browser_channel, headless=True)
        page = browser.new_page(viewport={"width": 1100, "height": 900})
        errors: list[str] = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(preview_uri)
        page.wait_for_timeout(400)

        frame = page.frame_locator("iframe")
        root = frame.locator("#fsr3-detail-compare")
        assert root.count() == 1
        assert frame.locator("#fsr3-detail-compare img").count() == 9

        content_height = root.evaluate("(element) => element.scrollHeight + 40")
        page.locator("iframe").evaluate(
            "(element, height) => { element.style.height = `${height}px`; "
            "element.style.minHeight = `${height}px`; }",
            content_height,
        )
        page.set_viewport_size({"width": 1100, "height": content_height + 32})
        page.screenshot(
            path=args.output_dir / "fsr3-preview-full.jpg",
            full_page=True,
            type="jpeg",
            quality=65,
        )

        for roi, roi_label in ROI_LABELS.items():
            frame.locator(f"[data-roi={roi}]").click()
            page.wait_for_timeout(50)
            assert frame.locator(f"[data-roi={roi}]").get_attribute("aria-pressed") == "true"
            assert roi_label in frame.locator("#compare-image-03-native-scale3").get_attribute("alt")

        page.set_viewport_size({"width": 390, "height": 900})
        overflow = frame.locator("#fsr3-detail-compare").evaluate(
            "(element) => element.scrollWidth - element.clientWidth"
        )
        assert overflow <= 1, f"mobile horizontal overflow: {overflow}px"
        assert not errors, errors

        page.locator("iframe").screenshot(
            path=args.output_dir / "fsr3-preview-mobile.jpg",
            type="jpeg",
            quality=65,
        )
        print(
            f"content_height={content_height} overflow={overflow} "
            f"errors={len(errors)} screenshots={args.output_dir}"
        )
        browser.close()


if __name__ == "__main__":
    main()
