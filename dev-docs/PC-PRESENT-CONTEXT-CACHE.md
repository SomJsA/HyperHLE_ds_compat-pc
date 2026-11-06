# Presentation cache ownership (2026-11-04)

Player report: after accumulated long6x play, changing to4x can start with
content rotated90degrees inside the same display area. Exact tester build,
checkpoint and10h reproduction are not available; that acceptance remains open.

Confirmed defect: `present.rs` kept shader/program names, uniform locations,
FSR textures/framebuffers and failure flags in thread-local storage. Guest EAGL
and the Core Animation compositor use different actual GL contexts on the same
thread. A program name may be invalid or identify another program there; FBO
attachments are context-local even in a shared object group. At1440p output,
6x bypasses redundant FSR while4x activates it, changing the affected path.

`PresentState` now belongs to the actual context owner: Window's internal
context, each EAGLContext, or Android's private shader context. Splash and CA
composition share only their same internal-context state. Android recreates
state with its helper context and releases GL resources only while that owner
is current. No guest/GL/Environment data is sent across threads. No forced
orientation, quality reduction, preference reset or save change is used.

Evidence: `E:/CodexProjects/DeadSpaceLegacy/report/pc-orientation-20261104`.
Old realGPU tests fail deterministically: missing FBO attachment0x8cd7 and
`IsProgram=0` in the other context. New tests pass both and15 pixel-rendered
cases /60 quadrant readbacks across two contexts,6x/4x/6x andFSR0/1/2.

Run serially on a real Windows GL driver, with a hidden owned window:

```powershell
$env:TOUCHHLE_DS_BACKGROUND_TEST='1'
$env:TOUCHHLE_DS_OUTPUT_WIDTH='320'
$env:TOUCHHLE_DS_OUTPUT_HEIGHT='240'
cargo test --offline --locked --release --lib pc_present_context -- --ignored --test-threads=1 --nocapture
```

Oldv7 also produces white frames during three explicit native locator/CPU
caption requests at4x+FSR; the new normal build renders upright native scenes
and the actual objective card. Five restart cases with shared private save,
15 locator requests, actualMSAA4/AF4 and both6x/4x are recorded. Protected IPA,
seed, player files and old package remain byte-identical. This is scoped
rendering correctness, not10h/full-story acceptance or a single-core gain.

Native implementation4225060d; Android local port4de15224. Both normal complete
release builds pass; Android also passes API21 arm64 typecheck. Android APK is
not installed or device/lifecycle accepted. Current reports retain exact build
hashes and distinguish later documentation commits from compiled inputs.
