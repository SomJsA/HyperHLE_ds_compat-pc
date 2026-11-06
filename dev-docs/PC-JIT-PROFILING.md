# Optional Windows JIT hotspot attribution

The runtime diagnostic is OFF unless `TOUCHHLE_DS_JIT_MAP` names a new file before
the first CPU is constructed. It records emitted host-code ranges and their ARM
block labels, not guest bytes, compilation durations, function CPU time or FPS.
No guest/Environment/GL state is sent to a worker.

## Journal

The Windows x64 Dynarmic sink is a process-lifetime callback installed before Jit
construction. Unconfigured builds install no callbacks. Linux's existing
perf-map implementation and other platforms remain unchanged.

The host journal uses `CREATE_NEW`, never overwrites an existing file, stops at
16MiB, and disables itself on I/O errors. It does not create directories. Names
are bounded to160bytes and control characters are sanitized. One mutex serializes
complete records; normal gameplay never polls the path. A failed diagnostic
must not alter guest arithmetic or instruction execution.

Tab-separated records:

| Record | Fields |
| --- | --- |
| H | schema2, PID, QPC frequency, process creation time in100ns units |
| M | QPC, epoch, host start hex, byte length hex, friendly block label |
| C | QPC, new cache-clear epoch |
| L | QPC, epoch, byte count before the journal stopped at its limit |

All previous mappings are discarded on C. New overlapping ranges supersede
only their overlap; the analyzer never applies a future record to an earlier
sample. Clear/overwrite transitions overlapping a context-capture interval are
classified as ambiguous. Coverage after L is unknown. A cache clear can also
remove still-valid dispatcher/helper labels from coverage; unknown is safer
than retaining stale attribution. Multiple Jit instances are not separately
identified, so this tool currently targets a single owned game process/CPU.

The PC submodule extension adds only nullable Win64 emitted-code callbacks.
The prior Win64 exception ABI fix remains in its ancestry. Android's Dynarmic
pin and runtime are unchanged. The explicit PC project-guard override records
both changes; it is not a resampled resource or lighting baseline.

## Saved-IP sampler

Build standalone tools into a fresh directory:

```powershell
& tools/pc-runtime/jit-profile/Build-JitProfiler.ps1 -OutputDirectory <NEW_DIRECTORY>
$env:DS_JIT_PROFILE_TOOLS='<NEW_DIRECTORY>'
python -m unittest discover -s tools/pc-runtime/jit-profile -p 'test_*.py' -v
```

`PcJitSampler PID CREATED_100NS SECONDS PERIOD_MS OUTPUT EXPECTED_EXE` verifies
the exact process creation time and executable before creating its output. Use
only a child owned by the isolated test controller. Duration is1–120seconds,
period10–1000ms, and output must be new. It observes the earliest-created thread
and retains missing captures and cleanup errors. It never changes priority,
injects input/code, reads guest memory, or manually calls SuspendThread.

The tool uses Windows Process Snapshotting to capture control contexts, copies
only RIP/thread timing metadata into the journal, and frees each walk marker
and snapshot before the next delay. Microsoft's contracts describe
[context capture](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-psscapturesnapshot),
the [context pointer lifetime](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/ns-processsnapshot-pss_thread_entry),
and [freeing a locally owned snapshot](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-pssfreesnapshot).
The standalone observer requires Windows8.1 or newer. It is not added to the
game launcher or normal release requirements.

These are wall-clock saved instruction pointers, including waits, preemption
and snapshot perturbation. Counts are **not on-CPU shares or exclusive time**.
Native module names use observer inventories rather than a complete loader
history. Use counts to select candidate hotspots, then verify their native
semantics/call cost and measure actual changes in separate observer-off A/B.
Do not certify performance from a run with this observer attached.

Join completed bounded journals with:

```powershell
python tools/pc-runtime/jit-profile/analyze_jit_samples.py --map <MAP> --samples <SAMPLES> --native-log <STDERR_LOG> --output <NEW_JSON>
```

PID/creation-time/QPC identity, cache epochs, complete records, successful observer footer and
zero snapshot cleanup errors are required. I/O-disabled runtime maps reject
analysis. Complete ARM-block and JIT-label histograms are retained; top30 is
only a display summary, never the cutoff for function aggregation. Raw unknowns,
limits and ambiguity remain in the result. ARM labels
identify basic-block starts, not automatically a containing game function.

Boundary tests cover exclusive files, disabled/error paths, concurrent complete
records, the16MiB cap, actual PSS capture on an owned busy child with CPU progress,
creation-time rejection, partial address overlap, cache clears, future mappings
and diagnostic truncation. Real native evidence and all final R15 gates remain
separate requirements.
