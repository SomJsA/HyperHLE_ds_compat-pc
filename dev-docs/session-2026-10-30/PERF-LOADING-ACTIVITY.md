# Loading-state texture synchronization evidence (R9)

The steady-state TEX samples did not exercise synchronization. A zero Off
timer count is also ambiguous: Off returns before creating that timer. These
facts cannot establish a loading-state benefit or justify changing Finish.

## Native observation

With PERF_TRACE or SUPER_MONITOR enabled, the guest texture upload sync hook
increments process-lifetime atomic counters before Off's early return. No new
GL query, finish, wait, framebuffer readback or payload hashing is introduced.
Normal runs with both diagnostics disabled skip counter updates.

The 2 Hz monitor adds:

```json
{
  "texture_sync_submission_schema": 1,
  "texture_sync_submissions": {
    "off": [12, 3],
    "flush": [0, 0],
    "finish": [25, 7]
  }
}
```

Each row is `[payload_submissions, non_payload_submissions]`. A non-null,
positive-area submission qualifies as payload; compressed calls also require
positive image_size. NULL/zero-area calls remain separately observable. The
mode is read by the same hook that chooses Off/Flush/Finish, so counts are
independent of the optional timing block.

These are submitted guest calls reaching this hook, not accepted/completed GPU
work, uploaded byte totals, unique textures, or all HD/decode/mip/helper paths.
Some replacement paths return without this sync hook; their absence must not
be misreported as no uploads anywhere. Rendering/error-log acceptance remains
separate.

## Analysis and measurement

The summarizer checks schema, complete nonnegative integer counters and
monotonicity. It reports first-to-last observed deltas for each mode. Missing
legacy fields are explicitly unavailable; partial/invalid new fields and
counter regressions are rejected. Legacy GPU-copy timing remains readable.

A texture pair requires payload deltas in both requested modes, no work under
another mode inside either sample, and actual sync calls for Finish/Flush.
Off requires no sync-timer calls. Positive allocation counts alone are not a
loading workload. Both arms of a rejected cycle leave the pooled comparison.

Before triggering the load, obtain a fresh native monitor snapshot and retain
it as the start boundary. Include the whole transition and at least one
post-load snapshot. Starting only after an action ACK can omit the load itself.
Interior stalls must remain in elapsed time; missing edge coverage cannot be
salvaged by reporting only the fast tail. A fast load entirely between snapshots
still needs a boundary before and after its submissions.

Use repeated counterbalanced transitions from verified private save fixtures.
Record the actual scene/transition, applied mode, payload counts, shader/cache
state and internal before/after captures. Positive counters establish activity,
not equal workload or visual correctness. Do not compare unrelated cold starts
or pool changed source scale, MSAA, renderer or other quality settings.

36 offline Python report tests cover the counterexamples and valid Off/Flush
fixtures. Rust tests cover per-mode accounting, non-payload separation and
concurrent totals; source integration checks cover the real hook/monitor wiring.
This instrumentation is preparation for loading-state A/B, not its completion.
Keep production Finish, GPU copy off and GPU text off pending the remaining
runtime/driver/quality/physical acceptance. Tablet ADB remains reserved elsewhere.
