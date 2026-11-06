#!/usr/bin/env python3
"""Offline save analysis/editing. SBIN commands create new files only."""
import argparse
import json
from pathlib import Path
import sys

import cold_profiles
import progress
from sbin import Document, MAX_SIZE, SaveError, digest, require


def read_json(path):
    path = Path(path)
    require(path.stat().st_size <= MAX_SIZE, "JSON input exceeds size limit")
    return json.loads(path.read_text(encoding="utf-8-sig"))


def load(path):
    path = cold_profiles.safe_path(path)
    require(path.stat().st_size <= MAX_SIZE, "SBIN input exceeds size limit")
    return Document(path.read_bytes())


def write_new(path, content):
    path = cold_profiles.safe_path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation intentionally rejects source overwrite and existing outputs.
    with path.open("xb") as handle:
        handle.write(content)
    return {"output": str(path), "bytes": len(content), "sha256": digest(content)}


def output_json(value, path=None):
    content = (json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")
    if path:
        return write_new(path, content)
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("inspect", "verify"):
        p = sub.add_parser(command)
        p.add_argument("file")
        p.add_argument("--out")
    p = sub.add_parser("diff")
    p.add_argument("before")
    p.add_argument("after")
    p.add_argument("--out")
    p = sub.add_parser("edit")
    p.add_argument("file")
    p.add_argument("changes", help="JSON object mapping paths to values")
    p.add_argument("--out", required=True)
    p = sub.add_parser("export")
    p.add_argument("file")
    p.add_argument("path", help="slash-separated field path; empty string exports root")
    p.add_argument("--out", required=True)
    p = sub.add_parser("import")
    p.add_argument("file")
    p.add_argument("path")
    p.add_argument("module")
    p.add_argument("--out", required=True)
    p = sub.add_parser("list-replace")
    p.add_argument("file")
    p.add_argument("path")
    p.add_argument("items", help="JSON list; inventory entries have type/name and count")
    p.add_argument("--out", required=True)
    p = sub.add_parser("attach")
    p.add_argument("file")
    p.add_argument("parent")
    p.add_argument("name")
    p.add_argument("module")
    p.add_argument("--out", required=True)
    p = sub.add_parser("detach")
    p.add_argument("file")
    p.add_argument("path")
    p.add_argument("--out", required=True)
    p = sub.add_parser("profile-snapshot")
    p.add_argument("documents")
    p.add_argument("bank")
    p.add_argument("name")
    p = sub.add_parser("profile-verify")
    p.add_argument("entry")
    p = sub.add_parser("profile-activate")
    p.add_argument("entry")
    p.add_argument("documents")
    p.add_argument("--backups", required=True)
    p = sub.add_parser("progress-transplant")
    p.add_argument("file")
    p.add_argument("donor")
    p.add_argument("--out", required=True)
    for command in ("progress-export", "progress-import"):
        p = sub.add_parser(command)
        p.add_argument("file")
        if command == "progress-import":
            p.add_argument("anchor")
        p.add_argument("--out", required=True)
    for command in ("adb-pull", "adb-activate"):
        p = sub.add_parser(command)
        p.add_argument("--adb", required=True)
        p.add_argument("--serial", required=True)
        p.add_argument("--package", default="org.touchhle.android.savelab")
        if command == "adb-pull":
            p.add_argument("bank")
            p.add_argument("name")
        else:
            p.add_argument("entry")
            p.add_argument("--backups", required=True)
    p = sub.add_parser("slot-copy")
    p.add_argument("target_entry")
    p.add_argument("donor_entry")
    p.add_argument("bank")
    p.add_argument("name")
    p.add_argument("--target-slot", type=int, choices=(0, 1, 2), default=0)
    p.add_argument("--donor-slot", type=int, choices=(0, 1, 2), default=0)
    p.add_argument("--story-only", action="store_true")
    p = sub.add_parser("node-bank-inspect")
    p.add_argument("bank")
    p.add_argument("--out")
    args = parser.parse_args(argv)
    try:
        command = args.command
        if command == "node-bank-inspect":
            from node_bank import inspect_bank
            result = output_json(inspect_bank(args.bank), args.out)
        elif command.startswith("adb-"):
            from adb_profiles import AdbProfiles
            device = AdbProfiles(args.adb, args.serial, args.package)
            if command == "adb-pull":
                result = {"profile": str(device.pull(args.bank, args.name))}
            else:
                result = device.activate(args.entry, args.backups)
        elif command == "slot-copy":
            from profile_ops import slot_copy
            result = {"profile": str(slot_copy(args.target_entry, args.donor_entry, args.target_slot,
                        args.donor_slot, args.bank, args.name, args.story_only))}
        elif command == "profile-snapshot":
            result = {"profile": str(cold_profiles.snapshot(args.documents, args.bank, args.name))}
        elif command == "profile-verify":
            listing = cold_profiles.verify(args.entry)
            result = {"verified": True, "files": len(listing["files"])}
        elif command == "profile-activate":
            result = cold_profiles.activate(args.entry, args.documents, args.backups)
        elif command == "diff":
            before_summary, after_summary = load(args.before).summary(), load(args.after).summary()
            before = {**before_summary["containers"], **before_summary["values"]}
            after = {**after_summary["containers"], **after_summary["values"]}
            changes = []
            for path in sorted(before.keys() | after.keys()):
                left, right = before.get(path), after.get(path)
                left = {k: v for k, v in left.items() if k != "data_offset"} if left else None
                right = {k: v for k, v in right.items() if k != "data_offset"} if right else None
                if left != right:
                    changes.append({"path": path, "before": left, "after": right})
            result = output_json({"changes": changes}, args.out)
        else:
            document = load(args.file)
            if command == "inspect":
                result = output_json(document.summary(), args.out)
            elif command == "verify":
                require(document.rebuild({}) == document.original, "lossless round trip failed")
                result = output_json({"verified": True, "sha256": digest(document.original),
                                      "objects": len(document.entries), "paths": len(document.walk())}, args.out)
            elif command == "export":
                result = output_json(document.export_module(args.path), args.out)
            elif command == "progress-export":
                result = output_json(progress.export_anchor(document), args.out)
            else:
                if command == "edit":
                    changes = read_json(args.changes)
                    require(isinstance(changes, dict), "changes must be a JSON object")
                    content = document.edit(changes)
                elif command == "import":
                    content = document.import_module(args.path, read_json(args.module))
                elif command == "list-replace":
                    items = read_json(args.items)
                    require(isinstance(items, list), "items must be a JSON list")
                    content = document.replace_list(args.path, items)
                elif command == "attach":
                    content = document.attach(args.parent, args.name, read_json(args.module))
                elif command == "progress-transplant":
                    content, _ = progress.transplant(document, load(args.donor))
                elif command == "progress-import":
                    content, _ = progress.apply_anchor(document, read_json(args.anchor))
                else:
                    content = document.detach(args.path)
                result = write_new(args.out, content)
        print(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False))
        return 0
    except (SaveError, OSError, ValueError, KeyError, TypeError, RecursionError) as error:
        print(f"save-lab: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
