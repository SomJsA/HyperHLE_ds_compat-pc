"""Load isolated story cases via ADB/control inbox and capture actual tablet frames.

Requires Pillow only for screenshot thumbnails. Does not inject host input or
move the character. Failed loads are captured and reported as failures.
"""
import argparse
import ast
import json
from pathlib import Path
import re
import shlex
import time

from adb_profiles import AdbProfiles
from sbin import SaveError, digest, require


class Run:
    def __init__(self, client, output, timeout):
        self.client, self.output, self.timeout = client, output, timeout
        self.sequence = 0
        self.inbox = client.base + "/save-lab-control.txt"
        self.log_path = client.base + "/touchHLE_log.txt"

    def log(self):
        result = self.client.call("exec-out", "run-as", self.client.package, "cat", self.log_path, check=False)
        require(len(result.stdout) < 8 * 1024 * 1024, "unexpectedly large runtime log")
        return result.stdout.decode("utf-8", "replace")

    def send(self, command):
        require("\n" not in command and "\r" not in command, "control command must be one line")
        self.sequence += 1
        temporary = self.inbox + ".pending"
        self.client.shell(f"printf %s {shlex.quote(str(self.sequence) + ' ' + command)} > {shlex.quote(temporary)}; "
                          f"mv {shlex.quote(temporary)} {shlex.quote(self.inbox)}")
        return self.sequence

    def screenshot(self, name):
        from PIL import Image
        raw = self.client.call("exec-out", "screencap", "-p").stdout
        require(raw.startswith(b"\x89PNG\r\n\x1a\n"), "device screenshot is not PNG")
        original = self.output / (name + ".png")
        original.write_bytes(raw)
        thumbnail = self.output / (name + ".jpg")
        with Image.open(original) as frame:
            frame.thumbnail((1568, 1568))
            frame.convert("RGB").save(thumbnail, quality=84)
        return {"png": str(original.resolve()), "jpg": str(thumbnail.resolve()),
                "png_sha256": digest(raw), "jpg_bytes": thumbnail.stat().st_size}

    def run(self, case, backups):
        self.output.mkdir(parents=True)
        result = {"case": case, "package": self.client.package, "serial": self.client.serial,
                  "status": "starting", "navigation": [], "input_after_gameplay": False}
        try:
            result["activation"] = self.client.activate(Path(case["entry"]), backups)
            # The app must create its log with its own UID; a shell-owned log
            # can make logger initialization abort on scoped-storage ROMs.
            self.client.shell(f": > {shlex.quote(self.inbox)}; rm -f {shlex.quote(self.log_path)}")
            self.client.call("shell", "am", "start", "-n", self.client.package + "/org.touchhle.android.LauncherActivity",
                             "--ez", "autorun", "true", "--ei", "scale_hack", "1", "--ei", "fps_cap", "30",
                             "--ez", "show_fps", "false", "--ez", "skip_readback", "true", "--ez", "gpu_present_copy", "true",
                             "--es", "ipa_path", self.client.external + "/touchHLE_apps/DeadSpace.ipa",
                             "--es", "diagnostic_env", "TOUCHHLE_DS_NOTICE=0@@TOUCHHLE_DS_AUTO_DISMISS_ALERTS=1")
            deadline = time.monotonic() + self.timeout
            seen = set()
            mode_entered, last_probe, hint_dismissed = None, 0, False
            pattern = r"transformed page=(\d+) bounds=(\[.*\])"
            while time.monotonic() < deadline:
                log = self.log()
                gauges = re.findall(r"^.*\[DSgauge\] probe.*state=\d+.*$", log, re.MULTILINE)
                if any(re.search(r"\bstate=0\b", gauge) for gauge in gauges):
                    result["status"] = "gameplay"
                    result["arrival"] = self.screenshot("arrival")
                    time.sleep(8)
                    self.send("probe")
                    time.sleep(1)
                    alive = self.client.call("shell", "pidof", self.client.package + ":game", check=False).stdout.strip()
                    require(bool(alive), "game process exited after reaching the scene")
                    require(not any(marker in self.log() for marker in ("panicked at", "Panic at", "SIGSEGV", "SIGABRT")),
                            "native crash after reaching the scene")
                    result["frame"] = self.screenshot("after-8s")
                    break
                if any(marker in log for marker in ("panicked at", "Panic at", "SIGSEGV", "SIGABRT")):
                    raise SaveError("native crash while loading this case")
                rows = re.findall(pattern, log)
                if rows and rows[-1] not in seen:
                    page, raw = rows[-1]
                    buttons = dict(ast.literal_eval(raw))
                    if "BTN_PLAY" in buttons:
                        names = ["BTN_PLAY"]
                    elif "BTN_PROFILE1" in buttons:
                        names = ["BTN_PROFILE1", "BTN_OK"]
                    elif "BTN_STORY" in buttons:
                        names = ["BTN_STORY", "BTN_OK"]
                    elif "BTN_NORMAL" in buttons:
                        names = ["BTN_NORMAL", "BTN_OK"]
                    else:
                        raise SaveError(f"unhandled menu page {page}: {sorted(buttons)}")
                    seen.add(rows[-1])
                    time.sleep(0.8)
                    for name in names:
                        x, y, width, height = buttons[name]
                        self.send(f"tap {x + width/2} {y + height/2}")
                        time.sleep(0.8)
                    result["navigation"].append({"page": int(page), "buttons": names})
                    if "BTN_STORY" in names or "BTN_NORMAL" in names:
                        mode_entered = time.monotonic()
                if mode_entered is not None and time.monotonic() - last_probe > 5:
                    self.send("probe")
                    last_probe = time.monotonic()
                if mode_entered and time.monotonic() - mode_entered > 18 and not gauges and not hint_dismissed:
                    # Existing checkpoints may wait at a pre-game tap hint.
                    self.send("tap 240 285")
                    hint_dismissed = True
                time.sleep(1)
            else:
                raise SaveError("load timeout: gameplay HUD state 0 was not observed")
        except Exception as error:
            result["status"] = "unconfirmed"
            result["error"] = str(error)
            try:
                result["frame"] = self.screenshot("failed-load")
            except Exception as screenshot_error:
                result["screenshot_error"] = str(screenshot_error)
        finally:
            try:
                log = self.log()
            except Exception as log_error:
                log = ""
                result["log_error"] = str(log_error)
            (self.output / "runtime.log").write_text(log, encoding="utf-8")
            result["gauges"] = re.findall(r"^.*\[DSgauge\].*$", log, re.MULTILINE)[-10:]
            result["queued_controls"] = re.findall(r"^.*\[DScontrol\].*$", log, re.MULTILINE)[-15:]
            try:
                self.client.stop()
            except Exception as stop_error:
                result["stop_error"] = str(stop_error)
            (self.output / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", required=True)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", action="append")
    parser.add_argument("--timeout", type=float, default=100)
    args = parser.parse_args()
    cases = json.loads(args.cases.read_text(encoding="utf-8"))["cases"]
    client = AdbProfiles(args.adb, args.serial)
    for case in cases:
        if args.case and case["id"] not in args.case:
            continue
        output = args.out / case["id"]
        require(not output.exists(), "case output already exists; choose a new run directory")
        result = Run(client, output, args.timeout).run(case, args.out / "backups")
        print(json.dumps({"case": case["id"], "status": result["status"], "error": result.get("error"),
                          "frame": result.get("frame", {}).get("jpg")}, ensure_ascii=False), flush=True)
