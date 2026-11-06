"""Offline counterexamples for performance report validity; never launches games."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
BASE = 1_700_000_000_000


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PERF = load("perf_report_test", "Summarize-PerfRun.py")
PAIRED = load("paired_report_test", "Summarize-PairedRun.py")


def record(time_ms, frame, run="fixture"):
    return {"unix_ms": time_ms, "run": run, "frame": frame,
            "viewport": [0, 0, 1620, 1080], "source_pixels": [1280, 1920],
            "msaa_actual": 0, "msaa_requested": "0", "anisotropy_requested": "4",
            "gpu_renderer": "offline-fixture-renderer", "frame_mean_ms": 1000 / 60,
            "readback_finish_requested": 1,
            "frame_p95_ms": 17, "frame_p99_ms": 20,
            "cpu_stages_mean_max_total_calls": [
                [0, 0, 0], [0, 0, 0], [0, 0, 0], [2.35, 3.2, frame],
                [20, 80, 2], [30, 90, 1], [0.4, 149, 659]]}


def records():
    return [record(BASE + i * 500, 1000 + i * 30) for i in range(41)]


def present_row(row, copy=False, text=False, effective=None, cards=(0, 0), transfers=(0, 0)):
    row.update(present_path_schema=1, fsr_requested=2, fsr_eligible=False,
               fsr_final_submitted=False, present_shader_submitted=True,
               fsr_source_redundant=True, gpu_copy_requested=copy,
               gpu_text_requested=text, gpu_text_effective=text if effective is None else effective,
               host_text_card_submissions=list(cards), direct_present_transfer_submissions=list(transfers))
    return row


class Files(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="deadspace-offline-perf-")
        self.root = Path(self.temp.name).resolve()
        self.addCleanup(self.cleanup)

    def cleanup(self):
        # Only the generated test root may be recursively removed.
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.assertTrue(self.root.name.startswith("deadspace-offline-perf-"))
        self.assertEqual(Path(self.temp.name).resolve(), self.root)
        self.temp.cleanup()

    def write(self, rows, name="sample.jsonl"):
        path = self.root / name
        path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
        return path

    def summary(self, rows, bounded=True):
        path = self.write(rows)
        return PERF.summarize(path, BASE, BASE + 20000) if bounded else PERF.summarize(path)


class SampleTests(Files):
    def test_requested_fsr_does_not_invent_an_easu_submission(self):
        rows = [present_row(row, transfers=(i, 0)) for i, row in enumerate(records())]
        result = self.summary(rows)
        self.assertTrue(result["valid"])
        self.assertEqual(result["presentation_paths"]["values"]["fsr_requested"], [2])
        self.assertEqual(result["presentation_paths"]["values"]["fsr_final_submitted"], [False])

    def test_effective_shader_quality_changes_inside_arm_are_not_trimmed(self):
        rows = [present_row(row) for row in records()]
        for i, row in enumerate(rows):
            row.update(fsr_source_redundant=False, fsr_eligible=True, fsr_final_submitted=i >= 20)
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertEqual(result["validity"]["trimmed_startup_seconds"], 0)

    def test_impossible_present_flags_and_missing_new_fields_are_rejected(self):
        for problem in ("fsr", "text", "missing", "schema"):
            with self.subTest(problem=problem):
                rows = [present_row(row) for row in records()]
                if problem == "fsr": rows[10]["fsr_final_submitted"] = True
                if problem == "text": rows[10]["gpu_text_effective"] = True
                if problem == "missing": del rows[10]["gpu_copy_requested"]
                if problem == "schema":
                    for row in rows: del row["present_path_schema"]
                self.assertFalse(self.summary(rows)["valid"])

    def test_presentation_counter_reset_is_not_hidden_by_positive_final_delta(self):
        rows = [present_row(row, cards=(i, 0)) for i, row in enumerate(records())]
        rows[20]["host_text_card_submissions"][0] = 0
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("presentation_submission_counter_regressed", result["reason"])

    def submissions(self, rows, mode="off", payload=True):
        for index, row in enumerate(rows):
            row["texture_sync_submission_schema"] = 1
            row["texture_sync_submissions"] = {key: [0, 0] for key in ("off", "flush", "finish")}
            row["texture_sync_submissions"][mode][0 if payload else 1] = index
        return rows

    def test_legacy_timing_does_not_invent_upload_evidence(self):
        result = self.summary(records())
        self.assertTrue(result["valid"])
        self.assertFalse(result["texture_sync_submissions"]["available"])

    def test_off_payloads_are_visible_without_sync_timer_calls(self):
        result = self.summary(self.submissions(records()))
        self.assertTrue(result["valid"])
        self.assertEqual(result["texture_sync_submissions"]["payload_by_mode"]["off"], 40)
        self.assertEqual(result["stages"][6]["calls_in_sample"], 0)

    def test_storage_only_submissions_do_not_invent_payloads(self):
        result = self.summary(self.submissions(records(), payload=False))
        self.assertTrue(result["valid"])
        self.assertEqual(result["texture_sync_submissions"]["payload_by_mode"]["off"], 0)
        self.assertEqual(result["texture_sync_submissions"]["non_payload_by_mode"]["off"], 40)

    def test_submission_reset_with_positive_final_delta_is_invalid(self):
        rows = self.submissions(records())
        rows[20]["texture_sync_submissions"]["off"][0] = 1
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("texture_submission_counter_regressed", result["reason"])

    def test_partial_or_malformed_submission_telemetry_is_invalid(self):
        for problem in ("missing", "boolean", "version"):
            with self.subTest(problem=problem):
                rows = self.submissions(records())
                if problem == "missing": del rows[10]["texture_sync_submissions"]
                if problem == "boolean": rows[10]["texture_sync_submissions"]["off"][0] = True
                if problem == "version": rows[10]["texture_sync_submission_schema"] = 2
                self.assertFalse(self.summary(rows)["valid"])

    def test_stable_bounded_window(self):
        result = self.summary(records())
        self.assertTrue(result["valid"])
        self.assertEqual(result["fps_whole_run"], 60)
        self.assertEqual(result["validity"]["trimmed_startup_seconds"], 0)

    def test_configuration_switch_inside_timed_arm_is_not_startup(self):
        rows = records()
        for row in rows[20:]:
            row["msaa_actual"] = 4
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertEqual(result["validity"]["trimmed_startup_seconds"], 0)
        self.assertIn("configuration_changed_inside_measurement", result["reason"])

    def test_whole_log_can_still_trim_short_startup(self):
        rows = records()
        for row in rows[:10]:
            row["viewport"] = [0, 0, 800, 600]
        result = self.summary(rows, bounded=False)
        self.assertTrue(result["valid"])
        self.assertEqual(result["validity"]["trimmed_startup_seconds"], 5000 / 1000)

    def test_stalled_tail_cannot_be_called_complete(self):
        result = self.summary(records()[:5])
        self.assertFalse(result["valid"])
        self.assertEqual(result["validity"]["end_lag_ms"], 18000)

    def test_missing_start_cannot_be_called_complete(self):
        self.assertFalse(self.summary(records()[10:])["valid"])

    def test_interior_stall_is_included_not_discarded(self):
        rows = []
        for i in list(range(3)) + list(range(24, 41)):
            frame = 1000 + i * 30 - (660 if i >= 24 else 0)
            rows.append(record(BASE + i * 500, frame))
        result = self.summary(rows)
        self.assertTrue(result["valid"])
        self.assertEqual(result["fps_whole_run"], 27)

    def test_frame_reset_with_positive_final_delta_is_invalid(self):
        rows = records()
        rows[20]["frame"] = 0
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("frame_counter_regressed", result["reason"])

    def test_boolean_is_not_a_frame_counter(self):
        rows = records()
        rows[10]["frame"] = True
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("frame_counter_missing_or_invalid", result["reason"])

    def test_renderer_switch_is_a_configuration_change(self):
        rows = records()
        rows[-10:][0]["gpu_renderer"] = "another-driver"
        self.assertFalse(self.summary(rows)["valid"])

    def test_append_clock_regression_is_not_hidden_by_sorting(self):
        rows = records()
        rows[20]["unix_ms"] = rows[19]["unix_ms"] - 1
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("snapshot_timestamps_not_strictly_increasing", result["reason"])

    def test_inactive_stage_does_not_inherit_loading_maximum(self):
        stage = self.summary(records())["stages"][6]
        self.assertEqual(stage["total_calls"], 659)
        self.assertEqual(stage["calls_in_sample"], 0)
        self.assertIsNone(stage["window_mean_ms"])
        self.assertIsNone(stage["window_max_ms"])

    def test_new_stage_calls_have_sample_delta(self):
        rows = records()
        rows[-1]["cpu_stages_mean_max_total_calls"][6] = [2, 3, 662]
        stage = self.summary(rows)["stages"][6]
        self.assertEqual(stage["calls_in_sample"], 3)
        self.assertEqual(stage["window_max_ms"], 3)

    def test_stage_counter_reset_is_not_current_work(self):
        rows = records()
        rows[20]["cpu_stages_mean_max_total_calls"][6][2] = 1
        result = self.summary(rows)
        self.assertFalse(result["valid"])
        self.assertIn("stage_counter_regressed", result["reason"])

    def test_unknown_percentiles_render_as_missing(self):
        rows = records()
        for row in rows:
            row["frame_p95_ms"] = row["frame_p99_ms"] = None
        result = self.summary(rows)
        self.assertTrue(result["valid"])
        self.assertIn("| - | - |", PERF.markdown([result]))


class PairedTests(Files):
    def exercise_present(self, manifest, rows, empty_cards=False, gpu_fallback=False, wrong_copy=False):
        cards, transfers = [0, 0], [0, 0]
        for row in rows:
            segment = next(v for v in manifest["segments"] if v["since_unix_ms"] <= row["unix_ms"] <= v["until_unix_ms"])
            mode = segment["command"].rsplit(" ", 1)[-1]
            gpu_copy = manifest["paired_mode"] == "gpu-copy" and mode == "on"
            gpu_text = manifest["paired_mode"] == "text-overlay" and mode == "gpu"
            effective = gpu_text and not gpu_fallback
            if manifest["paired_mode"] == "text-overlay" and not empty_cards: cards[int(effective)] += 1
            transfers[int(gpu_copy and not wrong_copy)] += 1
            present_row(row, gpu_copy, gpu_text, effective, cards, transfers)
        self.save(manifest, rows)

    def exercise_uploads(self, manifest, rows, payload=True, force_mode=None):
        counters = {mode: [0, 0] for mode in ("off", "flush", "finish")}
        syncs = 659
        for row in rows:
            segment = next(segment for segment in manifest["segments"]
                           if segment["since_unix_ms"] <= row["unix_ms"] <= segment["until_unix_ms"])
            mode = force_mode or segment["command"].rsplit(" ", 1)[-1]
            counters[mode][0 if payload else 1] += 1
            row["texture_sync_submission_schema"] = 1
            row["texture_sync_submissions"] = copy.deepcopy(counters)
            if mode != "off": syncs += 1
            row["cpu_stages_mean_max_total_calls"][6][2] = syncs
        self.save(manifest, rows)

    def test_gpu_copy_modes_are_not_texture_sync_modes(self):
        manifest, rows = self.fixture()
        for index, row in enumerate(rows):
            row["texture_sync_submission_schema"] = 1
            row["texture_sync_submissions"] = {"off": [0, 0], "flush": [0, 0], "finish": [index, 0]}
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertTrue(result["valid"])
        self.assertTrue(all(pair["baseline_payload_submissions"] is None
                            and pair["treatment_payload_submissions"] is None for pair in result["pairs"]))

    def fixture(self, family="gpu-copy", cycles=2):
        baseline, treatment = {"gpu-copy": ("off", "on"), "tex-sync": ("finish", "flush"),
                               "text-overlay": ("cpu", "gpu"),
                               "readback-finish": ("on", "off")}[family]
        segments = []
        rows = []
        cursor = 1000
        for cycle in range(1, cycles + 1):
            order = [baseline, treatment] if cycle % 2 else [treatment, baseline]
            for mode in order:
                start = BASE + len(segments) * 5000
                fps = 60 if mode == baseline else 90
                segments.append({"label": f"{family}-{mode}-c{cycle}",
                                 "command": f"perf {family} {mode}",
                                 "since_unix_ms": start, "until_unix_ms": start + 4000})
                new_rows = [record(start + i * 500, cursor + i * fps // 2) for i in range(9)]
                if family == "readback-finish":
                    for row in new_rows:
                        row["readback_finish_requested"] = 1 if mode == "on" else 0
                rows.extend(new_rows)
                cursor = rows[-1]["frame"] + 60
        manifest = {"status": "sampled", "run_id": "fixture", "exe_sha256": "f" * 64,
                    "paired_mode": family, "segments": segments}
        self.save(manifest, rows)
        return manifest, rows

    def save(self, manifest, rows):
        (self.root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        self.write(rows, "super-monitor-fixture.jsonl")

    def test_counterbalanced_cycles_are_matched(self):
        self.fixture()
        result = PAIRED.analyze_run(self.root)
        self.assertTrue(result["valid"])
        self.assertEqual(len(result["pairs"]), 2)
        self.assertEqual(result["paired_changes"][0]["fps_change_percent_median"], 50)
        self.assertTrue(all(group["valid_runs"] == 2 for group in result["groups"]))

    def test_readback_finish_pair_requires_the_observed_mode_switch(self):
        manifest, rows = self.fixture(family="readback-finish")
        self.assertTrue(PAIRED.analyze_run(self.root)["valid"])
        for row in rows:
            row["readback_finish_requested"] = 1
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("readback_finish_mode_mismatch", result["pairs"][0]["invalid_reasons"])

    def test_copy_request_is_the_only_allowed_varying_copy_setting(self):
        manifest, rows = self.fixture()
        self.exercise_present(manifest, rows)
        self.assertTrue(PAIRED.analyze_run(self.root)["valid"])

    def test_same_fsr_request_does_not_hide_different_applied_quality_between_arms(self):
        manifest, rows = self.fixture(cycles=1)
        self.exercise_present(manifest, rows)
        for row in rows:
            row.update(fsr_source_redundant=False, fsr_eligible=True, fsr_final_submitted=True)
        self.save(manifest, rows)
        self.assertTrue(PAIRED.analyze_run(self.root)["valid"])
        for row in rows:
            if row["unix_ms"] < BASE + 5000: row["fsr_final_submitted"] = False
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("configuration_changed_between_segments", result["protocol_errors"])

    def test_copy_request_without_gpu_transfer_work_is_not_an_applied_copy_pair(self):
        manifest, rows = self.fixture()
        self.exercise_present(manifest, rows, wrong_copy=True)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("direct_present_transfer_work_not_observed", result["pairs"][0]["invalid_reasons"])

    def test_legacy_text_flag_pair_is_not_proof_of_text_rendering(self):
        self.fixture(family="text-overlay")
        self.assertFalse(PAIRED.analyze_run(self.root)["valid"])

    def test_text_pairs_require_the_actual_cpu_and_gpu_card_paths(self):
        manifest, rows = self.fixture(family="text-overlay")
        self.exercise_present(manifest, rows)
        self.assertTrue(PAIRED.analyze_run(self.root)["valid"])

    def test_empty_text_scene_does_not_establish_text_overlay_performance(self):
        manifest, rows = self.fixture(family="text-overlay")
        self.exercise_present(manifest, rows, empty_cards=True)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("text_card_work_not_observed", result["pairs"][0]["invalid_reasons"])

    def test_cpu_fallback_is_not_reported_as_gpu_text_work(self):
        manifest, rows = self.fixture(family="text-overlay")
        self.exercise_present(manifest, rows, gpu_fallback=True)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("text_overlay_applied_mode_mismatch", result["pairs"][0]["invalid_reasons"])

    def test_text_changes_during_copy_experiment_are_a_configuration_confound(self):
        manifest, rows = self.fixture(cycles=1)
        self.exercise_present(manifest, rows)
        for row in rows:
            if row["unix_ms"] >= BASE + 5000:
                row.update(gpu_text_requested=True, gpu_text_effective=True)
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("configuration_changed_between_segments", result["protocol_errors"])

    def test_one_bad_arm_excludes_both_arms_of_its_cycle(self):
        manifest, rows = self.fixture()
        rows = [row for row in rows if not BASE + 5000 < row["unix_ms"] <= BASE + 9000]
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertFalse(result["pairs"][0]["valid"])
        self.assertTrue(result["pairs"][1]["valid"])
        self.assertTrue(all(group["valid_runs"] == 1 for group in result["groups"]))

    def test_different_quality_settings_between_arms_are_not_comparable(self):
        manifest, rows = self.fixture()
        for row in rows[9:18]:
            row["source_pixels"] = [640, 960]
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("configuration_changed_between_segments", result["protocol_errors"])
        self.assertTrue(all(group["fps_median"] is None for group in result["groups"]))

    def test_different_quality_between_complete_cycles_is_not_pooled(self):
        manifest, rows = self.fixture()
        for row in rows[18:]:
            row["msaa_actual"] = 4
        self.save(manifest, rows)
        self.assertFalse(PAIRED.analyze_run(self.root)["valid"])

    def test_unmatched_arm_is_not_pooled_into_other_pairs(self):
        manifest, rows = self.fixture()
        manifest["segments"].pop()
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertTrue(all(group["valid_runs"] == 1 for group in result["groups"]))

    def test_missing_cycle_id_does_not_invent_pairs(self):
        manifest, rows = self.fixture()
        manifest["segments"][0]["label"] = "baseline"
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("cycle_id_missing", result["pairs"][0]["invalid_reasons"])

    def test_overlapping_segments_invalidate_protocol(self):
        manifest, rows = self.fixture()
        manifest["segments"][1]["since_unix_ms"] = BASE + 3000
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("overlapping_or_out_of_order_segments", result["protocol_errors"])

    def test_failed_run_is_not_a_successful_experiment(self):
        manifest, rows = self.fixture()
        manifest["status"] = "failed"
        self.save(manifest, rows)
        self.assertFalse(PAIRED.analyze_run(self.root)["valid"])

    def test_steady_texture_run_does_not_prove_synchronization_effect(self):
        self.fixture(family="tex-sync")
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("texture_sync_work_not_observed", result["pairs"][0]["invalid_reasons"])
        self.assertTrue(all(group["texture_sync_ms_median"] is None for group in result["groups"]))

    def test_legacy_off_timer_absence_does_not_prove_upload_activity(self):
        manifest, rows = self.fixture(family="tex-sync", cycles=1)
        manifest["segments"][1]["command"] = "perf tex-sync off"
        counter = 659
        for row in rows:
            if row["unix_ms"] < BASE + 5000:
                counter += 1
            row["cpu_stages_mean_max_total_calls"][6][2] = counter
        self.save(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("texture_submission_activity_unavailable_or_invalid", result["pairs"][0]["invalid_reasons"])

    def test_finish_flush_pairs_require_independent_payload_work(self):
        manifest, rows = self.fixture(family="tex-sync")
        self.exercise_uploads(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertTrue(result["valid"])
        self.assertEqual(result["paired_changes"][0]["complete_pairs"], 2)

    def test_off_pairs_can_observe_payloads_without_sync_calls(self):
        manifest, rows = self.fixture(family="tex-sync", cycles=1)
        manifest["segments"][1]["command"] = "perf tex-sync off"
        self.exercise_uploads(manifest, rows)
        result = PAIRED.analyze_run(self.root)
        self.assertTrue(result["valid"])
        self.assertEqual(result["pairs"][0]["treatment_payload_submissions"], 8)
        self.assertEqual(result["segments"][1]["texture_sync_calls"], 0)

    def test_finish_calls_and_allocations_alone_are_not_loading_payload_work(self):
        manifest, rows = self.fixture(family="tex-sync", cycles=1)
        self.exercise_uploads(manifest, rows, payload=False)
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("texture_payload_work_not_observed", result["pairs"][0]["invalid_reasons"])

    def test_counter_modes_must_match_the_requested_treatment(self):
        manifest, rows = self.fixture(family="tex-sync", cycles=1)
        manifest["segments"][1]["command"] = "perf tex-sync off"
        self.exercise_uploads(manifest, rows, force_mode="finish")
        result = PAIRED.analyze_run(self.root)
        self.assertFalse(result["valid"])
        self.assertIn("texture_submission_mode_mismatch", result["pairs"][0]["invalid_reasons"])
        self.assertIn("texture_off_sync_calls_observed", result["pairs"][0]["invalid_reasons"])

    def test_extra_command_words_are_rejected(self):
        with self.assertRaises(ValueError):
            PAIRED.mode_from_command("perf gpu-copy on trailing")

    def test_cli_fails_closed_and_writes_rejection(self):
        manifest, rows = self.fixture()
        manifest["status"] = "failed"
        self.save(manifest, rows)
        process = subprocess.run([sys.executable, str(ROOT / "Summarize-PairedRun.py"), str(self.root)], capture_output=True)
        self.assertEqual(process.returncode, 2)
        report = json.loads((self.root / "paired-summary.json").read_text(encoding="utf-8"))
        self.assertFalse(report["valid"])
        self.assertIsNone(report["groups"][0]["fps_median"])


if __name__ == "__main__":
    unittest.main()
