#!/usr/bin/env python3
"""Import the latest six-column ledger into overlay, native SBIN, and chapter assets."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import struct
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from openpyxl import load_workbook


HEADERS = ("string_id", "zh_original", "en_original", "ASS", "video_position", "revised")
PLACEHOLDER = re.compile(r"\{[^{}]+\}")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fnv1(data: bytes) -> int:
    value = 0x811C9DC5
    for byte in data:
        value = ((value * 0x01000193) & 0xFFFFFFFF) ^ byte
    return value


def normalize(value) -> str:
    if value is None:
        return ""
    if value is True:
        return "TRUE"
    if value is False:
        return "FALSE"
    return str(value)


def read_ledger(path: Path) -> list[dict[str, str]]:
    sheet = load_workbook(path, read_only=True, data_only=True).worksheets[0]
    rows = list(sheet.iter_rows(values_only=True))
    if not rows or tuple(rows[0]) != HEADERS:
        raise SystemExit(f"unexpected ledger header: {rows[0] if rows else None}")
    result = []
    seen = set()
    for row in rows[1:]:
        if row[0] is None:
            continue
        values = dict(zip(HEADERS, row))
        string_id = normalize(values["string_id"])
        revised = normalize(values["revised"]).strip()
        English = normalize(values["en_original"]).strip()
        if not string_id or string_id in seen or not revised:
            raise SystemExit(f"invalid ledger row: id={string_id!r}")
        seen.add(string_id)
        original = normalize(values["zh_original"])
        if set(PLACEHOLDER.findall(original)) != set(PLACEHOLDER.findall(revised)):
            raise SystemExit(f"placeholder mismatch: {string_id}")
        result.append({
            "id": string_id,
            "original": original.strip(),
            "text": revised,
            "english": English,
        })
    return result


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def apply_corrections(root: Path, ledger: list[dict[str, str]]) -> list[str]:
    path = root / "tools/translation/text-corrections.json"
    if not path.exists():
        return []
    document = load_json(path)
    if document.get("schema") != 1:
        raise SystemExit("unexpected text-correction schema")
    by_id = {entry["id"]: entry for entry in ledger}
    applied = []
    for correction in document["corrections"]:
        key = correction["id"]
        entry = by_id.get(key)
        if key in applied or entry is None or entry["text"] not in correction["expected"]:
            raise SystemExit(f"text correction requires review: {key}")
        text = correction["text"]
        if not text or set(PLACEHOLDER.findall(entry["text"])) != set(PLACEHOLDER.findall(text)):
            raise SystemExit(f"invalid text correction: {key}")
        entry["text"] = text
        applied.append(key)
    return applied


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def update_overlay_json(path: Path, ledger: list[dict[str, str]], source_sha: str) -> list[str]:
    document = load_json(path)
    by_id = {entry["id"]: entry for entry in ledger}
    current_ids = {entry["id"] for entry in document["entries"]}
    if current_ids != set(by_id):
        raise SystemExit(
            f"{path}: ID set changed; overlay IDs={len(current_ids)} ledger IDs={len(by_id)}"
        )
    changed = []
    for entry in document["entries"]:
        latest = by_id[entry["id"]]
        if entry.get("text") != latest["text"] or entry.get("english", "") != latest["english"]:
            changed.append(entry["id"])
        entry["text"] = latest["text"]
        entry["english"] = latest["english"]
    document["sourceSha256"] = source_sha.lower()
    write_json(path, document)
    return changed


def sections(raw: bytes) -> tuple[list[tuple[bytes, bytes, bytes]], dict[bytes, bytes]]:
    if raw[:8] != b"SBIN\x02\0\0\0":
        raise SystemExit("expected SBIN v2")
    result = []
    by_tag = {}
    cursor = 8
    while cursor < len(raw):
        tag, size, checksum = struct.unpack_from("<4sII", raw, cursor)
        end = cursor + 12 + size
        aligned = (end + 3) & ~3
        if aligned > len(raw):
            raise SystemExit(f"truncated {tag!r} section")
        payload = raw[cursor + 12 : end]
        if fnv1(payload) != checksum:
            raise SystemExit(f"bad {tag!r} checksum")
        result.append((tag, payload, raw[end:aligned]))
        by_tag[tag] = payload
        cursor = aligned
    return result, by_tag


def pack_strings(strings: list[str]) -> bytes:
    body = bytearray(struct.pack("<I", len(strings)))
    for value in strings:
        encoded = value.encode("utf-8")
        body += struct.pack("<I", len(encoded)) + encoded
    return bytes(body)


def unpack_strings(payload: bytes) -> list[str]:
    count = struct.unpack_from("<I", payload)[0]
    cursor = 4
    result = []
    for _ in range(count):
        size = struct.unpack_from("<I", payload, cursor)[0]
        cursor += 4
        result.append(payload[cursor : cursor + size].decode("utf-8"))
        cursor += size
    if cursor != len(payload):
        raise SystemExit("unexpected string-table trailing bytes")
    return result


def rebuild(raw: bytes, replacements: dict[bytes, bytes]) -> bytes:
    parsed, _ = sections(raw)
    out = bytearray(raw[:8])
    for tag, payload, padding in parsed:
        payload = replacements.get(tag, payload)
        out += struct.pack("<4sII", tag, len(payload), fnv1(payload)) + payload
        aligned = (-len(out)) % 4
        out += padding if len(padding) == aligned else b"\0" * aligned
    return bytes(out)


def update_native_sbin(
    path: Path, ledger: list[dict[str, str]]
) -> tuple[list[str], int]:
    raw = path.read_bytes()
    _, by_tag = sections(raw)
    strings = unpack_strings(by_tag[b"STRS"])
    old_strings = list(strings)
    old_count = len(strings)
    ohdr = list(struct.unpack(f"<{len(by_tag[b'OHDR']) // 4}I", by_tag[b"OHDR"]))
    if len(ohdr) != 3:
        raise SystemExit("unexpected native OHDR")
    list_offset = ohdr[2] >> 3
    data = bytearray(by_tag[b"DATA"])
    kind, count = struct.unpack_from("<II", data, list_offset)
    if kind != 16 or count != len(ledger):
        raise SystemExit(f"unexpected native record list: kind={kind} count={count}")
    latest = {entry["id"]: entry["text"] for entry in ledger}
    records = list_offset + 8
    seen = set()
    changes = []
    for index in range(count):
        offset = records + index * 8
        name_id, value_id, _category = struct.unpack_from("<HHI", data, offset)
        if not (1 <= name_id <= len(strings) and 1 <= value_id <= len(strings)):
            raise SystemExit(f"invalid native string reference at record {index}")
        string_id = strings[name_id - 1]
        if string_id not in latest or string_id in seen:
            raise SystemExit(f"unexpected native ID: {string_id!r}")
        seen.add(string_id)
        revised = latest[string_id]
        if strings[value_id - 1] != revised:
            strings.append(revised)
            if len(strings) > 0xFFFF:
                raise SystemExit("native string table exceeds u16 references")
            struct.pack_into("<H", data, offset + 2, len(strings))
            changes.append(string_id)
    if seen != set(latest):
        raise SystemExit("native ID set does not match ledger")
    new_strs = pack_strings(strings)
    output = rebuild(raw, {b"DATA": bytes(data), b"STRS": new_strs})

    _, check_sections = sections(output)
    check_strings = unpack_strings(check_sections[b"STRS"])
    if check_strings[:old_count] != old_strings:
        raise SystemExit("native update altered existing strings")
    check_data = check_sections[b"DATA"]
    for index in range(count):
        offset = records + index * 8
        name_id, value_id, _category = struct.unpack_from("<HHI", check_data, offset)
        string_id = check_strings[name_id - 1]
        if check_strings[value_id - 1] != latest[string_id]:
            raise SystemExit(f"native verification failed: {string_id}")
    path.write_bytes(output)
    return changes, len(strings)


def chapter_titles(entries: list[dict[str, str]], english: bool = False) -> list[str]:
    by_id = {entry["id"]: entry for entry in entries}
    return [
        by_id[f"CHAPTER_{number:02d}_GAME_TITLE"]["english" if english else "text"]
        for number in range(1, 13)
    ]


def update_preview(path: Path, chapters: list[dict[str, str]], font_data: bytes | None) -> None:
    if not path.exists():
        return
    text = path.read_text(encoding="utf-8")
    compact = json.dumps(chapters, ensure_ascii=False, separators=(", ", ": "))
    text, count = re.subn(r"const chapters=\[.*?\],", f"const chapters={compact},", text, count=1)
    if "const chapters=" in text and count != 1:
        raise SystemExit(f"cannot update chapter list in {path}")
    if font_data is not None:
        encoded = base64.b64encode(font_data).decode("ascii")
        pattern = r"(@font-face\{font-family:(?:ChapterCN|Chapter);src:url\(data:font/ttf;base64,)[^)]+(\))"
        text, count = re.subn(pattern, lambda match: match.group(1) + encoded + match.group(2), text, count=1)
        if "font-family:ChapterCN" in text and count != 1:
            raise SystemExit(f"cannot update embedded Chinese chapter font in {path}")
    path.write_text(text, encoding="utf-8")


def rebuild_chapter_font(
    source: Path, output: Path, old_titles: list[str], new_titles: list[str]
) -> bytes:
    characters = set("第章0123456789")
    for title in old_titles + new_titles:
        characters.update(title)
    font = TTFont(source)
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.name_languages = ["*"]
    options.notdef_glyph = True
    options.notdef_outline = True
    options.recommended_glyphs = True
    options.symbol_cmap = True
    options.legacy_cmap = True
    options.glyph_names = True
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text="".join(sorted(characters)))
    subsetter.subset(font)
    for record in font["name"].names:
        if record.nameID in (1, 4, 6):
            record.string = "DSChapterChinese"
        elif record.nameID == 2:
            record.string = "Regular"
    font["head"].modified = 0
    temp = output.with_suffix(".tmp.ttf")
    font.save(temp)
    data = temp.read_bytes()
    output.write_bytes(data)
    temp.unlink()
    check = TTFont(output)
    cmap = {codepoint for table in check["cmap"].tables for codepoint in table.cmap}
    missing = sorted({character for character in characters if ord(character) not in cmap})
    if missing:
        raise SystemExit(f"chapter font still misses glyphs: {missing}")
    return data


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", required=True, type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()

    root = args.root.resolve()
    ledger_path = args.ledger.resolve()
    ledger = read_ledger(ledger_path)
    corrections = apply_corrections(root, ledger)
    source_sha = digest(ledger_path.read_bytes())

    overlay_paths = [
        root / "src/ds_overlay_texts.json",
        root / "android/app/src/main/assets/overlay/subtitles-original.json",
    ]
    overlay_paths = [path for path in overlay_paths if path.exists()]
    current_entries = load_json(overlay_paths[0])["entries"]
    old_titles = chapter_titles(current_entries)
    new_titles = chapter_titles(ledger)
    changed_by_file = {}
    for path in overlay_paths:
        changed_by_file[str(path)] = update_overlay_json(path, ledger, source_sha)

    native_changes, native_strings = update_native_sbin(
        root / "res/overlay/deadspace_iphone.bin", ledger
    )

    source_font = root / "android/app/src/main/assets/touchHLE_ds_fonts/SweiShearSansCJKsc-Medium.ttf"
    font_outputs = [
        root / "res/overlay/ChapterChinese.ttf",
        root / "android/app/src/main/assets/overlay/ChapterChinese.ttf",
    ]
    font_data = rebuild_chapter_font(source_font, font_outputs[0], old_titles, new_titles)
    for output in font_outputs[1:]:
        if output.parent.exists():
            output.write_bytes(font_data)

    chapters = [
        {
            "id": f"CHAPTER_{number:02d}_GAME_TITLE",
            "text": new_titles[number - 1],
            "english": chapter_titles(ledger, english=True)[number - 1],
        }
        for number in range(1, 13)
    ]
    for preview in (
        root / "tools/translation/RIG-motion-preview.html",
        root / "tools/translation/RIG-long-subtitle-preview.html",
    ):
        update_preview(preview, chapters, font_data)

    default_paths = [
        root / "src/ds_overlay_default.json",
        root / "android/app/src/main/assets/overlay/default-style.json",
    ]
    for path in default_paths:
        if path.exists():
            default = load_json(path)
            default.setdefault("samples", {})["chapter"] = new_titles[0]
            write_json(path, default)

    latest = {entry["id"]: entry for entry in ledger}
    audit = {
        "rows": len(ledger),
        "changed": len(changed_by_file[str(overlay_paths[0])]),
        "changed_from_original": sum(
            entry["original"] != entry["text"] for entry in ledger
        ),
        "stable_original_indices": 995,
        "appended_ids": [],
        "native_bin_keys": len(ledger),
        "native_string_count": native_strings,
        "native_changed_keys": len(native_changes),
        "placeholder_differences": [],
        "interference": [
            entry["id"] for entry in ledger if "（干扰）" in entry["text"]
        ],
        "chapter_chinese_acrostic": "".join(title[0] for title in new_titles),
        "chapter_english_acrostic": "".join(title[0] for title in chapter_titles(ledger, True)),
        "source_sha256": source_sha.upper(),
        "text_corrections": corrections,
        "chapter_titles": new_titles,
    }
    write_json(root / "tools/translation/ledger-audit.json", audit)
    print(json.dumps(audit, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
