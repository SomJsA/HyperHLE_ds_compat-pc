# PC Transform::PostMultiply candidate

Windows x64 Dead Space only, default OFF. The pure SSE2 candidate has an explicit
original-ARM oracle. `TOUCHHLE_DS_PC_TRANSFORM_MATH=shadow` retains original output;
`host` allows output only after64real byte/full-FPSCR matches. Both require an
explicit `TOUCHHLE_DS_PC_TRANSFORM_TRIGGER` file, created after native playability.
Every256actual host calls return to original execution for comparison. Unsupported
inputs use the original; a mismatch disables the hook for the process.

Candidate selected from the actual Windows saved-IP profiles recorded in
`report/pc-jit-hotspots-20261104` at the project root. Original function entry is
`0x4ea204`, length108, full FNV64 `15179455414201691398`. Its first instruction is
`ADD r2,r0,#16` (`0xe2802010`), not a PUSH. It multiplies the16float matrices at
both object pointers+16 and writes only the left matrix, loading all inputs
before stores. Headers and any unrelated bytes remain untouched.

The candidate uses separate snapshots, preserves multiply/add order without
FMA/reassociation, saves/restores full MXCSR, and requires ordinary finite/zero
input bit patterns. Invalid/denormal/overflow/underflow outputs require native
fallback. The complete64byte output and full FPSCR sticky flags must match the
original. No guest, GL or Environment state is shared with a worker.

The ignored production-JIT oracle requires an explicit private
`TOUCHHLE_DS_TRANSFORM_ORACLE_FIXTURE` directory containing unchanged original
`postmultiply-arm.bin` (108bytes/fingerprint checked). No game code is in the
repository or distribution. The test covers384cases, full object arena and
guards, identical/partial aliases, non16byte aligned matrices, ARM/Thumb returns,
callee GPR/FPR, complete FPSCR and CPSR, and exact first-ADD replay. This is
component correctness evidence, never a whole-game FPS result.

The runtime hook replays the exact ADD before original-body execution and leaves
SP unchanged. Host output reproduces integer and all caller-saved SIMD register
values too; callee registers and full FPSCR/CPSR are preserved. Matrix snapshots
are fixed stack arrays; bounds are rechecked before committing all64bytes. No
workers, GL operations, screenshots or per-call allocations are introduced.

`perf transform-host on/off` in the isolated control inbox is test-only. OFF
restores the original first word and removes all observation; ON rearms only the
configured, fingerprint-matched function. A mismatch cannot be rearmed.

At implementation `50c936f5adc9af3eb175f24641bcc7fc69c7ddcf`, the extended
384-case oracle, including the actual host register-output plan, passed with all
GPR/FPR/CPSR/FPSCR values equal. The related23 tests and7 control-inbox tests
passed, followed by a normal clean-source release and default-off native smoke.

Two opposite-order same-process Store runs (16x6s each) measured incremental FPS
gains of1.79% and1.43% with position-skin host on in both arms.15/16 adjacent FPS
pairs were positive. Estimated CPU/frame moved-2.28% and+1.31%, so a CPU reduction
is not established; tail/maximum-frame results do not establish a consistent
hitch improvement. All raw intervals remain in the private evidence. JIT map,
PSS, GL spans, scene trace and stutter probes were off for these A/B runs;
essential diagnostics/native frame journal and the same external CPU sampler
remained enabled. This is one static scene on one RTX4090D/driver, not full-game
performance acceptance. Keep the candidate default off.

A separate assisted combat regression advanced the actual native clock by
299563ms, observed grab-to-live recovery and lease expiry, and retained zero
matrix comparison mismatches. The last counter report including post-expiry
recorded73083 original comparisons,1840 unsupported fallbacks and18692949 actual
host outputs. Assistance and scene/stutter diagnostics change that workload;
this run is correctness evidence, not unassisted FPS. Original IPA/seed/user
files were unchanged and owned processes exited normally.

Exact build hashes, per-arm results, image reviews and limitations are recorded
at the project root in `report/pc-transform-host-20261104`. The delivered PC v7
is unchanged.20-native-minute ENDLESS, later enemies/Bosses, full story and
streaming/cross-level/passive-hitch coverage, other hardware and complete R15
PC/Android/final manual gates remain open.
