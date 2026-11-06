#!/usr/bin/env python3
"""Compare complete, configuration-matched cycles of a paired quiet run.

This validates observed timing samples. Applied native modes, scene/camera,
visual correctness and driver acceptance still require their runtime evidence.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import re
import statistics
from pathlib import Path
from typing import Any

BASELINES = {
    "gpu-copy": "gpu-copy-off", "tex-sync": "tex-sync-finish",
    "text-overlay": "text-overlay-cpu", "fps-limit": "fps-limit-off",
    "readback-finish": "readback-finish-on",
}
MODES = {
    "gpu-copy": ("off", "on"), "tex-sync": ("finish", "flush", "off"),
    "text-overlay": ("cpu", "gpu"), "fps-limit": ("off", "30", "60"),
    "readback-finish": ("on", "off"),
}


def load_summarizer() -> Any:
    path = Path(__file__).with_name("Summarize-PerfRun.py")
    spec = importlib.util.spec_from_file_location("summarize_perf_run", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def mode_from_command(command: str) -> str:
    words = command.split()
    if len(words) == 3 and words[0] == "perf" and words[1] in MODES:
        if words[2] in MODES[words[1]]:
            return f"{words[1]}-{words[2]}"
    raise ValueError(f"unrecognized paired command: {command}")


def stage_value(result: dict[str, Any], stage: str, key: str = "window_mean_ms") -> Any:
    for row in result.get("stages", []):
        if row["stage"] == stage:
            return row.get(key)
    return None


def median(values: list[float | None]) -> float | None:
    present = [value for value in values if value is not None]
    return statistics.median(present) if present else None


def cycle_id(segment: dict[str, Any]) -> str | None:
    explicit = segment.get("cycle")
    if isinstance(explicit, int) and not isinstance(explicit, bool) and explicit > 0:
        return str(explicit)
    match = re.search(r"-c([1-9][0-9]*)$", str(segment.get("label", "")))
    return match[1] if match else None


def analyze_run(run: Path) -> dict[str, Any]:
    manifest = json.loads((run / "manifest.json").read_text(encoding="utf-8"))
    segments = manifest.get("segments")
    if not isinstance(segments, list) or not segments:
        raise ValueError("manifest has no paired segments")
    monitor_files = sorted(run.glob("super-monitor-*.jsonl"))
    if len(monitor_files) != 1:
        raise ValueError(f"expected one monitor JSONL, found {len(monitor_files)}")
    summarizer = load_summarizer()
    rows = []
    protocol_errors = []
    if manifest.get("status") != "sampled":
        protocol_errors.append("run_did_not_finish_sampling")
    family = manifest.get("paired_mode")
    if family not in BASELINES:
        raise ValueError(f"unsupported paired_mode: {family}")
    previous_end = None
    previous_frame = None
    for segment in segments:
        start, end = segment["since_unix_ms"], segment["until_unix_ms"]
        if any(isinstance(v, bool) or not isinstance(v, int) for v in (start, end)) or start >= end:
            raise ValueError(f"invalid segment bounds: {segment.get('label')}")
        mode = mode_from_command(segment["command"])
        if mode.rsplit("-", 1)[0] != family:
            protocol_errors.append("mixed_treatment_families")
        if previous_end is not None and start < previous_end:
            protocol_errors.append("overlapping_or_out_of_order_segments")
        previous_end = end
        result = summarizer.summarize(monitor_files[0], start, end)
        first_frame, last_frame = result.get("first_frame"), result.get("final_frame")
        if (previous_frame is not None and isinstance(first_frame, int)
                and first_frame < previous_frame):
            protocol_errors.append("frame_counter_regressed_between_segments")
        if isinstance(last_frame, int):
            previous_frame = last_frame
        configured = result.get("configured", {})
        if configured and manifest.get("run_id") is not None and configured.get("run") != [json.dumps(manifest["run_id"])]:
            protocol_errors.append("run_id_mismatch")
        rows.append({
            "label": segment["label"], "mode": mode, "cycle": cycle_id(segment),
            "valid": result.get("valid", False), "reason": result.get("reason"),
            "paired_valid": False, "configured": configured,
            "validity": result.get("validity", {}),
            "fps": result.get("fps_whole_run"),
            "frame_mean_ms": result.get("frame_mean_ms_whole_run"),
            "p95_snapshot_ms": result.get("snapshot_p95_median_ms"),
            "p99_snapshot_ms": result.get("snapshot_p99_median_ms"),
            "gpu_present_ms": stage_value(result, "gpu_present"),
            "texture_sync_ms": stage_value(result, "texture_sync"),
            "gpu_present_calls": stage_value(result, "gpu_present", "calls_in_sample"),
            "texture_sync_calls": stage_value(result, "texture_sync", "calls_in_sample"),
            "texture_sync_submissions": result.get("texture_sync_submissions", {}),
            "presentation_paths": result.get("presentation_paths", {}),
        })
    # A/B switches only the named treatment. Comparing different renderer,
    # source scale, viewport or MSAA settings cannot establish its effect.
    varying = {"gpu-copy": ("gpu_copy_requested",),
               "text-overlay": ("gpu_text_requested", "gpu_text_effective"),
               "readback-finish": ("readback_finish_requested",)}.get(family, ())
    signatures = {json.dumps({key: value for key, value in row["configured"].items() if key not in varying}, sort_keys=True)
                  for row in rows if row["configured"]}
    if len(signatures) != 1:
        protocol_errors.append("configuration_changed_between_segments")
    protocol_errors = sorted(set(protocol_errors))
    cycles: dict[str, list[dict[str, Any]]] = {}
    for index, row in enumerate(rows):
        # Missing IDs never silently become a purported matched pair.
        key = row["cycle"] or f"unpaired-segment-{index + 1}"
        cycles.setdefault(key, []).append(row)
    pairs = []
    for key, values in cycles.items():
        baseline = [value for value in values if value["mode"] == BASELINES[family]]
        treatment = [value for value in values if value["mode"] != BASELINES[family]]
        reasons = list(protocol_errors)
        if any(value["cycle"] is None for value in values):
            reasons.append("cycle_id_missing")
        if len(values) != 2 or len(baseline) != 1 or len(treatment) != 1:
            reasons.append("need_one_baseline_and_one_treatment_in_cycle")
        if not all(value["valid"] for value in values):
            reasons.append("invalid_sample_in_cycle")
        b = baseline[0] if len(baseline) == 1 else None
        t = treatment[0] if len(treatment) == 1 else None
        exercise_note = "Applied mode and scene/camera require separate native runtime verification."
        if b and t and family == "gpu-copy":
            if not all((value.get("gpu_present_calls") or 0) > 0 for value in (b, t)):
                reasons.append("gpu_present_work_not_observed")
            if any(value["presentation_paths"].get("available") for value in (b, t)):
                for value in (b, t):
                    path = value["presentation_paths"]
                    if not path.get("valid"):
                        reasons.append("presentation_path_evidence_unavailable_or_invalid")
                        continue
                    gpu = value["mode"] == "gpu-copy-on"
                    if path["values"]["gpu_copy_requested"] != [gpu]:
                        reasons.append("gpu_copy_requested_mode_mismatch")
                    counts = path["direct_transfers"]
                    if counts["gpu" if gpu else "cpu"] <= 0:
                        reasons.append("direct_present_transfer_work_not_observed")
                    if counts["cpu" if gpu else "gpu"]:
                        reasons.append("direct_present_transfer_mode_mismatch")
            else:
                exercise_note += " Legacy log has no direct-transfer counters; applied copy path needs separate native evidence."
        if b and t and family == "text-overlay":
            for value in (b, t):
                path = value["presentation_paths"]
                if not path.get("valid"):
                    reasons.append("presentation_path_evidence_unavailable_or_invalid")
                    continue
                gpu = value["mode"] == "text-overlay-gpu"
                if any(path["values"][key] != [gpu] for key in ("gpu_text_requested", "gpu_text_effective")):
                    reasons.append("text_overlay_applied_mode_mismatch")
                counts = path["text_cards"]
                if counts["gpu" if gpu else "cpu"] <= 0:
                    reasons.append("text_card_work_not_observed")
                if counts["cpu" if gpu else "gpu"]:
                    reasons.append("text_card_path_mismatch")
            exercise_note += " Card/transfer counters record submitted calls, not GPU completion or image equivalence."
        if b and t and family == "readback-finish":
            for value in (b, t):
                expected = 1 if value["mode"] == "readback-finish-on" else 0
                if value["configured"].get("readback_finish_requested") != [str(expected)]:
                    reasons.append("readback_finish_mode_mismatch")
            exercise_note += " The switch changes explicit glFinish behavior; pixel correctness and device-specific driver safety require separate acceptance."
        if b and t and family == "tex-sync":
            for value in (b, t):
                activity = value["texture_sync_submissions"]
                if not activity.get("valid"):
                    reasons.append("texture_submission_activity_unavailable_or_invalid")
                    continue
                mode = value["mode"].rsplit("-", 1)[-1]
                payload = activity["payload_by_mode"]
                other = activity["non_payload_by_mode"]
                if payload[mode] <= 0:
                    reasons.append("texture_payload_work_not_observed")
                if any(payload[key] or other[key] for key in payload if key != mode):
                    reasons.append("texture_submission_mode_mismatch")
            if (b.get("texture_sync_calls") or 0) <= 0:
                reasons.append("texture_sync_work_not_observed")
            if t["mode"] == "tex-sync-flush" and (t.get("texture_sync_calls") or 0) <= 0:
                reasons.append("texture_sync_work_not_observed")
            if t["mode"] == "tex-sync-off":
                if (t.get("texture_sync_calls") or 0) > 0:
                    reasons.append("texture_off_sync_calls_observed")
            exercise_note += " Payload counters observe guest submissions reaching the sync hook, including Off; not GPU completion or bypassing HD/mip/helper uploads. Matched loading workload and visual correctness require separate evidence."
        valid = not reasons
        pair = {"cycle": key, "valid": valid, "invalid_reasons": sorted(set(reasons)),
                "baseline": b["mode"] if b else None,
                "treatment": t["mode"] if t else None,
                "labels": [value["label"] for value in values],
                "evidence_limitations": exercise_note,
                "fps_change_percent": None, "frame_mean_change_percent": None}
        for label, value in (("baseline", b), ("treatment", t)):
            activity = value.get("texture_sync_submissions", {}) if value else {}
            mode = value["mode"].rsplit("-", 1)[-1] if value and family == "tex-sync" else None
            pair[label + "_payload_submissions"] = (activity.get("payload_by_mode") or {}).get(mode)
        if valid and b and t:
            for value in values:
                value["paired_valid"] = True
            pair["fps_change_percent"] = (t["fps"] / b["fps"] - 1) * 100
            pair["frame_mean_change_percent"] = (t["frame_mean_ms"] / b["frame_mean_ms"] - 1) * 100
        pairs.append(pair)
    groups = []
    for mode in sorted({row["mode"] for row in rows}):
        values = [row for row in rows if row["mode"] == mode]
        valid = [value for value in values if value["paired_valid"]]
        groups.append({"mode": mode, "runs": len(values),
                       "valid_sample_runs": sum(value["valid"] for value in values),
                       "valid_runs": len(valid),
                       "fps_median": median([value["fps"] for value in valid]),
                       "frame_mean_ms_median": median([value["frame_mean_ms"] for value in valid]),
                       "gpu_present_ms_median": median([value["gpu_present_ms"] for value in valid]),
                       "texture_sync_ms_median": median([value["texture_sync_ms"] for value in valid])})
    changes = []
    for mode in sorted({pair["treatment"] for pair in pairs if pair["treatment"]}):
        valid = [pair for pair in pairs if pair["valid"] and pair["treatment"] == mode]
        changes.append({"treatment": mode, "complete_pairs": len(valid),
                        "fps_change_percent_median": median([pair["fps_change_percent"] for pair in valid]),
                        "frame_mean_change_percent_median": median([pair["frame_mean_change_percent"] for pair in valid])})
    return {"run_dir": str(run), "run_id": manifest.get("run_id"),
            "exe_sha256": manifest.get("exe_sha256"),
            "valid": not protocol_errors and all(pair["valid"] for pair in pairs),
            "scope": "Observed timing validity only. Applied native modes, scene/camera, captures and driver acceptance are separate requirements.",
            "protocol_errors": protocol_errors, "segments": rows,
            "pairs": pairs, "groups": groups, "paired_changes": changes}


def number(value: float | None, digits: int = 2) -> str:
    return "-" if value is None else f"{value:.{digits}f}"


def markdown(result: dict[str, Any]) -> str:
    lines = [f"# Paired Timing Summary: {result['run_id']}", "", result["scope"], "",
             f"Comparable run: {'yes' if result['valid'] else 'no'}", "",
             "| Segment | Mode | Sample valid | Pair valid | FPS | Mean ms |",
             "| --- | --- | --- | --- | ---: | ---: |"]
    for row in result["segments"]:
        lines.append(f"| {row['label']} | {row['mode']} | {row['valid']} | {row['paired_valid']} | {number(row['fps'])} | {number(row['frame_mean_ms'], 3)} |")
    lines += ["", "## Complete matched cycles", "",
              "| Cycle | Baseline | Treatment | Valid | FPS change % | Payload A/B | Rejection |",
              "| --- | --- | --- | --- | ---: | --- | --- |"]
    for pair in result["pairs"]:
        payload = f"{number(pair['baseline_payload_submissions'], 0)}/{number(pair['treatment_payload_submissions'], 0)}"
        lines.append(f"| {pair['cycle']} | {pair['baseline']} | {pair['treatment']} | {pair['valid']} | {number(pair['fps_change_percent'])} | {payload} | {', '.join(pair['invalid_reasons'])} |")
    lines += ["", "## Mode medians from complete comparable pairs", "",
              "| Mode | Segments | Comparable | FPS median | Mean ms median | GPU present ms | Texture sync ms |",
              "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for row in result["groups"]:
        lines.append(f"| {row['mode']} | {row['runs']} | {row['valid_runs']} | {number(row['fps_median'])} | {number(row['frame_mean_ms_median'], 3)} | {number(row['gpu_present_ms_median'], 3)} | {number(row['texture_sync_ms_median'], 3)} |")
    lines += ["", "## Matched treatment changes", "",
              "| Treatment | Complete pairs | Median FPS change % | Median mean-ms change % |",
              "| --- | ---: | ---: | ---: |"]
    for row in result["paired_changes"]:
        lines.append(f"| {row['treatment']} | {row['complete_pairs']} | {number(row['fps_change_percent_median'])} | {number(row['frame_mean_change_percent_median'])} |")
    if result["protocol_errors"]:
        lines += ["", "Protocol errors: " + ", ".join(result["protocol_errors"])]
    notes = sorted({pair["evidence_limitations"] for pair in result["pairs"]})
    lines += [""] + notes
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_dir", type=Path)
    args = parser.parse_args()
    run = args.run_dir.resolve()
    try:
        result = analyze_run(run)
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.error(str(error))
    report = markdown(result)
    (run / "paired-summary.json").write_text(json.dumps(result, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    (run / "paired-summary.md").write_text(report, encoding="utf-8")
    print(report, end="")
    return 0 if result["valid"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
