# Opening docking-bay depth fix (2026-10-21)

## Symptom

At approximately 15-16 seconds in the opening camera, large docking-bay
surfaces on the right side of the frame alternate between black and bright
rectangular bands. The defect becomes more obvious while the camera moves.
It is separate from the long bridge/tex57 issue seen at approximately
26 seconds.

## Root cause

The decisive texture is content key `25a08a82`. It is shared by the opening
space/nebula and the large docking-bay panels. Its draw data contains
out-of-range texture coordinates. With `GL_REPEAT`, those coordinates pull
unrelated atlas cells into the large platform surfaces and create black/bright
bands. `CLAMP_TO_EDGE` removes that atlas wrap.

The same key also contains mixed batches: full-atlas background triangles and
sub-rectangle scene geometry are submitted together. A global LESS/LEQUAL switch
therefore cannot satisfy both. The replacement path splits indexed triangle
batches by triangle UV coverage:

- full-atlas space triangles use LEQUAL without depth writes;
- sub-rectangle scene triangles currently use LEQUAL, keep depth writes, and
  receive a one-unit forward polygon offset.

The split is geometric and frame-independent, but it is still a candidate:
scene triangles using LEQUAL can overwrite another coplanar scene triangle,
which is the same failure mode that caused the earlier global-depth regression.
That point must be tested against the official recording while the camera
moves before this path is considered final.

## Draw-level correction (2026-10-22)

The per-pixel/UV probe at the registered 15-second camera pose corrects an
earlier assumption. The guest does *not* submit these docking-bay triangles
with `GL_LESS`: the observed platform batches already use `GL_LEQUAL` (0x203),
normal alpha blending, depth writes on the first pass and off on a second pass.
Therefore the triangle split is not repairing a global `LESS -> LEQUAL` error.

At frame 1400, all four probed platform points resolve to content key
`25a08a82`. Representative scene triangles have UVs such as
`u=0.7126..0.7243, v=-0.7826..-0.7600` and NDC z around `1.0027..1.0029`;
other covering triangles span much larger off-screen coordinates. This confirms
that the platform is rendered from many overlapping sub-rectangle triangles in
the same atlas, not from one simple quad.

The diagnostic `TOUCHHLE_DS_TRI_DEPTH_TRACE=1` now records whether the split
actually executes and what it classifies. On the VM opening run it reaches
`split active`; no VBO texture-coordinate rejection was observed. The remaining
question is therefore which scene triangle should own each pixel, not whether
the mixed-atlas split is being skipped.

`TOUCHHLE_DS_TRI_DEPTH_SPLIT_SCENE_FUNC=original` and
`TOUCHHLE_DS_TRI_DEPTH_SPLIT_FORWARD_UNITS=0` are VM-only A/B controls. The
shipped `2247` default remains unchanged.

A denser 30-frame capture spanning guest frames 1200-1800 was also run against
the tri-split path. The resulting 21-frame contact sheet is continuous: no
frame-number-triggered black/bright rectangle appears after the camera starts
moving. Evidence:

`F:\CodexProjects\DeadSpaceLegacy-package-staging\stripe-diagnosis-20261021\tri-split-dense-20261022\dense-contact.jpg`

This is VM/Mesa evidence only. The user's PC GPU path and Android remain
unconfirmed, so `2247` is still marked as a test candidate.

Earlier global-depth experiments are retained only as diagnostics.

## Earlier diagnosis

The compatibility layer forced every `GL_LESS` draw to `GL_LEQUAL` to recover
a coplanar stack of space-background quads. On a docking-bay platform, that
global override lets later coplanar surfaces overwrite earlier correct ones.
The result is the moving black/bright banding.

Texture probing at the affected screen points showed several overlapping
draws using content keys `25a08a82`, `9d49e3d6`, `7bcfa36c`, `780d00eb`, and
`f4d178fa`. The defect disappears with depth testing disabled and also with
the game's original depth function. This isolates the issue to depth ordering,
not texture decoding or mip generation.

## Fix

- Preserve the game's original depth function by default on GL2 and native
  GLES1 paths.
- Keep `TOUCHHLE_DS_DEPTH_LEQUAL=1` only as an explicit legacy compatibility
  switch.
- Do not enable the global workaround from Android/desktop PC-compatibility
  defaults.
- Keep tex57/58 LOD bias opt-in; it masks texture detail and is not required
  for this defect.

## Verification status

The first packaged attempt (`1610`) is a known regression: it removed part of
the docking-bay banding on the VM capture but broke starfield layers on the
user's PC. It must not be distributed as the current build. Investigation
continues with draw-level depth state and texture sharing, since content key
`25a08a82` is used by both the nebula and docking-bay batches.

A VM llvmpipe A/B at the same registered camera time (official reference
approximately 16 seconds) shows:

- Before: alternating white/black strips on the right docking platform.
- Fixed: continuous gray surfaces and normal geometric seams.
- With `TOUCHHLE_DS_DEPTH_LEQUAL` removed from the environment entirely,
  the rebuilt binary still renders the fixed platform correctly. This verifies
  the compiled default, not just an explicit test override.

Comparison image:

`F:\CodexProjects\DeadSpaceLegacy-package-staging\stripe-diagnosis-20261021\opening-16s-depth-fix-boxed-aligned.jpg`

The previous withdrawn diagnostic package is:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261021-1610.zip`

SHA-256:

`d365509d864f90350fca4a87f541342b051b46a9629cb85e30c45c1cb8684493`

The current remote-test candidate is:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261021-2247.zip`

SHA-256:

`b6af163b4c58ff3e8592f9285b429694aff5ef384a9f6222e32e3d78438886af`

VM default-start captures show the platform substantially cleaner at
14-18 seconds while nebula/planet layers remain visible at 35-40 seconds.
User-hardware confirmation is still required before calling this final.

## Hardware result for 2247/0229

User-PC testing of `0229` (which carried the same opening renderer defaults as
`2247`) did **not** pass:

- one small horizontal strip improved;
- the distant starfield regressed, including a black ring during the first ten
  frames of the opening;
- other similar cut lines became visibly worse.

Therefore the default tex55 clamp and triangle-level depth split are disabled
again. `TOUCHHLE_DS_TEXWRAP_HASH=25a08a82:edge` and
`TOUCHHLE_DS_TRI_DEPTH_SPLIT=1` remain explicit diagnostics, but neither is
enabled by default. The PC/Android launch paths return to the pre-2247 renderer
behaviour while retaining the separately validated scene-interaction fix.

## Input-fix candidate (2026-10-22)

The next PC test package keeps the opening render path from `2247` and adds the
VM-verified scene-interaction fallback:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261022-0229.zip`

SHA-256:

`e45b8bf71d61f5890555c8bb54fa6452ccac3c9b5ce6020a996f5ee79e580db9`

The `0229` package remains archived as a failed A/B result. New packages must
not reuse its renderer defaults.

The rollback test package is:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261022-0305.zip`

SHA-256:

`a225502dea0a3b5a4272b55b047897873075fbb92366af766ed63a5063a231db`

It restores the old opening renderer behavior and keeps only the validated
scene-interaction selector. The original docking-bay strip may return; this is
a control build for isolating the starfield and cut-line regression.

Bundled executable SHA-256:

`653b7eb71c4e97a2fc0f6893018da7f77bf129b7bdbbd95cc1ee263a9e8e56b3`

Bundled launcher SHA-256:

`342a189961e75271f91fcf8d6b82f1ed01e04b9702369cf7cdc261a894c6ed61`

## Deferred input task

Recorded for the next input-compatibility pass:

- `E` is unreliable for doors, shops, workstations, and consoles.
- `F` does not trigger kinesis.
- `E` melee responds normally.
- QTE behavior has not yet been retested.

These input issues are not part of the opening depth fix.
