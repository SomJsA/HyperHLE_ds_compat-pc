# Runtime performance source integration (R9)

The working runtime already contained performance switches, caches and an
experimental GPU text path. They were not a complete committed dependency
chain. This integration selects that chain without staging unrelated blood,
texture-audit, backend, capture-window or formatting changes.

## Runtime behavior

- Control inbox switches GPU present copy, texture Finish/Flush/Off, CPU/GPU
  text and frame caps within the process. No host keys or foreground injection.
- Timers expose CPU call duration rather than pretending to measure GPU time.
  The opt-in monitor records the real GL renderer and adapter-wide NVX memory
  only when supported, plus frame and stage snapshots at 2 Hz.
- Schema-1 texture submission counters remain observable under Off. They
  distinguish payload from NULL/zero-area calls and identify the mode chosen
  at the same sync hook. See [PERF-LOADING-ACTIVITY.md](PERF-LOADING-ACTIVITY.md).
- CPU cards use cached transposed pixels and retain the reference blending and
  placement. GPU cards share those cached images, remain opt-in, and fall back
  to CPU after upload failure or on an unsupported present backend.
- Native menu/Store/Bench/dialog ownership suppresses both card drawing and
  unnecessary CPU composition for hidden gameplay text. A visible CPU card
  requests composition even without an explicit force-composition option.
  EAGL and the CA fullscreen-layer decision share that policy.
- CA readback texture storage is reused at unchanged dimensions and invalidated
  when its source changes. The accepted direct-present stale-pixel repair stays
  intact. FSR retains its redundant-source-area gate.
- Absolute frame deadlines subtract presentation/vsync time from the remaining
  sleep budget. Disabling the cap resets its schedule.
- The optional host data-root helper stays at module scope so Cargo discovers
  its isolation tests. Relative/empty overrides are rejected; the default and
  Unicode absolute paths remain intact.

## Verification and limits

A private index based on 40fedbcc produced an isolated source archive. The final
check ran **108 DS tests, 3 data-root tests and 2 frame-pacing tests**, all passing.
The extra filters matter: DS-only filtering does not cover the latter two
modules. The initial function-local data-root module hid its tests; moving that
module fixed discovery. Final source and index are checked byte-for-byte after
CRLF normalization. Existing native wrapper/vendor paths were shared to reuse
the build cache; this is not a clean-vendor or second-driver certification.

The Python reports pass **36 offline tests**. Historical raw logs were copied
and reanalyzed without changing their hashes: GPU copy retains 3 valid timing
pairs; all 4 TEX cycles lack independent payload evidence and sync work, so none
qualify. No new loading-state FPS or default-policy conclusion follows.

Evidence is in the workspace `perf-continuation-20261030/R9-PERF-INTEGRATION`.
Build/test executables were used only for source checks; the R7i human runtime,
launcher and saves were not replaced. No game, VM or ADB operations occurred.

Production presets retain Finish, GPU copy off and GPU text off. Visible GPU
card comparisons, internal captures, complete loading transitions, effective
quality/FSR verification, the paired quality matrix, other GPU/driver checks,
physical input and the final PC/Android release remain required. Request labels
alone do not prove an effective GPU text/FSR path or a matched loading workload.
The uncommitted quiet runner and remaining launcher/renderer changes still need
their own scoped integration; this source commit is not a release candidate.
