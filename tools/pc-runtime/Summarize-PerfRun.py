#!/usr/bin/env python3
"""Summarize touchHLE super-monitor JSONL without inventing missing metrics."""

from __future__ import annotations

import argparse
import json
import math
import statistics
from pathlib import Path
from typing import Any

STAGE_NAMES = [
    "readback",
    "cpu_overlay",
    "ca_composition",
    "gpu_present",
    "shader_compile_call",
    "shader_link_call",
    "texture_sync",
]
# The monitor emits at most twice a second. A bounded measurement must reach
# both requested edges within two normal snapshots; otherwise a stalled tail
# can masquerade as a fast, complete run. Interior stalls remain in the sample.
BOUNDARY_TOLERANCE_MS = 1000
SIGNATURE_KEYS = [
    "run", "viewport", "source_pixels", "msaa_actual", "msaa_requested",
    "anisotropy_requested", "gpu_renderer",
    "fsr_requested", "fsr_eligible", "fsr_final_submitted", "present_shader_submitted",
    "fsr_source_redundant", "gpu_copy_requested", "readback_finish_requested",
    "gpu_text_requested", "gpu_text_effective",
]

PRESENT_BOOL_FIELDS = ("fsr_eligible", "fsr_final_submitted", "present_shader_submitted",
                       "fsr_source_redundant", "gpu_copy_requested", "gpu_text_requested", "gpu_text_effective")


def discover(paths: list[Path]) -> list[Path]:
    files: list[Path] = []
    for path in paths:
        if path.is_dir():
            files.extend(sorted(path.glob("super-monitor-*.jsonl")))
        elif path.is_file():
            files.append(path)
        else:
            raise FileNotFoundError(path)
    return sorted(set(files))


def load_records(
    path: Path,
    since_unix_ms: int | None = None,
    until_unix_ms: int | None = None,
) -> tuple[list[dict[str, Any]], int]:
    records: list[dict[str, Any]] = []
    bad = 0
    with path.open("r", encoding="utf-8") as stream:
        for line in stream:
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                bad += 1
                continue
            if isinstance(value, dict):
                timestamp = value.get("unix_ms")
                if isinstance(timestamp, bool) or not isinstance(timestamp, int):
                    bad += 1
                    continue
                if since_unix_ms is not None and timestamp < since_unix_ms:
                    continue
                if until_unix_ms is not None and timestamp > until_unix_ms:
                    continue
                records.append(value)
            else:
                bad += 1
    # Native JSONL is append ordered. Sorting here concealed clock regressions
    # and changed which counter observations were treated as consecutive.
    return records, bad


def percentile_nearest(values: list[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, math.ceil(len(ordered) * fraction) - 1))
    return ordered[index]


def finite(value: Any) -> float | None:
    if not isinstance(value, bool) and isinstance(value, (int, float)) and math.isfinite(value):
        return float(value)
    return None


def texture_submissions(records: list[dict[str, Any]]) -> dict[str, Any]:
    """Observe sync-hook work independently of the optional sync timer."""
    modes = ("off", "flush", "finish")
    available = any("texture_sync_submissions" in row or "texture_sync_submission_schema" in row
                    for row in records)
    result = {"available": available, "valid": False, "payload_by_mode": None,
              "non_payload_by_mode": None, "reason": None}
    if not available:
        result["reason"] = "texture_submission_activity_unavailable"
        return result
    counters = []
    for row in records:
        values = row.get("texture_sync_submissions")
        if (type(row.get("texture_sync_submission_schema")) is not int
                or row["texture_sync_submission_schema"] != 1 or not isinstance(values, dict)
                or any(not isinstance(values.get(mode), list) or len(values[mode]) != 2
                       or any(type(value) is not int or value < 0 for value in values[mode])
                       for mode in modes)):
            result["reason"] = "texture_submission_counters_missing_or_invalid"
            return result
        counters.append([value for mode in modes for value in values[mode]])
    if any(a > b for first, second in zip(counters, counters[1:])
           for a, b in zip(first, second)):
        result["reason"] = "texture_submission_counter_regressed"
        return result
    if len(counters) < 2:
        result["reason"] = "need_two_texture_submission_snapshots"
        return result
    deltas = [b - a for a, b in zip(counters[0], counters[-1])]
    result.update(valid=True, payload_by_mode=dict(zip(modes, deltas[::2])),
                  non_payload_by_mode=dict(zip(modes, deltas[1::2])))
    return result


def presentation_paths(records: list[dict[str, Any]]) -> dict[str, Any]:
    present_keys = ("present_path_schema", "fsr_requested", *PRESENT_BOOL_FIELDS,
                    "host_text_card_submissions", "direct_present_transfer_submissions")
    available = any(any(key in row for key in present_keys) for row in records)
    result = {"available": available, "valid": False, "reason": None,
              "values": {}, "text_cards": None, "direct_transfers": None}
    if not available:
        result["reason"] = "presentation_path_evidence_unavailable"
        return result
    snapshots = []
    for row in records:
        if (type(row.get("present_path_schema")) is not int or row["present_path_schema"] != 1
                or type(row.get("fsr_requested")) is not int or row["fsr_requested"] not in (0, 1, 2)
                or any(type(row.get(key)) is not bool for key in PRESENT_BOOL_FIELDS)):
            result["reason"] = "presentation_path_fields_missing_or_invalid"
            return result
        if ((row["fsr_eligible"] and (row["fsr_requested"] == 0 or row["fsr_source_redundant"]))
                or (row["fsr_final_submitted"] and not (row["fsr_eligible"] and row["present_shader_submitted"]))
                or (row["gpu_text_effective"] and not row["gpu_text_requested"])):
            result["reason"] = "presentation_path_flags_inconsistent"
            return result
        counters = []
        for key in ("host_text_card_submissions", "direct_present_transfer_submissions"):
            values = row.get(key)
            if (not isinstance(values, list) or len(values) != 2
                    or any(type(value) is not int or value < 0 for value in values)):
                result["reason"] = "presentation_submission_counters_missing_or_invalid"
                return result
            counters.extend(values)
        snapshots.append(counters)
    if any(a > b for first, second in zip(snapshots, snapshots[1:]) for a, b in zip(first, second)):
        result["reason"] = "presentation_submission_counter_regressed"
        return result
    if len(snapshots) < 2:
        result["reason"] = "need_two_presentation_snapshots"
        return result
    delta = [b - a for a, b in zip(snapshots[0], snapshots[-1])]
    result.update(valid=True, values={key: sorted({row[key] for row in records})
                  for key in ("fsr_requested", *PRESENT_BOOL_FIELDS)},
                  text_cards=dict(zip(("cpu", "gpu"), delta[:2])),
                  direct_transfers=dict(zip(("cpu", "gpu"), delta[2:])))
    return result


def stage_summary(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    windows: list[list[float]] = [[] for _ in STAGE_NAMES]
    maxima: list[float] = [0.0 for _ in STAGE_NAMES]
    calls: list[int] = [0 for _ in STAGE_NAMES]
    last_total = [-1 for _ in STAGE_NAMES]
    sample_calls = [0 for _ in STAGE_NAMES]
    resets = [0 for _ in STAGE_NAMES]

    if records:
        rows = records[0].get("cpu_stages_mean_max_total_calls")
        if isinstance(rows, list):
            for index, row in enumerate(rows[: len(STAGE_NAMES)]):
                if (isinstance(row, list) and len(row) == 3
                        and not isinstance(row[2], bool) and isinstance(row[2], int)
                        and row[2] >= 0):
                    last_total[index] = row[2]
                    calls[index] = row[2]

    for record in records[1:]:
        rows = record.get("cpu_stages_mean_max_total_calls")
        if not isinstance(rows, list):
            continue
        for index, row in enumerate(rows[: len(STAGE_NAMES)]):
            if not isinstance(row, list) or len(row) != 3:
                continue
            mean = finite(row[0])
            maximum = finite(row[1])
            total = row[2]
            if mean is None or isinstance(total, bool) or not isinstance(total, int) or total < 0:
                continue
            calls[index] = max(calls[index], total)
            if last_total[index] < 0:
                last_total[index] = total
                continue
            if total < last_total[index]:
                resets[index] += 1
                last_total[index] = total
                continue
            if total > 0 and total > last_total[index]:
                windows[index].append(mean)
                sample_calls[index] += total - last_total[index]
                if maximum is not None:
                    maxima[index] = max(maxima[index], maximum)
                last_total[index] = total

    result = []
    for index, name in enumerate(STAGE_NAMES):
        result.append(
            {
                "stage": name,
                "window_mean_ms": statistics.fmean(windows[index])
                if windows[index]
                else None,
                # An inactive stage retains its previous snapshot in the
                # runtime. That old maximum is not work in this measurement.
                "window_max_ms": maxima[index] if windows[index] else None,
                "total_calls": calls[index],
                "calls_in_sample": sample_calls[index],
                "counter_regressions": resets[index],
                "deduped_windows": len(windows[index]),
            }
        )
    return result


def signature(records: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        key: sorted(
            {
                json.dumps(record.get(key), sort_keys=True, ensure_ascii=True)
                for record in records
            }
        )
        for key in SIGNATURE_KEYS
    }


def stable_tail(records: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], float]:

    def key(record: dict[str, Any]) -> tuple[str, ...]:
        return tuple(
            json.dumps(record.get(name), sort_keys=True, ensure_ascii=True)
            for name in SIGNATURE_KEYS
        )

    start = len(records) - 1
    last_key = key(records[start])
    while start > 0 and key(records[start - 1]) == last_key:
        start -= 1
    trimmed_ms = int(records[start].get("unix_ms", 0)) - int(
        records[0].get("unix_ms", 0)
    )
    return records[start:], max(0.0, trimmed_ms / 1000.0)


def summarize(
    path: Path,
    since_unix_ms: int | None = None,
    until_unix_ms: int | None = None,
) -> dict[str, Any]:
    raw_records, bad_lines = load_records(path, since_unix_ms, until_unix_ms)
    if len(raw_records) < 2:
        return {
            "file": str(path),
            "valid": False,
            "reason": "need at least two complete monitor snapshots",
            "records": len(raw_records),
            "bad_lines": bad_lines,
        }

    explicit_window = since_unix_ms is not None or until_unix_ms is not None
    # Startup trimming is only meaningful when inspecting an entire log.
    # Never salvage an explicitly timed A/B arm by discarding its transition.
    records, trimmed_startup_seconds = (
        (raw_records, 0.0) if explicit_window else stable_tail(raw_records)
    )
    first = records[0]
    last = records[-1]
    start_ms = int(first.get("unix_ms", 0))
    end_ms = int(last.get("unix_ms", 0))
    frame_values = [record.get("frame") for record in records]
    frames_valid = all(
        not isinstance(value, bool) and isinstance(value, int) and value >= 0
        for value in frame_values
    )
    first_frame = frame_values[0] if frames_valid else 0
    last_frame = frame_values[-1] if frames_valid else 0
    frame_monotonic = frames_valid and all(
        a <= b for a, b in zip(frame_values, frame_values[1:])
    )
    # Check the original observations, including any startup that was trimmed.
    times = [record["unix_ms"] for record in raw_records]
    time_monotonic = all(a < b for a, b in zip(times, times[1:]))
    elapsed_s = max(0.0, (end_ms - start_ms) / 1000.0)
    frames = max(0, last_frame - first_frame)
    whole_fps = frames / elapsed_s if elapsed_s > 0 else None
    whole_frame_mean = elapsed_s * 1000.0 / frames if frames > 0 else None

    configured = signature(records)
    stable = all(len(values) == 1 for values in configured.values())
    startup_trim_allowed = trimmed_startup_seconds <= 30.0
    start_lag = start_ms - since_unix_ms if since_unix_ms is not None else None
    end_lag = until_unix_ms - end_ms if until_unix_ms is not None else None
    coverage_ok = all(
        lag is None or 0 <= lag <= BOUNDARY_TOLERANCE_MS
        for lag in (start_lag, end_lag)
    )
    stages = stage_summary(records)
    submissions = texture_submissions(records)
    present = presentation_paths(records)
    reasons = []
    if not stable:
        reasons.append("configuration_changed_inside_measurement")
    if not startup_trim_allowed:
        reasons.append("startup_trim_exceeds_30_seconds")
    if not time_monotonic:
        reasons.append("snapshot_timestamps_not_strictly_increasing")
    if not frames_valid:
        reasons.append("frame_counter_missing_or_invalid")
    elif not frame_monotonic:
        reasons.append("frame_counter_regressed")
    if not coverage_ok:
        reasons.append("requested_window_not_covered")
    if frames <= 0 or elapsed_s <= 0:
        reasons.append("no_positive_frame_interval")
    if any(stage["counter_regressions"] for stage in stages):
        reasons.append("stage_counter_regressed")
    if submissions["available"] and not submissions["valid"]:
        reasons.append(submissions["reason"])
    if present["available"] and not present["valid"]:
        reasons.append(present["reason"])
    validity = {
        "configuration_stable": stable,
        "records": len(raw_records),
        "analyzed_records": len(records),
        "bad_lines": bad_lines,
        "trimmed_startup_seconds": trimmed_startup_seconds,
        "startup_trim_allowed": startup_trim_allowed,
        "elapsed_seconds": elapsed_s,
        "frames": frames,
        "explicit_measurement_window": explicit_window,
        "frame_counter_monotonic": frame_monotonic,
        "snapshot_timestamps_monotonic": time_monotonic,
        "measurement_covered": coverage_ok,
        "start_lag_ms": start_lag,
        "end_lag_ms": end_lag,
        "boundary_tolerance_ms": BOUNDARY_TOLERANCE_MS,
        "invalid_reasons": reasons,
        "known_limitations": [
            "FPS uses the observed snapshot interval; bounded-window edge gaps are reported separately",
            "p95/p99 are monitor-window snapshots, not raw whole-run frame percentiles",
            "frames_over_50ms is a per-snapshot window counter where available",
            "gpu_time_ms is unavailable in the current monitor",
            "stage means/maxima are recent windows that can straddle a measurement boundary; call deltas cover analyzed snapshots only",
        ],
    }
    p95 = [
        value
        for record in records
        if (value := finite(record.get("frame_p95_ms"))) is not None
    ]
    p99 = [
        value
        for record in records
        if (value := finite(record.get("frame_p99_ms"))) is not None
    ]
    means = [
        value
        for record in records
        if (value := finite(record.get("frame_mean_ms"))) is not None
    ]

    return {
        "file": str(path),
        "valid": not reasons,
        "reason": "; ".join(reasons) if reasons else None,
        "run": last.get("run"),
        "start_unix_ms": start_ms,
        "end_unix_ms": end_ms,
        "sample_window": {
            "since_unix_ms": since_unix_ms,
            "until_unix_ms": until_unix_ms,
        },
        "fps_whole_run": whole_fps,
        "frame_mean_ms_whole_run": whole_frame_mean,
        "frame_mean_ms_snapshot": statistics.fmean(means) if means else None,
        "snapshot_p95_median_ms": statistics.median(p95) if p95 else None,
        "snapshot_p95_max_ms": max(p95) if p95 else None,
        "snapshot_p99_median_ms": statistics.median(p99) if p99 else None,
        "snapshot_p99_max_ms": max(p99) if p99 else None,
        "final_frame": last_frame,
        "first_frame": first_frame,
        "configured": configured,
        "validity": validity,
        "stages": stages,
        "texture_sync_submissions": submissions,
        "presentation_paths": present,
    }


def markdown(results: list[dict[str, Any]]) -> str:
    lines = [
        "# Performance Run Summary",
        "",
        "| File | Valid | FPS | Frame mean | p95 snapshot | p99 snapshot |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for result in results:
        if not result.get("valid"):
            lines.append(
                f"| `{Path(result['file']).name}` | no | - | - | - | - |"
            )
            continue
        def number(value: float | None) -> str:
            return "-" if value is None else f"{value:.2f}"

        lines.append(
            "| `{}` | yes | {} | {} | {} | {} |".format(
                Path(result["file"]).name,
                number(result["fps_whole_run"]),
                number(result["frame_mean_ms_whole_run"]),
                number(result["snapshot_p95_median_ms"]),
                number(result["snapshot_p99_median_ms"]),
            )
        )
    lines.extend(
        [
            "",
            "Whole-run FPS and frame mean use frame-count delta over wall-clock time.",
            "Explicit measurement windows are never startup-trimmed; both edges must be covered within 1 second.",
            "Snapshot p95/p99 are medians of the monitor's rolling windows; they are "
            "not raw whole-run percentiles.",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--since-unix-ms", type=int)
    parser.add_argument("--until-unix-ms", type=int)
    args = parser.parse_args()
    if (
        args.since_unix_ms is not None
        and args.until_unix_ms is not None
        and args.since_unix_ms >= args.until_unix_ms
    ):
        parser.error("--since-unix-ms must be earlier than --until-unix-ms")

    files = discover(args.paths)
    if not files:
        parser.error("no super-monitor JSONL files found")
    results = [
        summarize(path, args.since_unix_ms, args.until_unix_ms) for path in files
    ]
    report = markdown(results)
    print(report, end="")

    if args.out_dir:
        args.out_dir.mkdir(parents=True, exist_ok=True)
        (args.out_dir / "summary.json").write_text(
            json.dumps(results, indent=2, ensure_ascii=True) + "\n",
            encoding="utf-8",
        )
        (args.out_dir / "summary.md").write_text(report, encoding="utf-8")
    return 0 if all(result.get("valid") for result in results) else 2


if __name__ == "__main__":
    raise SystemExit(main())
