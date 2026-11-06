# Dead Space PC opening render: current analysis

Date: 2026-10-24

## Scope

This pass isolates the remaining structural rendering failures in Chapter 1:

- regular diagonal scan lines / triangular cuts across the opening station hull;
- the large opaque black polygon next to the player in the porthole room;
- porthole interior brightness before gameplay control;
- smooth distance fog and the safety poster, without regressing the starfield.

Font, text rendering, and character/armor material differences are out of scope.

## Current repository and runtime state

- Source branch: `codex/android-mainline-merge`
- Source HEAD at the start of this pass:
  `419f8e2c diag: reset pixel trigger state at frame boundaries`
- Runtime currently restored to the known-working binary:
  - title: `Dead Space (touchHLE 173574de)`
  - SHA-256: `888A2DD548969748FBC26CFE240F495FC8FCC3ABA4F2D509E6A102332F233264`
- A fresh build containing the latest diagnostic trigger fix exists at:
  - `target-local-verify/release/touchHLE.exe`
  - title: `Dead Space (touchHLE 87a23def)`
  - SHA-256: `2035899F8A319796BAE3C56C5A77914DB92DC564EB4D681E4AD3127BCCB1F1D4`
- The fresh build launches normally, but repeatedly stalled at frame 10 when launched through the full diagnostic environment while another build was competing for CPU. The old binary was restored for deterministic captures.
- Committed diagnostic change: reset `PIXEL_DARK_TRACE_SAW_BRIGHT` when a trigger range starts. This fixes false
  `TRIGGER_AFTER_BRIGHT` activation at the beginning of a range. It is diagnostic-only and does not alter rendering.
- Existing dirty items `vendor/SDL`, `target-local-verify/`, and `target-verify/` are not part of this pass and must not be included in a release commit.

## Confirmed visual evidence

### 1. Hull scan lines remain

The 09:50 old-save run visually reproduces the long, regular bright/dark bands on the
station hull:

- [`logs/stripe-f10080.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/stripe-f10080.jpg)
- [`logs/menu-route-live-1.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/menu-route-live-1.jpg)

They are not removed by the currently active PVRTC mip reconstruction. Earlier A/B
tests also ruled out:

- global PVRTC mip rebuilding;
- anisotropic filtering;
- texture-wide nearest sampling;
- clamp-to-edge on the hull atlases;
- alternate LOD bias.

The pattern therefore points to per-draw composition/depth state or malformed indexed
geometry, not texture minification.

### Hull scan-line attribution

The `(300,267)` pixel on the bright part of a hull stripe reaches opaque white
through the 256x256 content key `15f54e17`. Its uploaded texture is not hull
artwork: it is the black/white dither/grid mask shown in
`logs/tex3-preview.jpg`. `f4d178fa` is the same class of mask. These legacy
dither layers should not be visible as scene textures on desktop OpenGL, where
real alpha blending is available.

### 2. Porthole interior can be lit in the right camera state

At guest frame 12240 in the 09:50 run, the exterior camera sees the room floor, walls,
lights, poster, and character rather than a black room:

- [`logs/current-f12240.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/current-f12240.jpg)

This supports retaining scene/projection-gated repair rather than globally disabling
fog or depth. It does not yet replace user acceptance, because later camera cuts and
normal gameplay still need checking.

### 3. The black weapon/block is distinct from the dark room

During the close room shot, a large pure-black polygon appears next to and partially
over the player:

- [`logs/current-f12360.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/current-f12360.jpg)
- [`logs/current-f12480-black.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/current-f12480-black.jpg)
- [`logs/scenegated-f7680.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/scenegated-f7680.jpg)

The official Android capture has a visible weapon and normal transparent effects at
the corresponding shot:

- [`logs/official-50-59-contact.jpg`](E:/CodexProjects/DeadSpaceLegacy/logs/official-50-59-contact.jpg)

This is the highest-confidence remaining material failure. It is consistent with a
transparent weapon/effect atlas being rendered as opaque black, but that is not yet
proven. A corrected per-pixel trace should identify the exact texture content key and
draw state.

### 4. Poster and porthole share dangerous overlay atlases

The iPad2 atlases involved are:

- room/poster/porthole overlay: `56c62e8a`
- porthole overlay alternative: `7027023c`

The current depth exception remains scoped to `count=6` and `count=36` draw batches:

```text
TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT=56c62e8a:6,56c62e8a:36,7027023c:6,7027023c:36
```

Do not restore either of these failed broad experiments:

- drawing the entire atlas without depth;
- deleting arbitrary hull triangles from `18c949fb`.

The first creates black sheets, and the second removes real station foundations.

## Diagnostic bug found and fixed

`TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_AFTER_BRIGHT` previously kept its `SAW_BRIGHT`
state after entering a later frame range. Consequently, a trace starting at frame
12000 incorrectly armed at frame 12010 during the station shot instead of waiting for
the character close-up.

The source now clears `PIXEL_DARK_TRACE_SAW_BRIGHT` whenever:

- tracing is disabled;
- the frame is before `TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME`;
- the frame is after `TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME`.

For immediate testing with the old binary, use an exact frame window instead of the
stateful trigger: `TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME=12360` and
`TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME=12360`.

## Black-block root cause (confirmed 2026-10-24)

The first probe used a reused `touchHLE_log.txt` and crossed two process runs,
which produced a false `56c62e8a/count=6` attribution. Logs are now isolated
per launch.

The isolated same-PID trace at frame 12360, point `(250,190)`, identified the
final opaque write:

```text
kind=elements count=2532 tex=78 key=9bdc088c
before=[3,4,5,255] after=[0,0,0,253]
depthFunc=513 depth mask=1
blend=false
cull=true
```

The final geometry is produced by several material batches, including the large
`9bdc088c` character/weapon atlas and `60bb1a47` room geometry. A triangle-only
skip was tested and did not fix the block. `TOUCHHLE_DS_FOG_RED=1` makes the
entire black region bright red, proving that the opaque black is the fog colour
rather than black texture data or alpha.

The live fog state at this frame was:

```text
GL_LINEAR
fog colour = [0, 0, 0, 1]
fog start  = -0.001
fog end    = 0
density    = 1
```

This is a degenerate range: every positive eye-space distance saturates to the
black fog colour on desktop GL. That explains both the black room and the large
black polygon over the character/weapon. A triangle-only skip was tested and
could not fix it because multiple material batches were affected by the same
fog state.

Do not remove `56c62e8a:6` based on the earlier mixed-log result; doing so was
observed to darken the porthole room.

The compatibility fix is scene-independent:

- inspect the live fog mode/range before a draw;
- if mode is `GL_LINEAR` and `end <= start` or `end <= 0`, disable fog for that
  draw only;
- preserve all valid positive fog ranges used for actual distance haze.

## Fixed OBS window identity

Dead Space now uses the stable SDL window title `deadspace ios`. This prevents
OBS from accidentally following a build/version-specific title such as
`Dead Space (touchHLE 173574de)`.

The top-level console window is explicitly named `deadspace console`; it must
not be used as the OBS source. The `dsm` scene captures:

```text
deadspace ios:SDL_app:touchHLE.exe
```

## Trace used to identify the black weapon

Use a point clearly inside the black polygon in frame 12360: `(250,190)` in the
480x320 landscape coordinate space.

```text
TOUCHHLE_DS_PIXEL_TRACE=250,190
TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME=12360
TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME=12360
```

The existing probe prints, for every draw that changes that pixel:

- texture GL name and stable content key;
- draw count and primitive mode;
- color before/after;
- depth function, depth mask, and depth before/after;
- blend state and blend equation;
- alpha test, cull, scissor, stencil, current color;
- texture units, combine modes, and texture matrices;
- per-vertex color samples.

The earlier `(200,300)` point was outside the central black polygon and therefore is
not a valid weapon probe.

## Navigation reliability

The old-save route is frame-timed and occasionally loses a tap:

1. frame 6000: title `开始`;
2. frame 6600: save slot 1;
3. frame 7200–7320: chapter-select screen;
4. frame 7500: Chapter 1 card;
5. frame 7800+: confirmation check mark.

The chapter-check icon is centered near `(417,300)`. Earlier `(396,297)` occasionally
worked, but multiple runs left the selection unchanged. The current diagnostic route
uses the card center `(165,130)` and check center `(417,300)`. This route has not yet
been validated to completion after the last edit, so do not treat a later
`12360` frame as gameplay unless frame 7320/7560/8040 was checked first.

## Next work order

1. Validate the navigation reaches the porthole room after the corrected card/check
   coordinates.
2. Run the exact `(250,190)` frame-12360 trace and classify whether black is produced
   by an opaque texture, missing alpha test/blend, or a separate overlay.
3. Trace a scan-line pixel on frame 10080, for example `(300,267)`, with the same
   exact-frame method. Compare the winning key/count against the official shot.
4. Fix only the identified draw/triangle/alpha state. Do not add frame-number or
   chapter-number conditions.
5. Re-run a 60 fps RTX 4090 + OBS capture with visible frame numbers and hand the
   distance-fog check to the user.

## Supporting tools created during this pass

- `E:/CodexProjects/DeadSpaceLegacy/run-host/obs_api.py`
  - OBS WebSocket v5 status, snapshot, source-window rebinding, record start/stop.
- `E:/CodexProjects/DeadSpaceLegacy/run-host/Runtime-PC/Start-DeadSpace-Diagnostic.ps1`
  - preserves the verified runtime environment and supports direct fixed-frame traces.
- `E:/CodexProjects/DeadSpaceLegacy/run-host/Runtime-PC/dsm-menu-route.txt`
  - old-save → chapter select → Chapter 1 automation.

OBS scene `dsm` now points at the current touchHLE window title. All game, launcher,
and OBS processes were closed after this analysis pass.
