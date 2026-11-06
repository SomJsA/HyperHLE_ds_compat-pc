"""Read exported node banks into an editor-ready index without activating saves."""
import json
from pathlib import Path
import re

import cold_profiles
from sbin import Document, MAX_SIZE, SaveError, digest, require


def read_json(path, limit=1024 * 1024):
    path = cold_profiles.safe_path(path)
    require(path.stat().st_size <= limit, "oversized node metadata")
    return json.loads(path.read_text(encoding="utf-8"))


def inspect_bank(bank):
    bank = cold_profiles.safe_path(bank)
    nodes = cold_profiles.safe_path(bank / "nodes")
    require(nodes.is_dir(), "node bank is missing nodes directory")
    result = {"format": "deadspace.save-node-index.v1", "bank": str(bank), "nodes": [],
              "invalid_nodes": [], "unsupported_sbin": [], "pending_nodes": [],
              "versions": 0, "invalid_versions": [], "events": 0,
              "unavailable_version_events": 0, "journal_gaps": [], "interrupted_event_files": 0,
              "complete_playthrough_coverage_verified": False}
    for entry in sorted(nodes.iterdir()):
        cold_profiles.safe_path(entry)
        if entry.name.startswith("."):
            result["pending_nodes"].append(entry.name)
            continue
        if not entry.is_dir():
            continue
        try:
            manifest = read_json(entry / "manifest.json", 4 * 1024 * 1024)
            require(manifest.get("format") == cold_profiles.FORMAT, "unknown profile format")
            listing = cold_profiles.inventory(entry / "Documents", validate_sbin=False)
            require(listing == {"files": manifest["files"], "directories": manifest["directories"]}, "node content hash/member mismatch")
            node = read_json(entry / "node.json")
            require(node.get("format") == "deadspace.save-node.v1" and node.get("id") == entry.name, "invalid node identity")
            unparsed = []
            for name in listing["files"]:
                if name.lower().endswith(".sb"):
                    try:
                        path = entry / "Documents" / name
                        require(path.stat().st_size <= MAX_SIZE, "SBIN exceeds editor size limit")
                        Document(path.read_bytes())
                    except SaveError as error:
                        unparsed.append({"node": entry.name, "file": name, "error": str(error)})
            result["unsupported_sbin"].extend(unparsed)
            thumbnail = cold_profiles.safe_path(entry / "thumbnail.jpg")
            result["nodes"].append({"id": entry.name, "entry": str(entry), "created_utc": node.get("created_utc"),
                                    "session": node.get("session"), "label": node.get("label"), "reason": node.get("reason"),
                                    "metadata": node.get("metadata", {}), "files": len(listing["files"]),
                                    "editor_supported": not unparsed, "gameplay_resume_validated": False,
                                    "thumbnail": str(thumbnail) if thumbnail.is_file() else None})
        except (SaveError, OSError, ValueError, KeyError, TypeError) as error:
            result["invalid_nodes"].append({"node": entry.name, "error": str(error)})
    versions = cold_profiles.safe_path(bank / "versions")
    if versions.is_dir():
        for version in sorted(versions.glob("*.sb")):
            cold_profiles.safe_path(version)
            require(version.stat().st_size <= MAX_SIZE, "oversized recorded SBIN version")
            if not re.fullmatch(r"[0-9a-f]{64}", version.stem) or digest(version.read_bytes()) != version.stem:
                result["invalid_versions"].append(version.name)
            else:
                result["versions"] += 1
    journal = cold_profiles.safe_path(bank / "events.jsonl")
    if journal.is_file():
        require(journal.stat().st_size <= 128 * 1024 * 1024, "event journal exceeds reader limit")
        last = 0
        with journal.open(encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, 1):
                try:
                    event = json.loads(line)
                    sequence = event["sequence"]
                    require(isinstance(sequence, int) and not isinstance(sequence, bool), "invalid event sequence")
                    if sequence != last + 1:
                        result["journal_gaps"].append({"line": line_number, "expected": last + 1, "actual": sequence})
                    last = sequence
                    result["events"] += 1
                    result["unavailable_version_events"] += not bool(event.get("stored"))
                except (ValueError, KeyError, TypeError) as error:
                    result["journal_gaps"].append({"line": line_number, "error": str(error)})
    result["interrupted_event_files"] = len(list(bank.glob("events-interrupted-*.txt")))
    return result
