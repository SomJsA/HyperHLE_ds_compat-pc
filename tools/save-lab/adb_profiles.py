"""ADB transport for verified cold Documents swaps in an isolated Save Lab app."""
from __future__ import annotations

import io
from pathlib import Path
import re
import shlex
import subprocess
import tarfile
import tempfile
import uuid

import cold_profiles as profiles
from sbin import SaveError, require


class AdbProfiles:
    def __init__(self, adb, serial, package="org.touchhle.android.savelab"):
        require(bool(serial) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z][A-Za-z0-9_]*)+", package), "explicit serial and valid package required")
        self.adb, self.serial, self.package = str(adb), serial, package
        self.external = f"/sdcard/Android/data/{package}/files"
        self.internal = package == "org.touchhle.android.savelab"
        self.base = f"/data/user/0/{package}/files/save-lab" if self.internal else self.external
        self.root = self.base + "/touchHLE_sandbox/com.ea.deadspace.bv"
        self.documents = self.root + "/Documents"

    def call(self, *args, input=None, timeout=120, check=True):
        try:
            result = subprocess.run([self.adb, "-s", self.serial, *map(str, args)], input=input,
                                    capture_output=True, timeout=timeout)
        except (subprocess.SubprocessError, OSError) as error:
            raise SaveError("ADB transport failed; retain remote rollback/staging trees: " + str(error)) from error
        if check and result.returncode:
            raise SaveError("ADB failed: " + result.stderr.decode("utf-8", "replace")[-1500:]
                            + result.stdout.decode("utf-8", "replace")[-500:])
        return result

    def shell(self, script, check=True):
        # Keep test saves inside app-owned internal storage. run-as preserves
        # the app UID; shell-owned external directories fail on scoped ROMs.
        command = shlex.join(["run-as", self.package, "sh", "-c", script]) if self.internal else script
        return self.call("shell", command, check=check)

    def require_stopped(self):
        names = self.call("shell", "ps -A -o NAME").stdout.decode("utf-8", "replace").splitlines()
        require(not any(name.strip() in (self.package, self.package + ":game") for name in names),
                "isolated app must be fully stopped before a cold operation")

    def stop(self):
        require(self.package == "org.touchhle.android.savelab", "mutations are restricted to org.touchhle.android.savelab")
        self.call("shell", "am", "force-stop", self.package)
        self.require_stopped()

    def _extract(self, destination):
        prefix = ("run-as", self.package) if self.internal else ()
        content = self.call("exec-out", *prefix, "tar", "-C", self.documents, "-cf", "-", ".").stdout
        require(len(content) <= profiles.MAX_TOTAL + 16 * 1024 * 1024, "remote tar exceeds limit")
        try:
            extract_tar(content, destination)
        except tarfile.TarError as error:
            raise SaveError("remote Documents archive could not be read") from error
        return profiles.inventory(destination)

    def pull(self, bank, name):
        # Reading a different package is allowed; it is never stopped or written.
        if self.package == "org.touchhle.android.savelab":
            self.require_stopped()
        else:
            names = self.call("shell", "ps -A -o NAME").stdout.decode("utf-8", "replace").splitlines()
            require(self.package + ":game" not in {n.strip() for n in names}, "source game is running")
        with tempfile.TemporaryDirectory(prefix="save-lab-adb-") as temporary:
            first, second = Path(temporary) / "first", Path(temporary) / "second"
            listing = self._extract(first)
            require(self._extract(second) == listing, "remote source changed while reading")
            # The source is an immutable transport copy; remote process checks
            # apply independently of unrelated PC game processes.
            return profiles.snapshot(first, bank, name, check_local_process=False)

    def activate(self, entry, backup_bank):
        listing = profiles.verify(entry)
        self.stop()
        token = uuid.uuid4().hex
        marker = self.base + "/.save-lab-adb-lock"
        stage = self.root + "/.save-lab-stage-" + token
        previous = self.root + "/.save-lab-rollback-" + token
        failed = self.root + "/.save-lab-failed-" + token
        upload = self.base + "/.save-lab-upload-" + token + ".tar"
        backup = None
        moved = False
        self.shell(f"set -e; mkdir -p {shlex.quote(self.root)}; mkdir {shlex.quote(marker)}")
        try:
            exists = self.shell(f"test -d {shlex.quote(self.documents)}", check=False).returncode == 0
            if exists:
                backup = self.pull(backup_bank, "adb-pre-" + token)
            with tempfile.TemporaryDirectory(prefix="save-lab-upload-") as temporary:
                archive = Path(temporary) / "Documents.tar"
                create_tar(Path(entry) / "Documents", archive)
                self.shell(f"mkdir {shlex.quote(stage)}")
                # Toybox tar on some ROMs seeks while parsing PAX headers, so
                # receive a regular app-owned file before extracting it.
                command = shlex.join(["run-as", self.package, "sh", "-c", "cat > " + shlex.quote(upload)])
                self.call("exec-in", command, input=archive.read_bytes())
                self.shell(f"tar -C {shlex.quote(stage)} -xof {shlex.quote(upload)}")
            # Verify bytes on-device before touching the active directory.
            original_path = self.documents
            try:
                self.documents = stage
                with tempfile.TemporaryDirectory(prefix="save-lab-stage-check-") as temporary:
                    require(self._extract(Path(temporary) / "Documents") == listing, "uploaded profile verification failed")
            finally:
                self.documents = original_path
            self.require_stopped()
            if exists:
                with tempfile.TemporaryDirectory(prefix="save-lab-before-check-") as temporary:
                    require(self._extract(Path(temporary) / "Documents") == profiles.verify(backup), "active profile changed before switch")
                self.shell(f"mv {shlex.quote(self.documents)} {shlex.quote(previous)}")
                moved = True
            self.shell(f"mv {shlex.quote(stage)} {shlex.quote(self.documents)}")
            with tempfile.TemporaryDirectory(prefix="save-lab-after-check-") as temporary:
                require(self._extract(Path(temporary) / "Documents") == listing, "activated profile verification failed")
            result = {"serial": self.serial, "package": self.package, "backup": str(backup) if backup else None,
                      "remote_rollback": previous if moved else None, "files": len(listing["files"])}
            return result
        except BaseException:
            if moved:
                self.shell(f"if test -d {shlex.quote(self.documents)}; then mv {shlex.quote(self.documents)} {shlex.quote(failed)}; fi; "
                           f"mv {shlex.quote(previous)} {shlex.quote(self.documents)}")
            raise
        finally:
            # Original/failed/staging trees are intentionally retained, never recursively deleted over ADB.
            self.shell(f"rm -f {shlex.quote(upload)}", check=False)
            self.shell(f"rmdir {shlex.quote(marker)}", check=False)


def extract_tar(content, destination):
    destination = profiles.safe_path(destination)
    destination.mkdir()
    seen, total = set(), 0
    with tarfile.open(fileobj=io.BytesIO(content), mode="r:") as archive:
        for item in archive:
            name = item.name
            while name.startswith("./"):
                name = name[2:]
            if name in ("", "."):
                continue
            name = name.rstrip("/") if item.isdir() else name
            relative = profiles.relative_name(name)
            require(name.casefold() not in seen and len(seen) < profiles.MAX_FILES, "duplicate/excessive tar members")
            seen.add(name.casefold())
            require(item.isdir() or item.isfile(), "tar links and special entries rejected")
            target = destination / str(relative)
            if item.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            total += item.size
            require(item.size <= profiles.MAX_FILE and total <= profiles.MAX_TOTAL, "oversized tar contents")
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.extractfile(item) as source, target.open("xb") as output:
                output.write(source.read(profiles.MAX_FILE + 1))


def create_tar(documents, output):
    listing = profiles.inventory(documents)
    with tarfile.open(output, "x:") as archive:
        for directory in listing["directories"]:
            archive.add(documents / directory, arcname=directory, recursive=False)
        for name in listing["files"]:
            archive.add(documents / name, arcname=name, recursive=False)
    require(profiles.inventory(documents) == listing, "local profile changed during upload packing")
