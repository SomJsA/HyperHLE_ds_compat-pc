"""Compose actual device captures with the exact experiment field labels."""
import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def wrap(draw, text, font, width):
    lines, current = [], ""
    for char in text:
        if current and draw.textlength(current + char, font=font) > width:
            lines.append(current)
            current = char
        else:
            current += char
    return [*lines, current] if current else lines


def compose(cases_path, runs, output, font_path):
    cases = json.loads(cases_path.read_text(encoding="utf-8"))["cases"]
    output.mkdir(parents=True, exist_ok=True)
    font = ImageFont.truetype(str(font_path), 18)
    title_font = ImageFont.truetype(str(font_path), 23)
    header_font = ImageFont.truetype(str(font_path), 28)
    records, pages = [], []
    for case in cases:
        path = runs / case["id"] / "result.json"
        result = json.loads(path.read_text(encoding="utf-8"))
        records.append((case, result))
    for page_index, offset in enumerate(range(0, len(records), 4)):
        page = Image.new("RGB", (1568, 1446), "#111821")
        draw = ImageDraw.Draw(page)
        draw.text((22, 12), f"剧情点位 · 独立包平板对照  {page_index + 1}", font=header_font, fill="#edf4ff")
        draw.text((22, 53), "每格独立冷加载；未移动角色。画面能进入，不代表后续剧情已验证。", font=font, fill="#aab8c9")
        for i, (case, result) in enumerate(records[offset:offset + 4]):
            x, y = 18 + (i % 2) * 775, 94 + (i // 2) * 667
            draw.rounded_rectangle((x, y, x + 757, y + 660), radius=12, fill="#1e2937")
            draw.text((x + 12, y + 10), case["title"], font=title_font, fill="#ffffff")
            labels = case["labels"]
            lines = [line for label in labels for line in wrap(draw, label, font, 731)]
            for index, line in enumerate(lines):
                draw.text((x + 12, y + 44 + index * 24), line, font=font, fill="#c9dfef")
            require_height = 44 + len(lines) * 24
            if require_height > 171:
                raise ValueError(f"field labels overflow tile: {case['id']}")
            frame_path = Path(result["frame"]["jpg"])
            with Image.open(frame_path) as frame:
                frame = frame.convert("RGB")
                frame.thumbnail((733, 438))
                page.paste(frame, (x + 12 + (733 - frame.width) // 2, y + 176 + (438 - frame.height) // 2))
            status = "HUD state=0；入场后静置 8 秒，再截取画面" if result["status"] == "gameplay" else "未确认加载：" + result.get("error", "unknown")
            status_lines = wrap(draw, status, font, 731)
            for index, line in enumerate(status_lines[:2]):
                draw.text((x + 12, y + 620 + index * 19), line, font=font,
                          fill="#8eebc8" if result["status"] == "gameplay" else "#ffce93")
        target = output / f"story-comparison-{page_index + 1}.jpg"
        quality = 88
        while True:
            page.save(target, quality=quality, optimize=True)
            if target.stat().st_size <= 390000 or quality <= 60:
                break
            quality -= 4
        pages.append({"path": str(target.resolve()), "bytes": target.stat().st_size, "size": list(page.size)})
    (output / "montage-index.json").write_text(json.dumps(pages, ensure_ascii=False, indent=2), encoding="utf-8")
    return pages


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--runs", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--font", type=Path, default=Path("C:/Windows/Fonts/msyh.ttc"))
    args = parser.parse_args()
    print(json.dumps(compose(args.cases, args.runs, args.out, args.font)))
