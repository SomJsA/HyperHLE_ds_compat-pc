# PC position-skin SIMD host mode

Windows x64 Dead Space only. The production default remains OFF.

## Modes

- `TOUCHHLE_DS_PC_SKIN_SHADOW=1` plus `TOUCHHLE_DS_PC_SKIN_TRIGGER=<new file>`:
  validate the SSE2 candidate while always retaining the original ARM output.
- `TOUCHHLE_DS_PC_SKIN_MATH=host` plus the same explicit trigger:
  compare at least 64 real calls against the original ARM result, then allow
  bounded host acceleration. Every 256 accelerated calls execute the original
  function again and compare bytes plus complete FPSCR state.

The position function is `0x37fac8`, full 716-byte fingerprint
`10538268167844965500`; the 16-byte `GetData` helper fingerprint is
`5222870166602214087`. The original entry must remain ARM `PUSH`.

## Borrowed strided input experiment

`TOUCHHLE_DS_PC_SKIN_INPUT=strided` selects a new, independent opt-in input
candidate inside the explicit shadow/host mode. The default remains `packed`,
and all skin host output remains off by default. Strided mode reads bounded
immutable position/weight/index/palette byte views synchronously on the owning
thread, avoiding four input Vec allocations/copies. Output remains independent;
all input borrows end before native original execution or output commit. No
Environment/guest/GL state crosses a worker. Bounds, supported strides, palette
indices, ordinary float bit patterns and destination/input alias exclusion are
retained. The pure C++ kernel uses memcpy for unaligned byte inputs and preserves
the original SSE2 operation order and full host MXCSR.

The test-only inbox command `perf skin-input packed/strided` resets that mode's
warmup to64 real original comparisons on every request. Only then can it output;
every256 actual host outputs require original comparison again. Unsupported
calls never consume/reset the recheck budget. Any mismatch permanently disables
the process's skin hook, including attempts to rearm using another mode. Both
inbox activation and mode selection require initial explicit skin configuration.

The legacy `accelerated` field now counts only actual committed host outputs;
older builds also advanced its recheck index on successful original comparisons.
New `DSpc-skin-input` records state the mode, mode comparisons, actual host_calls
and remaining recheck budget. They are counts, not CPU time. Existing nested wall
timing is retained for research only. Component/original oracle, live64 matching
and observer-off same-binary packed/strided A/B are separate evidence scopes.

At implementation `f6c912eea6a1a3033753e382d7ddf35481942490`,25 related production
JIT tests passed, including192cases/36864vertices of packed and strided outputs
and fullFPSCR matched against original ARM, new unaligned/bounded-input negatives,
and the prior matrix oracle.8 current-source inbox tests also passed. The initial
inbox run had one incorrect test expectation for a different command reusing a
sequence; the existing protocol correctly rejected it as stale. Failed logs are
retained, and the corrected-source executable is hashed in the report.

Normal full release SHA256 is
`295eb457d323b00e9986c0dc0942bcda410fbd5621006cfba59af0005daa0b74`.
Default-off native smoke passed with8shots/ammo20to12 and actualMSAA4/AF4.
Two opposite-order16x6s packed/strided A/B runs kept host skin on in both arms.
Strided FPS increased2.08% and1.67%, with estimated mainCPU/frame changes-2.83%
and-4.75%;12/16 adjacent FPS pairs were positive.36 per-request warmups each
required fresh64 real matches. All raw long intervals and unfavorable pairs
remain. Median per-arm maximum frames worsened31.89% and9.65%; there is no
passive-hitch improvement claim. The largest strided raw intervals were17.320ms
and14.209ms, versus packed13.439ms and10.136ms. These static windows had no
33.333ms/50ms frame, which is not broader hitch acceptance.

The single Store scene/RTX4090D evidence has JIT map/PSS/transform/GL spans/
scene/stutter probes off, with equal essential diagnostics/native frame journal
and external10Hz CPU sampling. CPU/frame is approximate. Diagnostic counter
deltas within a mode show input preparation wall cost around12us/call packed
versus2.4us/call strided, including settling and periodic original checks;
nested wall spans are not exclusive CPU or exact FPS windows.

A separate assisted strided-host combat regression advanced actual native time
298361ms. Last counters including post-expiry had1869 comparisons,15 unsupported
fallbacks,462080 actual host outputs and zero mismatches. Grab-to-live recovery,
health intervention and lease expiry were observed. Normal exit persisted the
queued diagnostic tail, and original IPA/seed/user files remained unchanged.
Assistance and scene/stutter probes alter this workload; it is correctness
evidence, not unassisted performance or20-native-minute/later-enemy/Boss coverage.

Keep skin host OFF and input default PACKED. Exact build/test hashes, per-arm
results, unfavorable maximum frames and image/clock/integrity reviews are at the
project root in `report/pc-skin-strided-20261104`. PC v7 and Android runtime and
installation are unchanged. Full R15 and final manual acceptance remain open.

The same native implementation later completed an actual1200120ms assisted
ENDLESS run, with5920 original comparisons,67 unsupported fallbacks and1499136
host outputs last logged, zero mismatches, grab-to-live recovery, lease expiry
and normal game exit.240 bounded5-second snapshots observed six verified enemy
classes (Slasher,Lurker,Exploder,Pregnant,Swarm,Brute) and a peak24 known alive
enemy objects; snapshots can miss peaks between observations and are not kills
or Boss coverage. Explicit F7 issue0001/0002/0003 JSON+PNG saved at native~5/10/15
minutes; actual images and hashes were checked. Original IPA/seed/user files
remained unchanged. See `LONG-ENDLESS.json` in the same report directory.

All117080 raw active-window intervals remain, including55 above50ms and15
above100ms. The three largest coincide with F7 capture/wait transitions and
matching explicit dump waits of1018125/869729/921083us; other long frames remain
unexplained. Assistance,scene/stutter probes,CPU sampling and detail throttling
alter this workload; its inclusive observed presentation rate is not unassisted
FPS or an improvement A/B.

The normal diagnostic writer reported `queued_tail_not_persisted=2` despite no
writer error or queue drops. This is an open completeness defect: its STOP path
returns after one message rather than draining the queue, with producer shutdown
races also requiring a bounded admission/close protocol. Do not mark collector
shutdown/F7 export completeness passed from this long run. Fix and test ordinary
and Problem/Picture senders, bounded drain and native exit separately. The20min
game/host stability evidence remains scoped and all broader R15 gates stay open.

## Safety

- Captured spans, strides, aliases, vertex count (maximum 32768), indices and
  matrix bounds are checked before acceleration.
- Nonfinite/subnormal inputs, invalid indices or FP exception outputs use the
  original ARM function.
- A mismatch immediately restores the original entry and disables observation.
- Host MXCSR is restored. Candidate output and FPSCR sticky flags must match the
  original exactly. DAZ is not used for input classification.
- No Environment, guest pointer, GL state or input ownership crosses a worker.

`perf skin-host on/off` through the isolated control inbox is test-only. OFF
restores the original entry and removes all observation; ON rearms the verified
entry. It exists only for same-process A/B.

## Evidence

- Production JIT oracle: 4 tests passed, 192 cases / 36864 vertices, full output
  bytes, FPSCR, callee registers, guards, strides and host MXCSR.
- Real host run: 132097 accelerated calls with 0 mismatches and 0 unsupported.
- Later same-process host/original runs: about +5.4% and +15.6% FPS in the two
  120/240-second arms, with main-thread CPU about -5.1% / -5.7%. A later host
  arm retained 115969 accelerated calls, 15 unsupported fallbacks and 0
  mismatches.

These assisted diagnostic results justify the opt-in mode, not a production
default or a claim of full-chapter stable FPS. Full chapter, streaming, passive
door/elevator hitch and final hardware acceptance remain open.
