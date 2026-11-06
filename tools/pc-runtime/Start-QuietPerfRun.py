#!/usr/bin/env python3
"""Run one hidden PC performance arm driven only by the control inbox."""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


def is_link_or_reparse(path: Path) -> bool:
    """Include Windows junctions and dangling links; never follow them to clean."""
    info = path.lstat()
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, "st_file_attributes", 0)
        & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
    )


def copy_private_seed(seed: Path, out: Path) -> Path:
    """Copy saves to a fresh private directory without deleting seed diagnostics.

    Ignore only the seed's root diagnostics. Following a junction could copy an
    unrelated project or expose its files to later runtime writes, so fail closed.
    No recursive deletion is needed, even when the seed contains old logs.
    """
    if out.absolute() != out.resolve() or is_link_or_reparse(out):
        raise ValueError(f"output root must be a real private directory: {out}")
    if is_link_or_reparse(seed):
        raise ValueError(f"seed must not be a link or junction: {seed}")
    sandbox = out / "touchHLE_sandbox"
    if sandbox.exists() or sandbox.is_symlink():
        raise ValueError(f"private sandbox already exists: {sandbox}")

    def ignore(directory: str, names: list[str]) -> list[str]:
        parent = Path(directory)
        excluded = [name for name in names if parent == seed and name.casefold() == "diagnostics"]
        for name in names:
            if name not in excluded and is_link_or_reparse(parent / name):
                raise ValueError(f"seed contains a link or junction: {parent / name}")
        return excluded

    shutil.copytree(seed, sandbox, ignore=ignore)
    (sandbox / "diagnostics").mkdir()
    return sandbox


def private_file(path: Path, out: Path) -> Path:
    """Validate the direct private output before replacing or unlinking a file."""
    if (out.absolute() != out.resolve() or is_link_or_reparse(out)
            or path.parent != out or path.absolute() != path.resolve()):
        raise ValueError(f"file must remain directly inside the private output: {path}")
    if path.exists() or path.is_symlink():
        if is_link_or_reparse(path) or not path.is_file():
            raise ValueError(f"private output must be a regular file: {path}")
    return path


def report_process(command: list[str]) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ, PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    return subprocess.run(command, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", env=env, timeout=60,
                          creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0)


def request_native_close(process: subprocess.Popen[bytes]) -> list[int]:
    """Post WM_CLOSE only to this still-owned child's windows; never focus them."""
    if sys.platform != "win32":
        raise RuntimeError("native quiet-runner close requires Windows")
    import ctypes as c
    user = c.WinDLL("user32", use_last_error=True)
    callback_type = c.WINFUNCTYPE(c.c_int, c.c_void_p, c.c_void_p)
    user.EnumWindows.argtypes = [callback_type, c.c_void_p]
    user.EnumWindows.restype = c.c_int
    user.GetWindowThreadProcessId.argtypes = [c.c_void_p, c.POINTER(c.c_ulong)]
    user.GetWindowThreadProcessId.restype = c.c_ulong
    user.PostMessageW.argtypes = [c.c_void_p, c.c_uint, c.c_void_p, c.c_void_p]
    user.PostMessageW.restype = c.c_int
    posted = []

    @callback_type
    def visit(hwnd, _):
        pid = c.c_ulong()
        user.GetWindowThreadProcessId(hwnd, c.byref(pid))
        # The retained Popen handle also prevents this PID from being reused.
        if pid.value == process.pid and process.poll() is None:
            if user.PostMessageW(hwnd, 0x10, None, None):
                posted.append(int(hwnd))
        return 1

    if not user.EnumWindows(visit, None):
        raise c.WinError(c.get_last_error())
    if not posted:
        raise RuntimeError("no owned window accepted WM_CLOSE")
    return posted


def finish_owned_process(process: subprocess.Popen[bytes], timeout: float = 45.0) -> dict:
    """Keep native exit evidence separate from bounded forced failure cleanup."""
    result = {"normal_close_pass": False, "forced_cleanup": False,
              "pid": process.pid, "exit_code": process.poll()}
    if result["exit_code"] is not None:
        result["method"] = "already_exited"
        return result
    start = time.monotonic()
    result["method"] = "owned_windows_WM_CLOSE"
    try:
        result["posted_windows"] = request_native_close(process)
        process.wait(timeout=timeout)
        result["normal_close_seconds"] = time.monotonic() - start
        result["normal_close_pass"] = process.returncode == 0
    except Exception as error:
        result["error"] = repr(error)
    finally:
        if process.poll() is None:
            result["forced_cleanup"] = True
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                result["kill_required"] = True
                process.kill()
                process.wait(timeout=8)
        result["exit_code"] = process.returncode
    return result


def summarize_run(out: Path, monitor: Path, manifest: dict) -> int:
    """Keep aggregate diagnostics, but use complete pairs for a paired verdict."""
    summary_script = Path(__file__).with_name("Summarize-PerfRun.py")

    def summarize(bounds: dict, destination: Path) -> subprocess.CompletedProcess[str]:
        result = report_process([
            sys.executable, str(summary_script), str(monitor),
            "--since-unix-ms", str(bounds["since_unix_ms"]),
            "--until-unix-ms", str(bounds["until_unix_ms"]),
            "--out-dir", str(destination),
        ])
        if result.returncode not in (0, 2) or not (destination / "summary.json").is_file():
            raise RuntimeError(f"summarizer failed at {destination}: exit {result.returncode}; {result.stderr}")
        return result

    result = summarize({"since_unix_ms": manifest["sample_since_unix_ms"],
                        "until_unix_ms": manifest["sample_until_unix_ms"]}, out)
    (out / "summary.stdout.log").write_text(result.stdout, encoding="utf-8")
    (out / "summary.stderr.log").write_text(result.stderr, encoding="utf-8")
    segment_status = []
    for index, segment in enumerate(manifest.get("segments", [])):
        # Labels are metadata, not arbitrary paths read from a manifest.
        label = str(segment["label"])
        segment_out = out / "segments" / f"{index + 1:02d}"
        segment_out.mkdir(parents=True)
        segment_result = summarize(segment, segment_out)
        segment_status.append({"label": label, "exit_code": segment_result.returncode,
                               "stdout": segment_result.stdout, "stderr": segment_result.stderr})
    (out / "segments.json").write_text(
        json.dumps(segment_status, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    paired = manifest.get("paired_mode", "none") != "none"
    verdict = result
    if paired:
        verdict = report_process([sys.executable, str(Path(__file__).with_name("Summarize-PairedRun.py")), str(out)])
        (out / "paired-summary.stdout.log").write_text(verdict.stdout, encoding="utf-8")
        (out / "paired-summary.stderr.log").write_text(verdict.stderr, encoding="utf-8")
        if verdict.returncode not in (0, 2) or not (out / "paired-summary.json").is_file():
            raise RuntimeError(f"paired summarizer failed: exit {verdict.returncode}; {verdict.stderr}")
    cleanup = manifest.get("cleanup")
    final_exit = verdict.returncode
    if cleanup is not None and (not cleanup.get("normal_close_pass") or cleanup.get("forced_cleanup")):
        final_exit = 2
    (out / "report-status.json").write_text(json.dumps({
        "aggregate_exit_code": result.returncode,
        "verdict_source": "paired-summary.json" if paired else "summary.json",
        "timing_exit_code": verdict.returncode,
        "exit_code": final_exit,
        "cleanup": cleanup,
        "note": "Timing comparability and native cleanup are separate; visual/driver acceptance remains separate.",
    }, indent=2) + "\n", encoding="utf-8")
    print(verdict.stdout, end="")
    return final_exit


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest().upper()


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


def wait_for(
    path: Path,
    pattern: str,
    timeout: float,
    process: subprocess.Popen[bytes] | None = None,
) -> re.Match[str]:
    regex = re.compile(pattern)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        match = regex.search(read_text(path))
        if match:
            return match
        if process is not None and process.poll() is not None:
            raise RuntimeError(
                f"process exited before pattern {pattern!r}; exit={process.returncode}"
            )
        time.sleep(0.2)
    raise TimeoutError(f"timed out waiting for {pattern!r} in {path}")


def publish_command(temporary: Path, inbox: Path) -> None:
    """Retry only failed publication while the unpublished snapshot still exists.

    Windows readers/virus scanners may briefly deny the atomic rename. A
    successful publication is never repeated, and acknowledgement timeouts are
    handled separately without replaying input.
    """
    deadline = time.monotonic() + 2.0
    while True:
        try:
            os.replace(temporary, inbox)
            return
        except OSError as error:
            if (getattr(error, "winerror", None) not in (5, 32, 33)
                    or not temporary.exists() or time.monotonic() >= deadline):
                raise
            time.sleep(0.05)


class ControlInbox:
    def __init__(self, inbox: Path, log: Path) -> None:
        self.inbox = inbox
        self.log = log
        self.sequence = 0

    def send(self, command: str, timeout: float = 10.0) -> str:
        if "\n" in command or "\r" in command:
            raise ValueError("control command must be one line")
        self.sequence += 1
        temporary = self.inbox.with_suffix(".tmp")
        temporary.write_text(f"{self.sequence} {command}", encoding="ascii")
        publish_command(temporary, self.inbox)
        needle = f"[DScontrol] sequence={self.sequence} queued"
        match = wait_for(self.log, re.escape(needle), timeout)
        return match.group(0)


def navigate_startup(control: ControlInbox, log: Path, process: subprocess.Popen[bytes]) -> list[dict]:
    """Use live native widget bounds; a real seed may start at the main menu.

    Do not assume page 7 (the empty-profile difficulty picker). That assumption
    hid the old doubled-sandbox bug and could never resume a seeded checkpoint.
    """
    routes = []
    seen = set()
    registration_cancelled = False
    deadline = time.monotonic() + 60.0
    pattern = r"transformed page=(\d+) bounds=(\[.*\])"
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("process exited during native startup navigation")
        text = read_text(log)
        rows = re.findall(pattern, text)
        # Dead Space 1.3.34's native registration page has touch glyphs,
        # not the regular widget bounds. Its X at landscape (60,300) was
        # visually verified; cancel once only on this observed startup page.
        pages = re.findall(r"\[DSfrontendInput\] page state=(\d+)", text)
        if not rows and pages and pages[-1] == "2" and not registration_cancelled:
            time.sleep(1.0)
            control.send("tap 60 300")
            registration_cancelled = True
            routes.append({"page": 2, "buttons": ["registration_cancel"]})
            continue
        if not rows or rows[-1] in seen:
            time.sleep(0.2)
            continue
        page, raw = rows[-1]
        seen.add((page, raw))
        buttons = dict(ast.literal_eval(raw))
        ids = set(buttons)
        if "BTN_PLAY" in ids:
            names = ["BTN_PLAY"]
        elif "BTN_PROFILE1" in ids:
            names = ["BTN_PROFILE1", "BTN_OK"]
        elif "BTN_STORY" in ids:
            names = ["BTN_STORY", "BTN_OK"]
        elif "BTN_NORMAL" in ids:
            names = ["BTN_NORMAL", "BTN_OK"]
        else:
            raise RuntimeError(f"unsupported native startup page {page}: {sorted(ids)}")
        routes.append({"page": int(page), "buttons": names})
        time.sleep(0.5)  # Native menu entry animation, outside timed samples.
        for name in names:
            x, y, width, height = buttons[name]
            control.send(f"tap {x + width / 2} {y + height / 2}")
            time.sleep(0.5)
        if "BTN_STORY" in ids or "BTN_NORMAL" in ids:
            time.sleep(2.0)
            after = re.findall(pattern, read_text(log))
            if after and after[-1] in seen:
                return routes
    raise TimeoutError("native startup navigation did not finish")


def git_identity(repo: Path) -> dict[str, str | int]:
    def git(*args: str) -> str:
        result = subprocess.run(
            ["git", "-C", str(repo), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        return result.stdout.strip() if result.returncode == 0 else ""

    status = git("status", "--short", "-uno")
    return {
        "head": git("rev-parse", "--short", "HEAD"),
        "branch": git("branch", "--show-current"),
        "tracked_changes": len(status.splitlines()) if status else 0,
    }


def environment(
    runtime: Path,
    sandbox: Path,
    inbox: Path,
    args: argparse.Namespace,
) -> dict[str, str]:
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("TOUCHHLE")
    }
    # Share correctness defaults with the visible launcher. Stripping the
    # parent environment must not silently remove a previously accepted fix.
    for line in Path(__file__).with_name("DeadSpace-VisualCompat.ini").read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            name, value = line.split("=", 1)
            env[name] = value
    env.update(
        {
            "SDL_AUDIODRIVER": "dummy",
            "TOUCHHLE_DS_BACKGROUND_TEST": "1",
            # This finite hidden child cannot answer the host modal dialog.
            # Bypass only that dialog; WM_CLOSE still runs guest lifecycle/save.
            "TOUCHHLE_DS_CONFIRM_QUIT": "0",
            "TOUCHHLE_DS_INPUT_MODE": "keyboard",
            # This variable is the data BASE, not the sandbox itself: Fs
            # appends touchHLE_sandbox/<bundle>. Passing sandbox nested it
            # twice and silently ran a fresh profile instead of the seed.
            "TOUCHHLE_USER_DATA_DIR": str(sandbox.parent),
            "TOUCHHLE_DS_FRONTEND_GUARD": "1",
            "TOUCHHLE_DS_FRONTEND_NAV": "1",
            "TOUCHHLE_DS_NATIVE_INPUTS": "1" if args.native_inputs else "0",
            "TOUCHHLE_DS_PC_DEPTH24": "1" if args.depth_bits == 24 else "0",
            "TOUCHHLE_DS_NOTICE": "0",
            "TOUCHHLE_DS_CONTROL_INBOX": str(inbox),
            "TOUCHHLE_DS_LOG_LEVEL": "normal",
            "TOUCHHLE_DS_OUTPUT_WIDTH": str(args.output_width),
            "TOUCHHLE_DS_OUTPUT_HEIGHT": str(args.output_height),
            "TOUCHHLE_DS_WINDOW_MODE": "window",
            "TOUCHHLE_DS_MSAA": str(args.msaa),
            "TOUCHHLE_DS_ANISO": str(args.aniso),
            "TOUCHHLE_DS_PRESENT_FSR": str(args.fsr),
            "TOUCHHLE_DS_TEX_UPLOAD_FINISH": args.tex_sync,
            "TOUCHHLE_DS_GPU_PRESENT_COPY": "1" if args.gpu_copy else "0",
            "TOUCHHLE_DS_SUPER_MONITOR": "1",
            "TOUCHHLE_DS_PERF_TRACE": "1",
            "TOUCHHLE_DS_RUN_ID": args.run_id,
            "TOUCHHLE_DS_KEY_BINDINGS": str(runtime / "DeadSpace-Keys.ini"),
            "TOUCHHLE_DS_GAMEPAD_BINDINGS": str(
                runtime / "DeadSpace-Gamepad.ini"
            ),
        }
    )
    # The emulator frame cap and the driver's swap interval are independent.
    # A performance run must not inherit an unknown VSync ceiling.
    if getattr(args, "vsync", "off") == "off":
        env["TOUCHHLE_DS_VSYNC"] = "0"
    if args.frontend_trace:
        env["TOUCHHLE_DS_FRONTEND_TRACE"] = "1"
    if args.legacy_alpha_mips:
        env.pop("TOUCHHLE_DS_AUTO_MIPS_HASH", None)
        env.pop("TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH", None)
    if args.skip_readback_finish:
        env["TOUCHHLE_DS_SKIP_READBACK_FINISH"] = "1"
    if args.text_overlay:
        env["TOUCHHLE_DS_CHAPTER_OVERLAY"] = "1"
        env["TOUCHHLE_DS_SUBTITLE_OVERLAY"] = "1"
        env["TOUCHHLE_DS_OBJECTIVE_OVERLAY"] = "1"
        env["TOUCHHLE_DS_TUTORIAL_OVERLAY"] = "1"
        env["TOUCHHLE_DS_QTE_OVERLAY"] = "1"
        env["TOUCHHLE_DS_GPU_TEXT_OVERLAY"] = "1" if args.gpu_text_overlay else "0"
    if args.capture:
        env["TOUCHHLE_DS_PRESENT_CAPTURE"] = str(
            Path(args.out_dir).resolve() / "final.ppm"
        )
        env["TOUCHHLE_DS_PRESENT_CAPTURE_TRIGGER"] = str(
            Path(args.out_dir).resolve() / "capture.trigger"
        )
    return env


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-dir", type=Path, required=True)
    parser.add_argument("--seed-sandbox", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--ipa", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--warmup", type=float, default=20.0)
    parser.add_argument("--sample", type=float, default=60.0)
    parser.add_argument(
        "--paired-mode",
        choices=["none", "gpu-copy", "tex-sync", "text-overlay", "fps-limit", "readback-finish"],
        default="none",
    )
    parser.add_argument("--paired-seconds", type=float, default=20.0)
    parser.add_argument("--paired-cycles", type=int, default=3)
    parser.add_argument("--scale-hack", type=int, default=4)
    parser.add_argument("--output-width", type=int, default=1920)
    parser.add_argument("--output-height", type=int, default=1080)
    parser.add_argument("--msaa", type=int, default=2)
    parser.add_argument("--aniso", type=int, default=4)
    parser.add_argument("--fsr", type=int, default=2)
    parser.add_argument("--tex-sync", choices=["1", "flush", "0"], default="1")
    parser.add_argument("--gpu-copy", action="store_true")
    parser.add_argument("--text-overlay", action="store_true")
    parser.add_argument("--gpu-text-overlay", action="store_true")
    parser.add_argument("--native-inputs", action="store_true")
    parser.add_argument("--capture-subtitle-gaps", action="store_true")
    parser.add_argument("--depth-bits", type=int, choices=[16, 24], default=24)
    parser.add_argument("--legacy-alpha-mips", action="store_true")
    parser.add_argument("--startup-capture-every", type=float, default=0.0)
    parser.add_argument("--fps-limit", choices=["off", "30", "60", "70"], default="off")
    parser.add_argument("--vsync", choices=["off", "driver-default"], default="off")
    parser.add_argument("--skip-readback-finish", action="store_true")
    parser.add_argument("--frontend-trace", action="store_true")
    parser.add_argument("--capture", action="store_true")
    parser.add_argument("--capture-segments", action="store_true")
    parser.add_argument("--startup-observe", type=float, default=0.0)
    parser.add_argument("--menu-only", action="store_true")
    parser.add_argument(
        "--git-repo",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    args = parser.parse_args()
    if args.paired_cycles < 1 or args.paired_seconds <= 0 or args.sample <= 0:
        parser.error("cycles and sample durations must be positive")
    if args.warmup < 0 or args.startup_observe < 0 or args.startup_capture_every < 0:
        parser.error("warmup and startup observation durations must not be negative")
    if args.paired_mode == "text-overlay" and not args.text_overlay:
        parser.error("text-overlay pairs require --text-overlay and visible text work")
    if args.capture_segments or args.startup_observe > 0:
        args.capture = True

    runtime = args.runtime_dir.resolve()
    seed = args.seed_sandbox.resolve()
    out = args.out_dir.resolve()
    exe = runtime / "touchHLE.exe"
    for path in (exe, seed, args.ipa):
        if not path.exists():
            parser.error(f"missing required path: {path}")
    if out.exists():
        parser.error(f"output directory already exists: {out}")
    if not seed.is_dir():
        parser.error(f"seed sandbox is not a directory: {seed}")

    out.mkdir(parents=True)
    sandbox = copy_private_seed(seed, out)
    diagnostics = sandbox / "diagnostics"
    inbox = out / "control-inbox.txt"
    inbox.write_text("", encoding="ascii")
    stdout_path = out / "stdout.log"
    stderr_path = out / "stderr.log"

    command = [
        str(exe),
        str(args.ipa.resolve()),
        "--force-composition",
        "--fast-present",
        "--landscape-right",
        f"--scale-hack={args.scale_hack}",
        f"--fps-limit={args.fps_limit}",
        "--preferred-languages=zh-Hans,zh",
        "--no-error-popup",
    ]
    child_environment = environment(runtime, sandbox, inbox, args)
    manifest: dict[str, object] = {
        "run_id": args.run_id,
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "runtime": str(runtime),
        "exe": str(exe),
        "exe_sha256": sha256_file(exe),
        "visual_profile_sha256": sha256_file(Path(__file__).with_name("DeadSpace-VisualCompat.ini")),
        "effective_visual_environment": {k:v for k,v in child_environment.items() if k.startswith("TOUCHHLE_DS_")},
        "ipa": str(args.ipa.resolve()),
        "ipa_bytes": args.ipa.stat().st_size,
        "ipa_sha256": sha256_file(args.ipa),
        "harness_sha256": {name: sha256_file(Path(__file__).with_name(name)) for name in (
            "Start-QuietPerfRun.py", "Summarize-PerfRun.py", "Summarize-PairedRun.py")},
        "seed_sandbox": str(seed),
        "user_data_base_root": child_environment["TOUCHHLE_USER_DATA_DIR"],
        "seed_active00_sha256": {str(path.relative_to(seed)): sha256_file(path)
                                 for path in seed.glob("*/Documents/var/00_*") if path.is_file()},
        "command": command,
        "config": {
            "scale_hack": args.scale_hack,
            "output": [args.output_width, args.output_height],
            "msaa": args.msaa,
            "aniso": args.aniso,
            "fsr": args.fsr,
            "tex_sync": args.tex_sync,
            "gpu_copy": args.gpu_copy,
            "skip_readback_finish": args.skip_readback_finish,
            "fast_present": True,
            "frontend_trace": args.frontend_trace,
            "text_overlay": args.text_overlay,
            "gpu_text_overlay": args.gpu_text_overlay,
            "native_inputs": args.native_inputs,
            "fps_limit": args.fps_limit,
            "vsync": args.vsync,
            "depth_bits": args.depth_bits,
            "legacy_alpha_mips": args.legacy_alpha_mips,
        },
        "git": git_identity(args.git_repo),
        "status": "starting",
    }
    (out / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=True) + "\n",
        encoding="utf-8",
    )

    creationflags = (
        subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
    )
    startupinfo = None
    if sys.platform == "win32":
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startupinfo.wShowWindow = subprocess.SW_HIDE
    process: subprocess.Popen[bytes] | None = None
    try:
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            process = subprocess.Popen(
                command,
                cwd=runtime,
                env=child_environment,
                stdout=stdout,
                stderr=stderr,
                creationflags=creationflags,
                startupinfo=startupinfo,
            )
        (out / "pid.txt").write_text(str(process.pid), encoding="ascii")
        wait_for(stderr_path, r"\[DScontrol\] ready", 60.0, process)
        if args.vsync == "off":
            wait_for(stderr_path, r"\[DSvsync\] swap interval 0 \(vsync off\)", 60.0, process)
            manifest["vsync_off_confirmed"] = True
        wait_for(
            stderr_path,
            r"\[DSfrontendWidgets\] transformed page=\d+",
            60.0,
            process,
        )
        acknowledgements = []
        segments = []
        control = ControlInbox(inbox, stderr_path)

        def capture_frame(label: str) -> str:
            # Each manual trigger is acknowledged by a new file, never by an
            # earlier capture log line. Captures happen outside timed segments.
            raw = private_file(out / "final.ppm", out)
            raw.unlink(missing_ok=True)
            private_file(out / "capture.trigger", out).write_text("1", encoding="ascii")
            deadline = time.monotonic() + 15.0
            while not raw.exists():
                if process.poll() is not None:
                    raise RuntimeError("process exited while capturing")
                if time.monotonic() >= deadline:
                    raise TimeoutError(f"capture timed out: {label}")
                time.sleep(0.1)
            from PIL import Image
            # Read only after the writer closes the complete PPM.
            while True:
                try:
                    with Image.open(raw) as frame:
                        frame.load()
                        frame.thumbnail((1568, 1568))
                        destination = private_file(out / f"{label}.jpg", out)
                        frame.save(destination, quality=90)
                    break
                except (OSError, ValueError):
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(0.1)
            private_file(raw, out).unlink()
            return destination.name

        if args.capture:
            time.sleep(1.2)  # Let the initial difficulty entry animation settle.
            manifest["initial_menu_capture"] = capture_frame("initial-menu")

        if args.menu_only:
            acknowledgements.append(control.send("ui down"))
            time.sleep(0.5)
            acknowledgements.append(control.send("ui up"))
            gameplay_ready_ms = int(time.time() * 1000)
        else:
            manifest["native_startup_route"] = navigate_startup(control, stderr_path, process)
            if args.startup_observe > 0:
                observed = set()
                startup_captures = []
                deadline = time.monotonic() + args.startup_observe
                next_capture = time.monotonic() + args.startup_capture_every
                capture_started = time.monotonic()
                subtitle_match = r"mapped=(?:true|false)" if args.capture_subtitle_gaps else "mapped=true"
                pattern = re.compile(r"^.*(?:\[DSchapter-overlay\].*visible=true|\[DSsubtitle-event\].*" + subtitle_match + r"|\[DSobjective-overlay\].*visible=true).*$", re.MULTILINE)
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError("process exited while observing startup overlays")
                    events = [match.group(0) for match in pattern.finditer(read_text(stderr_path))]
                    fresh = [event for event in events if event not in observed]
                    timed = args.startup_capture_every > 0 and time.monotonic() >= next_capture
                    if fresh or timed:
                        observed.update(fresh)
                        name = capture_frame(f"startup-{len(startup_captures) + 1:02d}")
                        startup_captures.append({"events": fresh, "capture": name, "elapsed_seconds": time.monotonic() - capture_started})
                        next_capture = time.monotonic() + args.startup_capture_every
                    time.sleep(0.1)
                manifest["startup_captures"] = startup_captures
            gameplay = False
            for _ in range(8):
                time.sleep(5.0)
                acknowledgements.append(control.send("probe"))
                try:
                    wait_for(
                        stderr_path,
                        r"\[DSgauge\].*state=0\b",
                        7.0,
                        process,
                    )
                    gameplay = True
                    break
                except TimeoutError:
                    # Loaded checkpoints stop at the game's "tap to continue"
                    # hint page without clearing a gameplay colour buffer. The
                    # probe then has no player state at all. Dismiss only that
                    # pre-game condition; never tap through an active cutscene.
                    if not re.search(r"\[DSgauge\].*state=\d+", read_text(stderr_path)):
                        acknowledgements.append(control.send("tap 240 285"))
                    continue
            if not gameplay:
                raise TimeoutError("gameplay did not reach HUD state 0")
            gameplay_ready_ms = int(time.time() * 1000)
        time.sleep(args.warmup)

        def run_segment(label: str, command: str) -> None:
            acknowledgements.append(control.send(command))
            time.sleep(0.5)
            start = int(time.time() * 1000)
            time.sleep(args.paired_seconds)
            end = int(time.time() * 1000)
            segments.append(
                {
                    "label": label,
                    "command": command,
                    "since_unix_ms": start,
                    "until_unix_ms": end,
                }
            )
            if args.capture_segments:
                segments[-1]["capture"] = capture_frame(label)

        if args.menu_only and args.paired_mode == "none":
            sample_since_ms = int(time.time() * 1000)
            time.sleep(args.sample)
            sample_until_ms = int(time.time() * 1000)
        elif args.paired_mode == "gpu-copy":
            for cycle in range(args.paired_cycles):
                pair = [
                    ("gpu-copy-off", "perf gpu-copy off"),
                    ("gpu-copy-on", "perf gpu-copy on"),
                ]
                if cycle % 2:
                    pair.reverse()
                for label, command in pair:
                    run_segment(f"{label}-c{cycle + 1}", command)
            sample_since_ms = segments[0]["since_unix_ms"]
            sample_until_ms = segments[-1]["until_unix_ms"]
        elif args.paired_mode == "readback-finish":
            for cycle in range(args.paired_cycles):
                pair = [
                    ("readback-finish-on", "perf readback-finish on"),
                    ("readback-finish-off", "perf readback-finish off"),
                ]
                if cycle % 2:
                    pair.reverse()
                for label, command in pair:
                    run_segment(f"{label}-c{cycle + 1}", command)
            sample_since_ms = segments[0]["since_unix_ms"]
            sample_until_ms = segments[-1]["until_unix_ms"]
        elif args.paired_mode == "tex-sync":
            for cycle in range(args.paired_cycles):
                treatment = "flush" if cycle % 2 == 0 else "off"
                pair = [
                    ("tex-finish", "perf tex-sync finish"),
                    (f"tex-{treatment}", f"perf tex-sync {treatment}"),
                ]
                if cycle % 2:
                    pair.reverse()
                for label, command in pair:
                    run_segment(f"{label}-c{cycle + 1}", command)
            sample_since_ms = segments[0]["since_unix_ms"]
            sample_until_ms = segments[-1]["until_unix_ms"]
        elif args.paired_mode == "text-overlay":
            for cycle in range(args.paired_cycles):
                pair = [("text-overlay-cpu", "perf text-overlay cpu"),
                        ("text-overlay-gpu", "perf text-overlay gpu")]
                if cycle % 2:
                    pair.reverse()
                for label, command in pair:
                    run_segment(f"{label}-c{cycle + 1}", command)
            sample_since_ms = segments[0]["since_unix_ms"]
            sample_until_ms = segments[-1]["until_unix_ms"]
        elif args.paired_mode == "fps-limit":
            for cycle in range(args.paired_cycles):
                pair = [("fps-limit-off", "perf fps-limit off"),
                        ("fps-limit-60", "perf fps-limit 60")]
                if cycle % 2:
                    pair.reverse()
                for label, command in pair:
                    run_segment(f"{label}-c{cycle + 1}", command)
            sample_since_ms = segments[0]["since_unix_ms"]
            sample_until_ms = segments[-1]["until_unix_ms"]
        else:
            sample_since_ms = int(time.time() * 1000)
            time.sleep(args.sample)
            sample_until_ms = int(time.time() * 1000)
        if args.capture:
            manifest["final_capture"] = capture_frame("final")

        manifest.update(
            {
                "status": "sampled",
                "pid": process.pid,
                "control_acknowledgements": acknowledgements,
                "gameplay_ready_unix_ms": gameplay_ready_ms,
                "sample_since_unix_ms": sample_since_ms,
                "sample_until_unix_ms": sample_until_ms,
                "paired_mode": args.paired_mode,
                "paired_seconds": args.paired_seconds,
                "segments": segments,
            }
        )
    except Exception as error:
        manifest.update({"status": "failed", "error": repr(error)})
        raise
    finally:
        if process is not None:
            manifest["cleanup"] = finish_owned_process(process)
        manifest["exit_code"] = process.returncode if process is not None else None
        (out / "manifest.json").write_text(
            json.dumps(manifest, indent=2, ensure_ascii=True) + "\n",
            encoding="utf-8",
        )

    monitor_files = sorted(
        out.rglob("super-monitor-*.jsonl"),
        key=lambda path: path.stat().st_mtime,
    )
    if not monitor_files:
        raise RuntimeError("no super-monitor JSONL was produced")
    monitor = monitor_files[-1]
    shutil.copy2(monitor, out / monitor.name)

    return summarize_run(out, out / monitor.name, manifest)


if __name__ == "__main__":
    raise SystemExit(main())
