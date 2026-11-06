"""Verified whole-Documents snapshots and stopped-game profile switching.

This targets PC directories. Android process state must be handled by an Android
adapter; a local process check is never evidence that an Android game stopped.
"""
from __future__ import annotations

from contextlib import contextmanager
from datetime import datetime, timezone
import csv
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import uuid

from sbin import Document, SaveError, digest, require

MAX_FILES = 10000
MAX_TOTAL = 256 * 1024 * 1024
MAX_FILE = 64 * 1024 * 1024
FORMAT = "deadspace.cold-profile.v1"


def require_stopped():
    if os.name == "nt":
        try:
            completed = subprocess.run(["tasklist", "/FO", "CSV", "/NH"], capture_output=True,
                                       check=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
            names = [row[0] for row in csv.reader(io.StringIO(completed.stdout.decode("mbcs"))) if row]
        except (OSError, subprocess.SubprocessError, UnicodeError) as error:
            raise SaveError("cannot verify game process state") from error
    else:
        try:
            completed = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True,
                                       text=True, check=True, timeout=15)
            names = completed.stdout.splitlines()
        except (OSError, subprocess.SubprocessError) as error:
            raise SaveError("cannot verify game process state") from error
    require(not any("touchhle" in Path(name.strip()).name.lower() for name in names),
            "stop all touchHLE processes before snapshot or activation")


def safe_path(path):
    path = Path(os.path.abspath(path))
    # Check every ancestor before resolving: resolve() alone would hide links.
    for part in [path, *path.parents]:
        if part.exists() or part.is_symlink():
            info = part.lstat()
            reparse = bool(getattr(info, "st_file_attributes", 0) & 0x400)
            require(not stat.S_ISLNK(info.st_mode) and not reparse, f"link/reparse point rejected: {part}")
    return path


def disjoint(left, right):
    require(left != right and left not in right.parents and right not in left.parents,
            "profile, bank and destination paths must not overlap")


def relative_name(name):
    require(isinstance(name, str) and name and "\\" not in name and ":" not in name, "unsafe manifest path")
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(part not in (".", "..", "") for part in name.split("/")), "unsafe manifest path")
    for part in path.parts:
        require(not part.endswith((".", " ")) and not any(ch in part for ch in '<>"|?*\0'), "unsafe filename")
        require(not re.fullmatch(r"(?i)(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?", part), "reserved filename")
    return path


def inventory(root, validate_sbin=True):
    root = safe_path(root)
    require(root.is_dir(), "Documents directory missing")
    result, directories, seen = {}, [], set()
    total = 0
    for current, dirnames, filenames in os.walk(root, followlinks=False):
        for name in sorted(dirnames + filenames):
            path = safe_path(Path(current) / name)
            relative = path.relative_to(root).as_posix()
            relative_name(relative)
            require(relative.casefold() not in seen, "case-colliding profile paths")
            seen.add(relative.casefold())
            require(len(seen) <= MAX_FILES, "too many profile entries")
            if path.is_dir():
                directories.append(relative)
                continue
            info = path.stat()
            require(stat.S_ISREG(info.st_mode) and info.st_size <= MAX_FILE, "unsupported or oversized profile file")
            total += info.st_size
            require(total <= MAX_TOTAL, "profile exceeds 256 MiB")
            content = path.read_bytes()
            require(len(content) == info.st_size, "profile changed during read")
            if validate_sbin and path.suffix.lower() == ".sb":
                Document(content)
            result[relative] = {"sha256": digest(content), "bytes": len(content)}
    require(result, "empty profile")
    return {"files": dict(sorted(result.items())), "directories": sorted(directories)}


def json_write(path, value):
    with Path(path).open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2, allow_nan=False)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def copy_inventory(source, target, listing):
    target.mkdir()
    for name in listing["directories"]:
        (target / str(relative_name(name))).mkdir(parents=True, exist_ok=True)
    for name, expected in listing["files"].items():
        src = safe_path(source / str(relative_name(name)))
        destination = target / str(relative_name(name))
        destination.parent.mkdir(parents=True, exist_ok=True)
        content = src.read_bytes()
        require(len(content) == expected["bytes"] and digest(content) == expected["sha256"], "source changed during copy")
        with destination.open("xb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
    require(inventory(target) == listing, "copied profile verification failed")


def clean_private(path, parent):
    """Delete only a generated staging/rollback directory under its checked parent."""
    path, parent = safe_path(path), safe_path(parent)
    require(path.parent == parent and path.name.startswith(".save-lab-"), "cleanup escaped its generated directory")
    # Reject links inside the generated tree before recursive removal as well.
    if path.exists():
        for current, dirs, files in os.walk(path):
            for name in dirs + files:
                safe_path(Path(current) / name)
        shutil.rmtree(path)


@contextmanager
def exclusive_lock(parent, name):
    path = safe_path(parent / name)
    try:
        descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError as error:
        raise SaveError(f"another operation or interrupted-operation lock exists: {path}") from error
    try:
        with os.fdopen(descriptor, "w", encoding="ascii") as handle:
            handle.write(f"pid={os.getpid()}\n")
        yield
    finally:
        path.unlink()


def snapshot(documents, bank, name, *, check_local_process=True):
    if check_local_process:
        require_stopped()
    require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}", name) is not None, "profile name: 1-80 ASCII letters/digits/dot/dash/underscore")
    source, bank = safe_path(documents), safe_path(bank)
    disjoint(source, bank)
    bank.mkdir(parents=True, exist_ok=True)
    target = bank / name
    with exclusive_lock(bank, ".save-lab-bank.lock"):
        require(not target.exists(), "profile already exists; choose a new name")
        stage = bank / (".save-lab-stage-" + uuid.uuid4().hex)
        stage.mkdir()
        try:
            listing = inventory(source)
            copy_inventory(source, stage / "Documents", listing)
            require(inventory(source) == listing, "source changed during snapshot")
            manifest = {"format": FORMAT, "created_utc": datetime.now(timezone.utc).isoformat(), **listing}
            json_write(stage / "manifest.json", manifest)
            verify(stage)
            if check_local_process:
                require_stopped()
            os.rename(stage, target)
        finally:
            if stage.exists():
                clean_private(stage, bank)
    return target


def verify(entry):
    entry = safe_path(entry)
    manifest_path = safe_path(entry / "manifest.json")
    require(manifest_path.stat().st_size <= 4 * 1024 * 1024, "oversized profile manifest")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    require(manifest.get("format") == FORMAT, "unsupported profile manifest")
    files, directories = manifest["files"], manifest["directories"]
    require(isinstance(files, dict) and isinstance(directories, list) and len(files) + len(directories) <= MAX_FILES,
            "invalid manifest entries")
    for name in [*files, *directories]:
        relative_name(name)
    expected = {"files": files, "directories": directories}
    require(inventory(entry / "Documents") == expected, "profile hash/member verification failed")
    return expected


def activate(entry, documents, backup_bank):
    require_stopped()
    entry, destination, backup_bank = safe_path(entry), safe_path(documents), safe_path(backup_bank)
    require(destination.name == "Documents", "activation destination must be explicitly named Documents")
    disjoint(entry, destination)
    disjoint(backup_bank, destination)
    disjoint(entry, backup_bank)
    require(not destination.exists() or destination.is_dir(), "destination is not a directory")
    destination.parent.mkdir(parents=True, exist_ok=True)
    token = uuid.uuid4().hex
    stage = destination.parent / (".save-lab-stage-" + token)
    rollback = destination.parent / (".save-lab-rollback-" + token)
    backup = None
    with exclusive_lock(destination.parent, ".save-lab-Documents.lock"):
        listing = verify(entry)
        before = inventory(destination) if destination.exists() else None
        try:
            copy_inventory(entry / "Documents", stage, listing)
            if before is not None:
                backup = snapshot(destination, backup_bank, "pre-" + token)
                require(verify(backup) == before, "destination changed during backup")
            require_stopped()
            if before is not None:
                require(inventory(destination) == before, "destination changed before activation")
                os.rename(destination, rollback)
            try:
                os.rename(stage, destination)
                require(inventory(destination) == listing, "installed profile verification failed")
            except BaseException:
                # Retain failed installed content for diagnosis, restore original tree.
                if destination.exists():
                    os.rename(destination, destination.parent / (".save-lab-failed-" + token))
                if rollback.exists():
                    os.rename(rollback, destination)
                raise
            if rollback.exists():
                clean_private(rollback, destination.parent)
        finally:
            if stage.exists():
                clean_private(stage, destination.parent)
    return {"destination": str(destination), "backup": str(backup) if backup else None,
            "files": len(listing["files"])}
