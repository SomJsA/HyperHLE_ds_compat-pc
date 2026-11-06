"""Real-device ZIP/dependency/process-guard checks for the isolated lab APK."""
import argparse
import io
import json
from pathlib import Path
import shlex
import time
import zipfile

from adb_profiles import AdbProfiles
import cold_profiles
from sbin import SaveError, require


def run(client, entry, output):
    output.mkdir()
    original = cold_profiles.verify(entry)
    allowed = {name: value for name, value in original["files"].items()
               if name.lower().endswith(".sb") or name in ("var/last_version.txt", "loggedIn", "mtxprods", ".dat1fda.006")}
    report = {"package": client.package, "serial": client.serial, "source_files": len(original["files"]),
              "supported_zip_files": len(allowed), "filtered_files": sorted(set(original["files"]) - set(allowed))}
    fixture, result_path = client.base + "/import-test.zip", client.base + "/import-test-result.json"

    def probe(corrupt=False):
        body = io.BytesIO()
        damaged = False
        with zipfile.ZipFile(body, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name in allowed:
                data = (entry / "Documents" / name).read_bytes()
                if corrupt and not damaged and name.lower().endswith(".sb"):
                    # Change a payload byte without updating FNV-1.
                    data = data[:21] + bytes((data[21] ^ 1,)) + data[22:]
                    damaged = True
                archive.writestr("Documents/" + name, data)
        command = shlex.join(["run-as", client.package, "sh", "-c", "cat > " + shlex.quote(fixture)])
        client.call("exec-in", command, input=body.getvalue())
        client.shell("rm -f " + shlex.quote(result_path))
        client.call("shell", "am", "start", "--activity-clear-top", "-n",
                    client.package + "/org.touchhle.android.LauncherActivity", "--ez", "save_lab_import", "true")
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            raw = client.call("exec-out", "run-as", client.package, "cat", result_path, check=False).stdout
            try:
                return json.loads(raw)
            except (ValueError, UnicodeError):
                time.sleep(0.5)
        raise SaveError("native ZIP import probe timed out")

    client.stop()
    try:
        report["valid"] = probe()
        require(report["valid"]["status"] == "ok", "valid native import rejected")
        client.stop()
        good = client.pull(output / "bank", "after-valid")
        expected = {"files": allowed, "directories": original["directories"]}
        require(cold_profiles.verify(good) == expected, "native import changed known dependencies or SBIN contents")
        report["corrupt"] = probe(corrupt=True)
        require(report["corrupt"]["status"] == "error", "corrupt SBIN was accepted")
        client.stop()
        rejected = client.pull(output / "bank", "after-corrupt")
        require(cold_profiles.verify(rejected) == expected, "failed import changed active Documents")
        marker = client.base + "/.save-lab-adb-lock"
        client.shell("mkdir " + shlex.quote(marker))
        try:
            report["operation_lock"] = probe()
            require(report["operation_lock"]["status"] == "error", "operation lock did not block import")
            client.stop()
            client.call("shell", "am", "start", "--activity-clear-top", "-n",
                        client.package + "/org.touchhle.android.LauncherActivity", "--ez", "autorun", "true")
            time.sleep(1)
            require(not client.call("shell", "pidof", client.package + ":game", check=False).stdout.strip(), "operation lock did not block game start")
            report["game_start_blocked_by_lock"] = True
            client.stop()
        finally:
            client.shell("rmdir " + shlex.quote(marker))
        client.call("shell", "am", "start", "--activity-clear-top", "-n",
                    client.package + "/org.touchhle.android.LauncherActivity", "--ez", "autorun", "true",
                    "--es", "ipa_path", client.external + "/touchHLE_apps/DeadSpace.ipa", "--ei", "fps_cap", "30")
        deadline = time.monotonic() + 15
        while not client.call("shell", "pidof", client.package + ":game", check=False).stdout.strip():
            require(time.monotonic() < deadline, "could not start isolated game for guard test")
            time.sleep(0.5)
        report["running_game"] = probe()
        require(report["running_game"]["status"] == "error" and "Stop the game" in report["running_game"].get("error", ""),
                "running-game guard did not reject import")
        report["passed"] = True
    finally:
        client.stop()
        (output / "verification.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", required=True)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--entry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(run(AdbProfiles(args.adb, args.serial), args.entry, args.out), ensure_ascii=False))
