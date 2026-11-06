#!/usr/bin/env python3
"""Create and inspect compressed DeadSpace vandal.marker bundles.

The marker is a ZIP64 archive. Entry names are data, never instructions.
Hash/version mismatches are warnings so the launcher may continue after an
explicit user confirmation; structural/path errors are fatal.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import plistlib
import shutil
import stat
import sys
import tempfile
import uuid
import zipfile


MARKER_NAME = "vandal.marker"
MANIFEST_NAME = "vandal.json"
SCHEMA = 1
KIND = "deadspace-vandal-marker"
GAME_BUNDLE_ID = "com.ea.deadspace.bv"
TARGET_GAME_VERSION = "1.3.34"
SUPPORTED_TEXTURE_TIERS = ("2", "4", "6", "8")


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sha256_stream(stream) -> str:
    digest = hashlib.sha256()
    for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
        digest.update(block)
    return digest.hexdigest()


def entry_matches_prefixes(name: str, prefixes: set[str]) -> bool:
    return any(
        name == prefix.rstrip("/") or name.startswith(prefix.rstrip("/") + "/")
        for prefix in prefixes
    )


def zipinfo_is_symlink(info: zipfile.ZipInfo) -> bool:
    return stat.S_ISLNK(info.external_attr >> 16)


def normalized_name(name: str) -> str:
    value = name.replace("\\", "/")
    path = pathlib.PurePosixPath(value)
    if not value or value.startswith("/") or ".." in path.parts or "\x00" in value:
        raise ValueError(f"Unsafe marker entry name: {name!r}")
    return "/".join(part for part in path.parts if part not in ("", "."))


def role_for(name: str) -> str:
    lower = name.lower()
    if lower.endswith(".ipa"):
        return "ipa"
    if lower.startswith("payload/texture-packs/"):
        return "texture"
    raise ValueError(f"Marker contains data outside IPA/texture packs: {name}")


def platform_for(name: str) -> str:
    lower = name.lower()
    if lower.startswith("payload/texture-packs/"):
        return "all"
    return "all"


def regular_files(root: pathlib.Path) -> list[pathlib.Path]:
    root = root.resolve()
    result: list[pathlib.Path] = []
    for current, dirs, files in os.walk(root, followlinks=False):
        base = pathlib.Path(current)
        for name in list(dirs):
            path = base / name
            if path.is_symlink():
                raise ValueError(f"Marker input contains a symlink: {path}")
        for name in files:
            path = base / name
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode) or not stat.S_ISREG(mode):
                raise ValueError(f"Marker input contains a non-regular file: {path}")
            result.append(path)
    return sorted(result, key=lambda path: path.relative_to(root).as_posix())


def build_manifest(
    root: pathlib.Path,
    version: str,
    product: str,
    ipa_name: str | None,
    target_game_version: str = TARGET_GAME_VERSION,
) -> tuple[dict, list[pathlib.Path]]:
    files = regular_files(root)
    if not files:
        raise ValueError("Marker input is empty")
    entries = []
    ipa_candidates = []
    for path in files:
        relative = path.relative_to(root).as_posix()
        entry_name = normalized_name(relative)
        digest = sha256_file(path)
        entry = {
            "path": entry_name,
            "size": path.stat().st_size,
            "sha256": digest,
            "role": role_for(entry_name),
            "platform": platform_for(entry_name),
        }
        entries.append(entry)
        if entry["role"] == "ipa":
            ipa_candidates.append(entry)
    texture_tiers = set()
    for entry in entries:
        if entry["role"] != "texture":
            continue
        parts = pathlib.PurePosixPath(entry["path"]).parts
        if len(parts) < 4 or parts[:2] != ("payload", "texture-packs"):
            raise ValueError(f"Invalid texture pack path: {entry['path']}")
        tier = parts[2]
        if tier not in SUPPORTED_TEXTURE_TIERS:
            raise ValueError(f"Unsupported texture tier {tier}: {entry['path']}")
        texture_tiers.add(tier)
    if not texture_tiers:
        raise ValueError("Marker must contain at least one texture tier")
    if ipa_name:
        wanted = normalized_name(ipa_name)
        ipa_matches = [entry for entry in entries if entry["path"] == wanted]
        if len(ipa_matches) != 1:
            raise ValueError(f"Requested IPA entry is missing: {wanted}")
        ipa_entry = ipa_matches[0]
    elif len(ipa_candidates) == 1:
        ipa_entry = ipa_candidates[0]
    else:
        raise ValueError(
            "Marker input must contain exactly one .ipa, or --ipa-relative must select one"
        )
    manifest = {
        "schema": SCHEMA,
        "kind": KIND,
        "product": product,
        "marker_version": version,
        "created_utc": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "minimum_launcher": {"android": 1, "pc": 1},
        "game": {
            "bundle_id": GAME_BUNDLE_ID,
            "target_version": target_game_version,
            "compatibility_notice": (
                "DeadSpace Legacy is developed against Dead Space iPhone "
                + target_game_version
                + "; other IPA versions may run with compatibility warnings."
            ),
            "ipa_path": ipa_entry["path"],
            "ipa_sha256": ipa_entry["sha256"],
            "ipa_size": ipa_entry["size"],
        },
        "texture_tiers": {
            tier: sum(
                1
                for entry in entries
                if entry["path"].startswith("payload/texture-packs/" + tier + "/")
            )
            for tier in SUPPORTED_TEXTURE_TIERS
            if tier in texture_tiers
        },
        "entries": entries,
        "verification": "hash-mismatch-and-version-difference-are-warnings",
        "compression": "zip64-deflate",
    }
    canonical = json.dumps(
        manifest,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    manifest["marker_id"] = hashlib.sha256(canonical).hexdigest()
    return manifest, files


def create_marker(
    input_root: pathlib.Path,
    output: pathlib.Path,
    version: str,
    product: str,
    ipa_name: str | None = None,
    target_game_version: str = TARGET_GAME_VERSION,
) -> dict:
    if output.name.lower() != MARKER_NAME:
        raise ValueError(f"Output file name must be {MARKER_NAME}")
    if output.exists():
        raise FileExistsError(output)
    manifest, files = build_manifest(
        input_root,
        version,
        product,
        ipa_name,
        target_game_version,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".incomplete")
    if temporary.exists():
        raise FileExistsError(temporary)
    try:
        with zipfile.ZipFile(
            temporary,
            "x",
            compression=zipfile.ZIP_DEFLATED,
            compresslevel=6,
            allowZip64=True,
        ) as archive:
            archive.writestr(
                MANIFEST_NAME,
                json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
                compress_type=zipfile.ZIP_DEFLATED,
                compresslevel=6,
            )
            for path in files:
                name = normalized_name(path.relative_to(input_root).as_posix())
                archive.write(path, name, compress_type=zipfile.ZIP_DEFLATED, compresslevel=6)
        os.replace(temporary, output)
    finally:
        if temporary.exists():
            temporary.unlink()
    return manifest


def read_manifest(path: pathlib.Path) -> dict:
    with zipfile.ZipFile(path, "r") as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError("Marker has duplicate entry names")
        if MANIFEST_NAME not in names:
            raise ValueError("Marker is missing vandal.json")
        raw = archive.read(MANIFEST_NAME)
    manifest = json.loads(raw.decode("utf-8"))
    if manifest.get("schema") != SCHEMA or manifest.get("kind") != KIND:
        raise ValueError("Unsupported vandal.marker schema/kind")
    if not isinstance(manifest.get("entries"), list):
        raise ValueError("Marker manifest entries are invalid")
    seen = set()
    for entry in manifest["entries"]:
        name = normalized_name(entry.get("path", ""))
        if name in seen:
            raise ValueError(f"Duplicate manifest entry: {name}")
        seen.add(name)
        size = entry.get("size")
        digest = entry.get("sha256")
        if not isinstance(size, int) or size < 0:
            raise ValueError(f"Invalid manifest size: {name}")
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(character not in "0123456789abcdef" for character in digest.lower())
        ):
            raise ValueError(f"Invalid manifest SHA-256: {name}")
    return manifest


def verify_marker(
    path: pathlib.Path,
    expected_version: str | None = None,
    minimum_launcher: dict | None = None,
    full: bool = False,
    prefixes: set[str] | None = None,
) -> dict:
    result = {
        "marker": str(path),
        "operable": False,
        "warnings": [],
        "errors": [],
        "manifest": None,
        "hash_scope": "none",
        "hashed_entries": 0,
    }
    try:
        manifest = read_manifest(path)
        result["manifest"] = manifest
        game = manifest.get("game", {})
        target = game.get("target_version")
        if target != TARGET_GAME_VERSION:
            result["warnings"].append(
                "Marker targets game version "
                + str(target)
                + "; launcher development target is "
                + TARGET_GAME_VERSION
            )
        declared = {entry["path"]: entry for entry in manifest["entries"]}
        with zipfile.ZipFile(path, "r") as archive:
            actual_names = set(archive.namelist()) - {MANIFEST_NAME}
            listed_names = set(declared)
            for name in sorted(actual_names - listed_names):
                result["warnings"].append(f"Marker contains unlisted entry: {name}")
            for name in sorted(listed_names - actual_names):
                result["errors"].append(f"Marker entry is missing: {name}")
            for name, entry in declared.items():
                if name not in actual_names:
                    continue
                info = archive.getinfo(name)
                if info.is_dir():
                    result["errors"].append(f"Marker entry is a directory: {name}")
                    continue
                if zipinfo_is_symlink(info):
                    result["errors"].append(f"Marker entry is a symlink: {name}")
                    continue
                selected = full or (
                    prefixes is not None and entry_matches_prefixes(name, prefixes)
                )
                if not selected:
                    continue
                if info.file_size != entry.get("size"):
                    result["warnings"].append(
                        f"Size mismatch: {name} expected {entry.get('size')} got {info.file_size}"
                    )
                with archive.open(info, "r") as stream:
                    digest = sha256_stream(stream)
                result["hashed_entries"] += 1
                if digest != entry.get("sha256"):
                    result["warnings"].append(
                        f"SHA-256 mismatch: {name} expected {entry.get('sha256')} got {digest}"
                    )
            if prefixes is not None and not full:
                matched = {
                    name
                    for name in declared
                    if entry_matches_prefixes(name, prefixes)
                }
                if not matched:
                    result["errors"].append(
                        "No declared marker entries matched the requested verification scope"
                    )
                result["hash_scope"] = "selected"
            elif full:
                result["hash_scope"] = "full"
        if expected_version and manifest.get("marker_version") != expected_version:
            result["warnings"].append(
                "Marker version differs: "
                + str(manifest.get("marker_version"))
                + " vs "
                + expected_version
            )
        if minimum_launcher:
            declared_minimum = manifest.get("minimum_launcher", {})
            for platform, current in minimum_launcher.items():
                required = declared_minimum.get(platform, 0)
                if int(current) < int(required):
                    result["warnings"].append(
                        f"Launcher version for {platform} is {current}; marker requests {required}"
                    )
        result["operable"] = not result["errors"]
    except Exception as error:
        result["errors"].append(str(error))
    return result


def detect_ipa(path: pathlib.Path) -> dict:
    result = {
        "path": str(path),
        "is_dead_space_ipa": False,
        "target_version": TARGET_GAME_VERSION,
        "warnings": [],
        "errors": [],
    }
    try:
        with zipfile.ZipFile(path, "r") as archive:
            info_names = [
                name
                for name in archive.namelist()
                if name.startswith("Payload/") and name.endswith(".app/Info.plist")
            ]
            if len(info_names) != 1:
                raise ValueError("IPA must contain exactly one Payload/*.app/Info.plist")
            info = plistlib.loads(archive.read(info_names[0]))
            result["bundle_id"] = info.get("CFBundleIdentifier")
            result["bundle_version"] = info.get("CFBundleVersion")
            result["is_dead_space_ipa"] = result["bundle_id"] == GAME_BUNDLE_ID
            if not result["is_dead_space_ipa"]:
                result["errors"].append(
                    f"Unexpected bundle identifier: {result.get('bundle_id')}"
                )
            if result["bundle_version"] != TARGET_GAME_VERSION:
                result["warnings"].append(
                    "DeadSpace Legacy is developed for iPhone "
                    + TARGET_GAME_VERSION
                    + "; selected IPA version is "
                    + str(result["bundle_version"])
                    + ". Continuing may expose compatibility issues."
                )
    except Exception as error:
        result["errors"].append(str(error))
    return result


def safe_extract(
    marker: pathlib.Path,
    output: pathlib.Path,
    prefix: str = "payload/",
    include_manifest: bool = False,
) -> list[str]:
    manifest = read_manifest(marker)
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    prefix = normalized_name(prefix)
    allowed = {entry["path"] for entry in manifest["entries"]}
    extracted = []
    with zipfile.ZipFile(marker, "r") as archive:
        for info in archive.infolist():
            name = normalized_name(info.filename)
            if name == MANIFEST_NAME:
                if not include_manifest:
                    continue
            elif name not in allowed:
                continue
            elif prefix and not name.startswith(prefix):
                continue
            target = (output / pathlib.PurePosixPath(name)).resolve()
            if output != target and output not in target.parents:
                raise ValueError(f"Unsafe extraction path: {name}")
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info, "r") as source, target.open("xb") as destination:
                for block in iter(lambda: source.read(4 * 1024 * 1024), b""):
                    destination.write(block)
            extracted.append(name)
    return extracted


def install_marker(
    marker: pathlib.Path,
    output_root: pathlib.Path,
    tiers: set[str],
    allow_warnings: bool = False,
) -> dict:
    if not tiers.issubset(SUPPORTED_TEXTURE_TIERS):
        raise ValueError(
            "Texture tiers must be a subset of " + ",".join(SUPPORTED_TEXTURE_TIERS)
        )
    manifest = read_manifest(marker)
    marker_id = manifest.get("marker_id", "")
    if not isinstance(marker_id, str) or not marker_id:
        raise ValueError("Marker ID is missing")
    available_tiers = set(manifest.get("texture_tiers", {}))
    missing_tiers = tiers - available_tiers
    if missing_tiers:
        raise ValueError(
            "Marker does not contain requested texture tiers: "
            + ",".join(sorted(missing_tiers))
        )
    output_root = output_root.resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    content_root = output_root / "marker-content"
    content_root.mkdir(exist_ok=True)
    destination = content_root / marker_id
    installation_file = destination / "installation.json"
    existing = None
    if destination.exists():
        if not installation_file.is_file():
            raise ValueError("Existing marker content has no installation record")
        existing = json.loads(installation_file.read_text(encoding="utf-8"))
        if existing.get("marker_id") != marker_id:
            raise ValueError("Existing marker content record has a different marker ID")
        if not existing.get("base_data", {}).get("installed"):
            raise ValueError("Existing marker content is missing mandatory base data")
        missing_tiers = tiers - set(existing.get("texture_tiers", []))
        if not missing_tiers:
            return existing

    base_path = manifest["game"]["ipa_path"]
    prefixes = {base_path} if existing is None else set()
    prefixes.update(
        "payload/texture-packs/" + tier + "/" for tier in missing_tiers or tiers
    )
    verification = verify_marker(marker, prefixes=prefixes)
    if not verification["operable"]:
        raise ValueError("Marker is not operable: " + "; ".join(verification["errors"]))
    if verification["warnings"] and not allow_warnings:
        raise ValueError(
            "Marker has verification warnings; explicit warning confirmation is required"
        )

    def copy_entry(archive: zipfile.ZipFile, entry: dict, output: pathlib.Path) -> dict:
        name = entry["path"]
        info = archive.getinfo(name)
        target = output / pathlib.PurePosixPath(name)
        target.parent.mkdir(parents=True, exist_ok=True)
        digest = hashlib.sha256()
        with archive.open(info, "r") as source, target.open("xb") as destination_stream:
            for block in iter(lambda: source.read(4 * 1024 * 1024), b""):
                digest.update(block)
                destination_stream.write(block)
        source_digest = digest.hexdigest()
        target_digest = sha256_file(target)
        if target_digest != source_digest:
            raise ValueError(f"Copied marker entry changed while writing: {name}")
        return {
            "path": name,
            "size": target.stat().st_size,
            "sha256": target_digest,
            "manifest_sha256": entry["sha256"],
            "manifest_match": target_digest == entry["sha256"],
        }

    if existing is None:
        temporary = content_root / (".install-" + marker_id + "-" + uuid.uuid4().hex)
        temporary.mkdir()
        installed = []
        try:
            with zipfile.ZipFile(marker, "r") as archive:
                (temporary / MANIFEST_NAME).write_bytes(archive.read(MANIFEST_NAME))
                for entry in manifest["entries"]:
                    if entry["path"] == base_path or entry_matches_prefixes(
                        entry["path"], prefixes
                    ):
                        installed.append(copy_entry(archive, entry, temporary))
            base_record = next(
                record for record in installed if record["path"] == base_path
            )
            installation = {
                "schema": 2,
                "marker_id": marker_id,
                "marker_version": manifest.get("marker_version"),
                "source_marker": str(marker.resolve()),
                "updated_utc": datetime.datetime.now(datetime.timezone.utc)
                .replace(microsecond=0)
                .isoformat()
                .replace("+00:00", "Z"),
                "base_data": {"installed": True, **base_record},
                "texture_tiers": sorted(tiers),
                "files": installed,
                "total_bytes": sum(record["size"] for record in installed),
                "verification_warnings": verification["warnings"],
            }
            (temporary / "installation.json").write_text(
                json.dumps(installation, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            os.replace(temporary, destination)
        finally:
            if temporary.exists():
                shutil.rmtree(temporary)
        return installation

    records = {record["path"]: record for record in existing.get("files", [])}
    temporary = content_root / (".install-" + marker_id + "-" + uuid.uuid4().hex)
    temporary.mkdir()
    try:
        with zipfile.ZipFile(marker, "r") as archive:
            for entry in manifest["entries"]:
                if not entry_matches_prefixes(entry["path"], prefixes):
                    continue
                records[entry["path"]] = copy_entry(archive, entry, temporary)
        for tier in sorted(missing_tiers):
            source = temporary / "payload" / "texture-packs" / tier
            target = destination / "payload" / "texture-packs" / tier
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                raise ValueError(
                    f"Texture tier already exists without installation record: {tier}"
                )
            os.replace(source, target)
        installation = {
            **existing,
            "schema": 2,
            "marker_id": marker_id,
            "updated_utc": datetime.datetime.now(datetime.timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "texture_tiers": sorted(set(existing.get("texture_tiers", [])) | tiers),
            "files": [records[name] for name in sorted(records)],
            "total_bytes": sum(record["size"] for record in records.values()),
        }
        replacement = installation_file.with_name(
            installation_file.name + ".new-" + uuid.uuid4().hex
        )
        replacement.write_text(
            json.dumps(installation, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        os.replace(replacement, installation_file)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)
    return installation


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    create = commands.add_parser("create")
    create.add_argument("--input", type=pathlib.Path, required=True)
    create.add_argument("--output", type=pathlib.Path, required=True)
    create.add_argument("--version", required=True)
    create.add_argument("--product", default="DeadSpace Legacy / HyperHLE DS Compat")
    create.add_argument("--ipa-relative")
    create.add_argument("--target-game-version", default=TARGET_GAME_VERSION)

    verify = commands.add_parser("verify")
    verify.add_argument("marker", type=pathlib.Path)
    verify.add_argument("--expected-version")
    verify.add_argument("--launcher-android", type=int)
    verify.add_argument("--launcher-pc", type=int)
    verify.add_argument("--full", action="store_true")
    verify.add_argument(
        "--tier",
        action="append",
        choices=SUPPORTED_TEXTURE_TIERS,
    )
    verify.add_argument("--base-data", action="store_true")

    extract = commands.add_parser("extract")
    extract.add_argument("marker", type=pathlib.Path)
    extract.add_argument("--output", type=pathlib.Path, required=True)
    extract.add_argument("--prefix", default="payload/")
    extract.add_argument("--include-manifest", action="store_true")

    install = commands.add_parser("install")
    install.add_argument("marker", type=pathlib.Path)
    install.add_argument("--output", type=pathlib.Path, required=True)
    install.add_argument("--tiers", default="")
    install.add_argument("--allow-warnings", action="store_true")

    ipa = commands.add_parser("inspect-ipa")
    ipa.add_argument("path", type=pathlib.Path)

    args = parser.parse_args()
    try:
        if args.command == "create":
            manifest = create_marker(
                args.input,
                args.output,
                args.version,
                args.product,
                args.ipa_relative,
                args.target_game_version,
            )
            print(
                json.dumps(
                    {
                        "output": str(args.output),
                        "bytes": args.output.stat().st_size,
                        "sha256": sha256_file(args.output),
                        "marker_id": manifest["marker_id"],
                    }
                )
            )
            return 0
        if args.command == "verify":
            minimum = {}
            if args.launcher_android is not None:
                minimum["android"] = args.launcher_android
            if args.launcher_pc is not None:
                minimum["pc"] = args.launcher_pc
            prefixes = {
                "payload/texture-packs/" + tier + "/"
                for tier in (args.tier or [])
            }
            if args.base_data:
                manifest = read_manifest(args.marker)
                prefixes.add(manifest["game"]["ipa_path"])
            result = verify_marker(
                args.marker,
                args.expected_version,
                minimum,
                full=args.full,
                prefixes=prefixes or None,
            )
            print(json.dumps(result, ensure_ascii=False, indent=2))
            return 0 if result["operable"] else 2
        if args.command == "extract":
            names = safe_extract(
                args.marker,
                args.output,
                args.prefix,
                args.include_manifest,
            )
            print(json.dumps({"output": str(args.output), "entries": len(names)}))
            return 0
        if args.command == "install":
            tiers = {item.strip() for item in args.tiers.split(",") if item.strip()}
            result = install_marker(
                args.marker,
                args.output,
                tiers,
                args.allow_warnings,
            )
            print(
                json.dumps(
                    {
                        "marker_id": result["marker_id"],
                        "texture_tiers": result["texture_tiers"],
                        "base_data_installed": result["base_data"]["installed"],
                        "files": len(result["files"]),
                        "bytes": result["total_bytes"],
                        "path": str(
                            args.output.resolve()
                            / "marker-content"
                            / result["marker_id"]
                        ),
                    }
                )
            )
            return 0
        result = detect_ipa(args.path)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0 if result["is_dead_space_ipa"] else 2
    except Exception as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
