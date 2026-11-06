# Performance report validity (R8, 2026-10-31)

The reporters validate observed timing. They do not establish that a native
mode was applied, that both arms exercised the same scene/camera/workload, or
that rendering and another driver passed. Keep those runtime proofs separately.

## Regressions that must remain fixed

- An explicitly bounded A/B arm must never discard a configuration transition
  as startup. Only an unbounded whole-log inspection may trim a short startup.
- Both requested edges need monitor observations within 1,000 ms (the normal
  monitor interval is 500 ms). A missing tail is incomplete evidence. An
  interior stall remains part of elapsed time and lowers FPS; do not discard it.
- Preserve append order. Reject timestamp regressions, invalid frame counters,
  frame counter resets, and observed stage counter resets. Sorting timestamps
  or subtracting only the first/last counters can hide a reset.
- Run ID, source size, viewport, requested/actual MSAA, anisotropy, and renderer
  must remain stable inside and across compared arms.
- Stage snapshots persist when no new calls occur. Report no mean/maximum for
  an inactive stage; expose the call delta separately from its lifetime count.
- Compare complete baseline/treatment cycles. If either arm is invalid, exclude
  both from medians. Missing cycle IDs must not invent pairs. Report per-cycle
  changes as well as medians. An incomplete run returns CLI exit code 2 even
  when other complete pairs remain available for inspection.
- GPU-copy arms require observed GPU-present calls. Texture-sync comparisons
  require actual baseline sync work, and Flush also requires treatment calls.
  Off bypasses the sync timer: zero Off calls cannot prove zero uploads. Both
  arms now require independent schema-1 payload submission deltas in their
  requested modes, with no submissions under another mode inside the sample.
  Off must have zero sync-timer calls. Allocation/empty calls alone do not
  qualify. Legacy timing logs remain readable, but lack this texture evidence.

The monitor contains rolling p95/p99 and overlapping stage windows, not raw
whole-run percentiles or exact per-arm stage totals. Windows can straddle an arm
boundary; call deltas cover the analyzed snapshots. FPS uses the observed frame
and wall-clock interval, with requested-edge gaps recorded explicitly.

## Verification

```powershell
python tools/pc-runtime/test_perf_reports.py -q
```

48 standard-library tests cover bounded configuration changes, edge coverage,
interior stalls, frame/time/stage resets, stale stage maxima, missing metrics,
matched cycles, configuration confounds, incomplete runs, unexercised sync,
Off payloads without timers, non-payload calls, and missing/reset/wrong-mode
submission telemetry, submitted presentation paths and effective-quality
confounds. See [PERF-PRESENT-PATH-EVIDENCE.md](PERF-PRESENT-PATH-EVIDENCE.md)
for schema-1 presentation requirements and shader-timer limits.
They do not launch the emulator or access ADB.

The original 2026-10-30 paired manifests and monitor logs were copied for
reanalysis; source SHA-256 values were checked unchanged. Local evidence is in
the workspace `perf-continuation-20261030/R8-OFFLINE-PERF-AUDIT`, including
synthetic before/after reports and `historical-reanalysis.json`.

| Historical run | Result under these checks | Interpretation |
| --- | --- | --- |
| PAIRED-GPU-rep1 | 3 complete pairs; off 115.60 FPS, on 158.37 FPS; paired median change +36.80% | Timing evidence remains valid for the recorded scene and machine. It does not complete cross-scene/driver acceptance or authorize a default change. |
| PAIRED-TEX-rep1 | 4 cycles rejected: `texture_sync_work_not_observed` | Steady-state samples did not exercise sync work. This is insufficient experiment evidence, not an engine failure. Loading-state A/B remains pending; retain Finish. |

GPU copy remains off in the human acceptance preset. No runtime binaries,
human test saves, running launcher, or original evidence were changed by this
offline reporting work.

The later R9 schema and loading-measurement requirements are documented in
[PERF-LOADING-ACTIVITY.md](PERF-LOADING-ACTIVITY.md). New instrumentation and
report checks do not complete loading-state A/B or visual/driver acceptance.
