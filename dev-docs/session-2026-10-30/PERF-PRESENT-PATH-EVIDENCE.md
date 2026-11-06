# Submitted presentation-path evidence (R10)

A requested switch is not proof that its rendering path ran. The Windows
monitor now adds `present_path_schema: 1` alongside these observations:

| Field | Meaning |
| --- | --- |
| fsr_requested | Parsed spatial FSR mode, 0/1/2 |
| fsr_source_redundant | The source meets the existing 115% area gate |
| fsr_eligible | The FSR request passes the backend/source gates |
| fsr_final_submitted | The final EASU presentation route returned success; a two-pass route needs both passes |
| present_shader_submitted | A postprocess shader path returned success; plain sharpening can succeed after EASU fails |
| gpu_copy_requested | The runtime copy switch |
| gpu_text_requested | The runtime card-renderer switch |
| gpu_text_gate_enabled | Request/platform/session policy allows GPU cards |
| gpu_text_effective | This present call selected its attached GPU card state and remained enabled; CPU/CA calls have no such state |
| host_text_card_submissions | Lifetime `[CPU, GPU]` card-call counts |
| direct_present_transfer_submissions | Lifetime `[CPU readback/upload, GPU copy]` direct ES1 transfer-call counts |

Counters update at the actual call sites, with diagnostics enabled, without new
GL queries, fences or readbacks. Zero cards can mean there was no host text.
Diagnostic HUD/input badges do not count as host text cards. Direct-transfer
counters do not cover CA composition, every backend or every guest upload.
These are submitted calls, not GPU completion, accepted pixels, successful
driver execution or image-equivalence proof. Internal captures remain required.

The EASU flag is set only on its successful final route, not when a later plain
sharpen fallback succeeds. An FSR pass requires its FSR uniform. MSAA refreshes
from the receiving EAGL context at present, including zero for a new ordinary
context; it is that context's queried allocation count, not proof that every
guest FBO/draw uses MSAA.

## Report rules

New path fields require schema 1 and consistent typed flags/counters. Missing
new fields, impossible flags and counter regressions are rejected. Actual FSR
and shader quality remain constant inside and across compared samples. The
named treatment alone may vary: copy-request for GPU-copy, text request/selected
path for CPU/GPU-text. Other mode or quality changes remain confounds.

New GPU-copy logs require positive transfers in each requested path, with none
in the opposite path. Legacy GPU timing stays readable, with its separate
native-path evidence requirement. Text pairs require actual CPU cards in the
baseline and GPU cards in the treatment, no opposite-path work, and matching
requested/selected flags. Empty scenes or CPU fallback cannot establish GPU
text performance. Neither counters nor flags prove matched scenes/captions.

The diagnostic HUD distinguishes the FSR request from EASU submission. A gated
or failed path must not be reported as an effective FSR quality-matrix cell just
because its request was 1 or 2.

## Shader timing scope

All real Rust GLES backends time completed explicit CompileShader/LinkProgram
calls. Native GLES1 unsupported stubs are excluded. The GL1-on-GL2 source has no
additional explicit internal shader compiler outside those entry points; its
fixed-function driver work is outside these timers. Driver-internal work,
C/Java paths and calls still blocked/in flight are not established by zero
completed API counts. JSON records this scope explicitly.

48 offline report tests cover the new counterexamples, including equal FSR
requests with different submitted quality, empty text scenes and CPU fallback.
The isolated source runs DS, data-root and frame-pacing checks. This is source
and measurement preparation; new loading, quality, GPU text and driver runtime
acceptance remains pending. R7i human binaries and the reserved tablet are not
changed by these checks.

The quiet runner still needs scoped integration. A whole-run summary spanning
intentional treatment switches is expected to be configuration-invalid; its
exit code must not replace the matched-pair result. Correct that postprocessing
and verify private diagnostics cleanup before running a new native experiment.
