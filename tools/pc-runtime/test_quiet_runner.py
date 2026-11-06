"""Offline runner safety/report integration checks; never launches the emulator."""
import contextlib
import importlib.util
import io
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("quiet_runner", ROOT / "Start-QuietPerfRun.py")
RUNNER = importlib.util.module_from_spec(spec)
spec.loader.exec_module(RUNNER)
BASE = 1_700_000_000_000


class Files(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="deadspace-quiet-offline-")
        self.root = Path(self.temp.name).resolve()
        self.addCleanup(self.cleanup)

    def cleanup(self):
        # Validate the final target before TemporaryDirectory recursively cleans.
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.assertTrue(self.root.name.startswith("deadspace-quiet-offline-"))
        self.assertEqual(Path(self.temp.name).resolve(), self.root)
        self.temp.cleanup()

    def directory(self, name):
        path = self.root / name
        path.mkdir()
        return path


class SafetyTests(Files):
    def environment(self, **changes):
        values = dict(native_inputs=True, depth_bits=24, output_width=1920,
                      output_height=1080, msaa=2, aniso=4, fsr=0, tex_sync="1",
                      gpu_copy=False, run_id="offline", frontend_trace=False,
                      legacy_alpha_mips=False, skip_readback_finish=False,
                      text_overlay=False, gpu_text_overlay=False, capture=False,
                      out_dir=self.root)
        values.update(changes)
        return RUNNER.environment(self.root, self.root / "touchHLE_sandbox",
                                  self.root / "inbox.txt", SimpleNamespace(**values))

    def test_unlimited_emulator_cap_also_disables_inherited_vsync(self):
        with patch.dict(os.environ, {"TOUCHHLE_DS_VSYNC": "1"}):
            env = self.environment(fps_limit="off", vsync="off")
        self.assertEqual(env["TOUCHHLE_DS_VSYNC"], "0")

    def test_frame_cap_and_vsync_are_independent(self):
        env = self.environment(fps_limit="60", vsync="off")
        self.assertEqual(env["TOUCHHLE_DS_VSYNC"], "0")

    def test_private_child_bypasses_only_host_quit_confirmation(self):
        with patch.dict(os.environ, {"TOUCHHLE_DS_CONFIRM_QUIT": "1"}):
            env = self.environment()
            self.assertEqual(os.environ["TOUCHHLE_DS_CONFIRM_QUIT"], "1")
        self.assertEqual(env["TOUCHHLE_DS_CONFIRM_QUIT"], "0")
        self.assertNotIn("TOUCHHLE_DS_EXIT_MUTEX_HANDOFF", env)

    def test_driver_default_is_explicit_and_does_not_inherit_parent_override(self):
        with patch.dict(os.environ, {"TOUCHHLE_DS_VSYNC": "0"}):
            env = self.environment(vsync="driver-default")
        self.assertNotIn("TOUCHHLE_DS_VSYNC", env)

    def test_seed_logs_are_skipped_and_source_preserved(self):
        seed, out = self.directory("seed"), self.directory("out")
        (seed / "diagnostics").mkdir()
        source_log = seed / "diagnostics/old.log"
        source_log.write_text("keep", encoding="utf-8")
        (seed / "game").mkdir()
        (seed / "game/checkpoint").write_bytes(b"save")
        sandbox = RUNNER.copy_private_seed(seed, out)
        self.assertEqual((sandbox / "game/checkpoint").read_bytes(), b"save")
        self.assertEqual(list((sandbox / "diagnostics").iterdir()), [])
        self.assertEqual(source_log.read_text(encoding="utf-8"), "keep")

    def test_existing_sandbox_is_never_replaced(self):
        seed, out = self.directory("seed"), self.directory("out")
        sandbox = out / "touchHLE_sandbox"
        sandbox.mkdir()
        original = sandbox / "user-save"
        original.write_bytes(b"original")
        with self.assertRaises(ValueError):
            RUNNER.copy_private_seed(seed, out)
        self.assertEqual(original.read_bytes(), b"original")

    def test_private_capture_rejects_outside_and_directory(self):
        out, external = self.directory("out"), self.directory("external")
        for path in (external / "x.ppm", out / "../external/x.ppm", out):
            with self.subTest(path=path), self.assertRaises(ValueError):
                RUNNER.private_file(path, out)
        (out / "final.ppm").mkdir()
        with self.assertRaises(ValueError):
            RUNNER.private_file(out / "final.ppm", out)

    def test_private_capture_accepts_only_owned_regular_file(self):
        out = self.directory("out")
        path = RUNNER.private_file(out / "final.ppm", out)
        path.write_bytes(b"ppm")
        RUNNER.private_file(path, out).unlink()
        self.assertFalse(path.exists())

    @unittest.skipUnless(sys.platform == "win32", "Windows junction semantics")
    def test_seed_junction_rejected_without_touching_target(self):
        seed, out, external = self.directory("seed"), self.directory("out"), self.directory("external")
        marker = external / "user-save"
        marker.write_bytes(b"preserve")
        junction = seed / "game"
        subprocess.run(["cmd", "/d", "/c", "mklink", "/J", str(junction), str(external)],
                       check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            with self.assertRaises(ValueError):
                RUNNER.copy_private_seed(seed, out)
            self.assertEqual(marker.read_bytes(), b"preserve")
        finally:
            # Remove only this verified junction, with no recursion.
            self.assertTrue(RUNNER.is_link_or_reparse(junction))
            self.assertEqual(junction.resolve(), external)
            os.rmdir(junction)

    def test_dangling_capture_link_rejected(self):
        out = self.directory("out")
        link = out / "final.ppm"
        try:
            link.symlink_to(self.root / "missing.ppm")
        except OSError:
            self.skipTest("file symlink creation unavailable")
        try:
            with self.assertRaises(ValueError):
                RUNNER.private_file(link, out)
        finally:
            self.assertTrue(link.is_symlink())
            link.unlink()

    @unittest.skipUnless(sys.platform == "win32", "Windows junction semantics")
    def test_output_junction_rejected_without_touching_target(self):
        external, seed = self.directory("external"), self.directory("seed")
        junction = self.root / "out"
        subprocess.run(["cmd", "/d", "/c", "mklink", "/J", str(junction), str(external)],
                       check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            with self.assertRaises(ValueError):
                RUNNER.copy_private_seed(seed, junction)
            with self.assertRaises(ValueError):
                RUNNER.private_file(junction / "final.ppm", junction)
            self.assertEqual(list(external.iterdir()), [])
        finally:
            self.assertTrue(RUNNER.is_link_or_reparse(junction))
            self.assertEqual(junction.resolve(), external)
            os.rmdir(junction)

    def test_invalid_durations_fail_before_any_runtime_or_file_operation(self):
        for flag, value in (("--paired-cycles", "0"), ("--sample", "0"), ("--warmup", "-1")):
            with self.subTest(flag=flag):
                result = RUNNER.report_process([
                    sys.executable, str(ROOT / "Start-QuietPerfRun.py"),
                    "--runtime-dir", str(self.root / "missing-runtime"),
                    "--seed-sandbox", str(self.root / "missing-seed"),
                    "--out-dir", str(self.root / "must-not-be-created"),
                    "--ipa", str(self.root / "missing.ipa"), "--run-id", "invalid", flag, value,
                ])
                self.assertEqual(result.returncode, 2)
                self.assertNotIn("missing required path", result.stderr)
                self.assertFalse((self.root / "must-not-be-created").exists())


class NativeCleanupTests(unittest.TestCase):
    def process(self, exit_code=0, polls=(None, 0)):
        process = Mock(pid=12345, returncode=exit_code)
        process.poll.side_effect = list(polls)
        return process

    def test_native_close_waits_without_terminating(self):
        process = self.process()
        with patch.object(RUNNER, "request_native_close", return_value=[456]):
            result = RUNNER.finish_owned_process(process)
        self.assertTrue(result["normal_close_pass"])
        self.assertFalse(result["forced_cleanup"])
        process.wait.assert_called_once_with(timeout=45.0)
        process.terminate.assert_not_called()
        process.kill.assert_not_called()

    def test_close_timeout_cannot_be_reported_as_normal_exit(self):
        process = self.process(exit_code=1, polls=(None, None))
        process.wait.side_effect = [subprocess.TimeoutExpired("owned", 45), 1]
        with patch.object(RUNNER, "request_native_close", return_value=[456]):
            result = RUNNER.finish_owned_process(process)
        self.assertFalse(result["normal_close_pass"])
        self.assertTrue(result["forced_cleanup"])
        self.assertIn("TimeoutExpired", result["error"])
        process.terminate.assert_called_once()
        process.kill.assert_not_called()

    def test_missing_owned_window_keeps_failure_and_bounds_cleanup(self):
        process = self.process(exit_code=1, polls=(None, None))
        with patch.object(RUNNER, "request_native_close", side_effect=RuntimeError("no owned window")):
            result = RUNNER.finish_owned_process(process)
        self.assertFalse(result["normal_close_pass"])
        self.assertTrue(result["forced_cleanup"])
        self.assertIn("no owned window", result["error"])
        process.terminate.assert_called_once()

    def test_already_exited_child_is_never_signalled(self):
        process = self.process(exit_code=7, polls=(7,))
        with patch.object(RUNNER, "request_native_close") as close:
            result = RUNNER.finish_owned_process(process)
        self.assertEqual(result["method"], "already_exited")
        self.assertEqual(result["exit_code"], 7)
        self.assertFalse(result["normal_close_pass"])
        close.assert_not_called()
        process.terminate.assert_not_called()


class TransportTests(Files):
    def transport(self):
        inbox, log = self.root / "inbox.txt", self.root / "runtime.log"
        inbox.write_text("0 original", encoding="ascii")
        return RUNNER.ControlInbox(inbox, log)

    @staticmethod
    def sharing_error():
        error = PermissionError("temporary Windows sharing violation")
        error.winerror = 5
        return error

    def test_transient_lock_publishes_one_command_with_one_sequence(self):
        control = self.transport()
        real_replace = os.replace
        calls = []
        def locked_once(source, destination):
            calls.append(Path(source).read_text())
            if len(calls) == 1:
                raise self.sharing_error()
            real_replace(source, destination)
        ack = re.search(r".*", "[DScontrol] sequence=1 queued")
        with patch.object(RUNNER.os, "replace", side_effect=locked_once), patch.object(RUNNER.time, "sleep"), patch.object(RUNNER, "wait_for", return_value=ack) as wait:
            control.send("ui up")
        self.assertEqual(calls, ["1 ui up", "1 ui up"])
        self.assertEqual(control.sequence, 1)
        self.assertEqual(control.inbox.read_text(), "1 ui up")
        wait.assert_called_once()

    def test_nontransient_error_does_not_publish_or_wait_for_ack(self):
        control = self.transport()
        with patch.object(RUNNER.os, "replace", side_effect=OSError("not a sharing error")) as replace, patch.object(RUNNER, "wait_for") as wait:
            with self.assertRaises(OSError): control.send("ui up")
        replace.assert_called_once()
        wait.assert_not_called()
        self.assertEqual(control.inbox.read_text(), "0 original")

    def test_uncertain_publication_is_not_repeated(self):
        control = self.transport()
        real_replace = os.replace
        def published_then_error(source, destination):
            real_replace(source, destination)
            raise self.sharing_error()
        with patch.object(RUNNER.os, "replace", side_effect=published_then_error) as replace, patch.object(RUNNER, "wait_for") as wait:
            with self.assertRaises(OSError): control.send("ui up")
        replace.assert_called_once()
        wait.assert_not_called()
        self.assertEqual(control.inbox.read_text(), "1 ui up")

    def test_ack_timeout_never_republishes_input(self):
        control = self.transport()
        with patch.object(RUNNER.os, "replace", wraps=os.replace) as replace, patch.object(RUNNER, "wait_for", side_effect=TimeoutError("no acknowledgement")):
            with self.assertRaises(TimeoutError): control.send("ui up")
        replace.assert_called_once()
        self.assertEqual(control.inbox.read_text(), "1 ui up")


class ReportTests(Files):
    def fixture(self, paired=True, wrong_path=False):
        manifest = {"run_id": "runner-中文", "status": "sampled", "paired_mode": "gpu-copy" if paired else "none",
                    "sample_since_unix_ms": BASE, "sample_until_unix_ms": BASE + (9000 if paired else 4000),
                    "segments": []}
        rows, transfers, frame = [], [0, 0], 1000
        for arm in range(2 if paired else 1):
            gpu = arm == 1
            start = BASE + arm * 5000
            manifest["segments"].append({"label": f"gpu-copy-{'on' if gpu else 'off'}-c1",
                                         "command": f"perf gpu-copy {'on' if gpu else 'off'}",
                                         "since_unix_ms": start, "until_unix_ms": start + 4000})
            for i in range(9):
                transfers[int(gpu and not wrong_path)] += 1
                frame += 45 if gpu else 30
                rows.append({"unix_ms": start + i * 500, "run": manifest["run_id"], "frame": frame,
                             "viewport": [0, 0, 1620, 1080], "source_pixels": [1280, 1920],
                             "msaa_actual": 0, "msaa_requested": "0", "anisotropy_requested": "4",
                             "gpu_renderer": "offline-renderer", "frame_mean_ms": 1000 / 60,
                             "frame_p95_ms": 17, "frame_p99_ms": 20,
                             "cpu_stages_mean_max_total_calls": [[0, 0, 0], [0, 0, 0], [0, 0, 0],
                                                                  [2.35, 3.2, frame], [0, 0, 0],
                                                                  [0, 0, 0], [0, 0, 0], [0, 0, 0]],
                             "present_path_schema": 1, "fsr_requested": 2, "fsr_eligible": False,
                             "fsr_final_submitted": False, "present_shader_submitted": True,
                             "fsr_source_redundant": True, "gpu_copy_requested": gpu,
                             "gpu_text_requested": False, "gpu_text_effective": False,
                             "host_text_card_submissions": [0, 0],
                             "direct_present_transfer_submissions": list(transfers)})
        if not paired:
            manifest["segments"] = []
        (self.root / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False), encoding="utf-8")
        monitor = self.root / "super-monitor-fixture.jsonl"
        monitor.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows), encoding="utf-8")
        return manifest, monitor

    def summarize(self, **kwargs):
        manifest, monitor = self.fixture(**kwargs)
        with contextlib.redirect_stdout(io.StringIO()):
            code = RUNNER.summarize_run(self.root, monitor, manifest)
        status = json.loads((self.root / "report-status.json").read_text(encoding="utf-8"))
        return code, status

    def test_valid_pair_wins_over_expected_aggregate_mode_change(self):
        code, status = self.summarize()
        self.assertEqual(status["aggregate_exit_code"], 2)
        self.assertEqual((code, status["verdict_source"]), (0, "paired-summary.json"))
        self.assertTrue(json.loads((self.root / "paired-summary.json").read_text())["valid"])
        self.assertIn("runner-中文", (self.root / "paired-summary.stdout.log").read_text(encoding="utf-8"))

    def test_valid_timing_never_hides_forced_cleanup(self):
        manifest, monitor = self.fixture()
        manifest["cleanup"] = {"normal_close_pass": False, "forced_cleanup": True, "exit_code": 1}
        with contextlib.redirect_stdout(io.StringIO()):
            code = RUNNER.summarize_run(self.root, monitor, manifest)
        status = json.loads((self.root / "report-status.json").read_text(encoding="utf-8"))
        self.assertEqual((code, status["timing_exit_code"], status["exit_code"]), (2, 0, 2))
        self.assertTrue(json.loads((self.root / "paired-summary.json").read_text())["valid"])

    def test_request_without_transfer_is_still_rejected(self):
        code, status = self.summarize(wrong_path=True)
        self.assertEqual((code, status["exit_code"]), (2, 2))
        result = json.loads((self.root / "paired-summary.json").read_text())
        self.assertFalse(result["valid"])
        self.assertIn("direct_present_transfer_work_not_observed", result["pairs"][0]["invalid_reasons"])

    def test_unpaired_run_uses_aggregate_verdict(self):
        code, status = self.summarize(paired=False)
        self.assertEqual((code, status["aggregate_exit_code"]), (0, 0))
        self.assertEqual(status["verdict_source"], "summary.json")
        self.assertFalse((self.root / "paired-summary.json").exists())

    def test_crashed_reporter_cannot_be_mistaken_for_invalid_timing(self):
        manifest, monitor = self.fixture()
        crash = subprocess.CompletedProcess([], 1, "", "crash")
        with patch.object(RUNNER, "report_process", return_value=crash), self.assertRaises(RuntimeError):
            RUNNER.summarize_run(self.root, monitor, manifest)

    def test_usage_error_without_report_cannot_be_accepted(self):
        manifest, monitor = self.fixture()
        error = subprocess.CompletedProcess([], 2, "", "bad arguments")
        with patch.object(RUNNER, "report_process", return_value=error), self.assertRaises(RuntimeError):
            RUNNER.summarize_run(self.root, monitor, manifest)




class StartupNavigationTests(Files):
    def test_observed_registration_cancel_uses_one_sequence_owner(self):
        log = self.root / "native.log"
        log.write_text("[DSfrontendInput] page state=2\n", encoding="utf-8")
        commands = []

        def bounds(page, names):
            return "\n[DSfrontendWidgets] transformed page=%d bounds=%r\n" % (
                page, [(name, [10.0, 20.0, 40.0, 30.0]) for name in names])

        class Control:
            def send(self, command):
                commands.append(command)
                screens = {1: (5, ["BTN_PLAY"]),
                           2: (6, ["BTN_PROFILE1", "BTN_OK"]),
                           4: (9, ["BTN_STORY", "BTN_OK"])}
                if len(commands) in screens:
                    page, names = screens[len(commands)]
                    with log.open("a", encoding="utf-8") as stream:
                        stream.write(bounds(page, names))

        with patch.object(RUNNER.time, "sleep"):
            route = RUNNER.navigate_startup(Control(), log, SimpleNamespace(poll=lambda: None))
        self.assertEqual(commands.count("tap 60 300"), 1)
        self.assertEqual([entry["page"] for entry in route], [2, 5, 6, 9])

    def test_unknown_startup_widget_page_never_guesses_a_touch(self):
        log = self.root / "unsupported.log"
        log.write_text('[DSfrontendWidgets] transformed page=99 bounds=[("UNSUPPORTED", [1,2,3,4])]\n', encoding="utf-8")
        commands = []
        with self.assertRaisesRegex(RuntimeError, "unsupported native startup page"):
            RUNNER.navigate_startup(SimpleNamespace(send=commands.append), log, SimpleNamespace(poll=lambda: None))
        self.assertEqual(commands, [])

if __name__ == "__main__":
    unittest.main()
