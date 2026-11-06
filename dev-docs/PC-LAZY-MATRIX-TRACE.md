# Lazy formatting for optional matrix traces

The current111a4844 GPU-renderer saved-IP profiles repeatedly observed the host
float formatter. Those saved instruction pointers do not identify its callers.
Static inspection independently found eager diagnostic work: twelve direct
matrix-trace calls constructed Strings before checking whether a trace output
path or its dynamic trigger existed. Four LoadMatrixf/MultMatrixf sites format
sixteen floats each even when both trace outputs are disabled.

The two private log writers now receive synchronous fmt::Arguments. They check
the existing path and live marker, open the output, and only then format the
message into a String and append the original line. All twelve calls use
format_args. No GL call, matrix math, pointer ownership, guest value, render
setting or trace-line syntax changes. The marker remains a live filesystem
check; it is not folded into the existing per-run environment configuration.
Formatting arguments never cross a thread or outlive the synchronous call.

src/gles/trace_file.rs is used by the actual presenter and also builds as a
standalone Rust test target. Four tests use a Debug implementation that counts
evaluation: no output path, missing trigger and failed file open cause no
formatting; a configured output appends identical UTF8 lines while marker
removal/recreation stops/restarts formatting in the same process. These are
observable formatting/output tests, not an estimate of whole-game performance.

Normal full release compilation and native regressions are required. Measure
the previous111normal binary and the new normal binary with identical private
profiles, fixed settings, settled native scenes and opposite-order A/B windows.
Keep raw frame intervals, processCPU brackets, defaults and long-frame limits
explicit. No FPS gain, passive-hitch fix, multicore/structural single-core
resolution, Android acceptance or full R15 completion follows from this source
change or from saved-IP formatter counts alone.

The first four-process normal-binary ABBA comparison is retained with unfavorable
defaultCPU results and a framing caveat; it does not accept this optimization.
For the next controlled experiment, the explicit private control inbox accepts
`perf matrix-format eager` or `perf matrix-format lazy`. Normal process state is
lazy. This changes only optional diagnostic argument formatting. Literal callers
retain their original no-allocation behavior; eager replay applies only to the
former format! callers. Both modes write identical configured lines once.
No file trigger or game/GL state is changed by the command.

One actual formatted call after a request reports the effective mode, whether
the message was formatted before its output gate and whether an output path is
configured. Stale in-flight requests cannot acknowledge a newer mode. The
controller must wait for this proof and settle outside measured windows; a
command ACK alone is insufficient. Six actual formatting/output tests and the
bounded command parser tests cover the calibration. It remains an experiment,
with actual same-process native CPU/FPS/tail validation required.

Current evidence is at the project root in
report/pc-lazy-matrix-trace-20261105/STATUS.json; attribution evidence remains
separate at report/pc-current-jit-hotspots-20261105/STATUS.json.
