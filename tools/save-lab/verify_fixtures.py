"""Read-only real-save regression. Reports hashes/counts; never copies game data."""
import argparse
import collections
from pathlib import Path
import json
from datetime import datetime, timezone

from sbin import Document, digest, require


def semantics(document):
    return {p: (v.type, v.value) for p, v in document.walk().items() if v.type != 15}


def verify(root):
    report = {"created_utc": datetime.now(timezone.utc).isoformat(), "fixture_root": str(Path(root).resolve()),
              "parser_sha256": digest(Path(__file__).with_name("sbin.py").read_bytes()),
              "files": [], "failures": [], "operations": collections.Counter(), "types": collections.Counter()}
    loaded = []
    for path in sorted(Path(root).rglob("*.sb")):
        try:
            raw = path.read_bytes()
            doc = Document(raw)
            fields = {p: v.value for p, v in doc.walk().items() if v.type != 15}
            require(doc.rebuild({}) == raw, "container round trip changed bytes")
            require(doc.edit(fields) == raw, "same-value scalar edit changed bytes")
            clone = Document(doc.import_module("", doc.export_module("")))
            require(semantics(clone) == semantics(doc), "full graph relocation changed values")
            require(path.read_bytes() == raw, "original changed")
            report["operations"]["identity"] += 1
            report["operations"]["scalar_same_value"] += len(fields)
            report["operations"]["root_graph_relocation"] += 1
            report["types"].update(f"scalar-0x{v.type:02x}" for v in doc.walk().values() if v.type != 15)
            report["types"].update(f"list-0x{n.element_type:02x}" for n in doc.nodes.values() if n.kind == "list")
            report["files"].append({"file": path.relative_to(root).as_posix(), "sha256": digest(raw),
                                    "bytes": len(raw), "objects": len(doc.entries), "scalar_paths": len(fields),
                                    "root_offset": doc.entries[0] >> 3})
            loaded.append((path, doc))
        except Exception as error:
            report["failures"].append({"file": path.relative_to(root).as_posix(), "error": str(error)})
    require(loaded, "no valid SBIN fixtures found")
    for path, doc in loaded:
        try:
            if "player/inventory/items" in doc.walk():
                for items in ([], [{"type": "SAVE_LAB_NEW_ITEM", "count": 7}] * 32):
                    edited = Document(doc.replace_list("player/inventory/items", items))
                    node = edited.node(edited.locate("player/inventory/items").value)
                    require(len(node.items) == len(items), "inventory resize failed")
                    left = {p: v for p, v in semantics(doc).items() if not p.startswith("player/inventory/items/")}
                    right = {p: v for p, v in semantics(edited).items() if not p.startswith("player/inventory/items/")}
                    require(left == right, "inventory resize changed unrelated state")
                    report["operations"]["inventory_resize"] += 1
            if "player" in doc.walk():
                donor = next((other for other_path, other in loaded if other_path != path and "player" in other.walk()), None)
                if donor:
                    result = Document(doc.import_module("player", donor.export_module("player")))
                    donor_player = {p: v for p, v in semantics(donor).items() if p.startswith("player/")}
                    result_player = {p: v for p, v in semantics(result).items() if p.startswith("player/")}
                    require(donor_player == result_player, "player transplant mismatch")
                    old_other = {p: v for p, v in semantics(doc).items() if not p.startswith("player/")}
                    new_other = {p: v for p, v in semantics(result).items() if not p.startswith("player/")}
                    require(old_other == new_other, "transplant changed scene/progress")
                    report["operations"]["player_transplant"] += 1
        except Exception as error:
            report["failures"].append({"file": path.relative_to(root).as_posix(), "error": str(error)})
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = verify(args.root)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x", encoding="utf-8") as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    print(json.dumps({"files": len(report["files"]), "operations": report["operations"], "failures": report["failures"]}, ensure_ascii=False))
    raise SystemExit(bool(report["failures"]))
