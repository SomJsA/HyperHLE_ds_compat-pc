# Distance rendering: fog and mip investigation (2026-10-22)

## Symptoms

- Objects at a distance disappear into blackness with an abrupt transition.
- The official build instead has a gradual fog-like transition.
- Distant posters show horizontal/comb-like stripe artifacts.

These are distance-dependent symptoms and are separate from the failed tex55
triangle-depth experiment in `2247/0229`.

## Fog bug

The desktop GL2 backend originally treated `TOUCHHLE_DS_NO_FOG` as a boolean by
*presence*. The launcher always set the environment variable, using the string
`"0"` when compatibility fog was supposed to be enabled. Because `"0"` still
exists, fog remained disabled in both launcher states.

The backend now parses the value: `0` and `false` mean fog is enabled. The
launcher has a separate `关闭雾（仅旧 B1 黑房问题需要）` option, disabled by
default, so the normal path preserves the original distance fade.

VM A/B evidence:

`F:\CodexProjects\DeadSpaceLegacy-package-staging\input-room-probe-20261022-fog-on\fog-ab.jpg`

At the same saved room approach, fog-enabled rendering transitions the distant
doorway through the original atmospheric haze instead of treating fog as an
all-or-nothing switch.

## Poster mip A/B

The default opening mip rebuild only covered keys
`7027023c / 7bcfa36c / 780d00eb`. Distant posters may use other PVRTC atlases,
which still sample their original mip chains and can retain combing.

`TOUCHHLE_DS_AUTO_MIPS_ALL=1` now extends the same level-0 decode and clean mip
generation to all eligible 2D PVRTC textures. The launcher exposes it as
`全局重建 PVRTC mip（测试远景条纹）`, disabled by default. It is an A/B
diagnostic until texture identity and performance are measured on the target
GPU.

## Current controls

- Fog-on: `TOUCHHLE_DS_NO_FOG=0`
- Fog-off: `TOUCHHLE_DS_NO_FOG=1`
- Clean mip chain for the known opening atlases:
  `TOUCHHLE_DS_AUTO_OPENING_MIPS=1`
- Clean mip chain for all eligible PVRTC textures:
  `TOUCHHLE_DS_AUTO_MIPS_ALL=1`

The input selector remains independent of these render controls.

The first hardware A/B showed:

- fog on restored the gradual distance reveal;
- global PVRTC mip rebuilding did not remove the distant poster pattern;
- fog off removed the poster pattern but restored abrupt object pop-in.

This points to a draw-mode interaction rather than texture minification. The
launcher now keeps perspective fog enabled while setting
`TOUCHHLE_DS_NO_FOG_ORTHO=1`, which prevents eye-space fog from being applied
to orthographic/2D overlay passes.

## Opening porthole isolation (2026-10-22)

The full-screen-layer fog mask was tested in the VM and was insufficient: the
distant porthole still rendered almost black. A point probe at frame 2520,
landscape point `(240,162)`, identified the responsible archive as texture 37,
content key `7027023c`, drawn as a blended overlay with depth writes disabled.

Disabling fog only for content key `7027023c` restores the lit room behind the
opening porthole while retaining fog on opaque hull geometry. The launcher now
sets `TOUCHHLE_DS_NO_FOG_HASH=7027023c`. `TOUCHHLE_DS_NO_FOG_SCREEN` remains an
unchecked diagnostic because its first A/B did not fix the porthole.

## Poster draw isolation

The earlier mip-based diagnosis did not hold on the host RTX 4090. A
same-camera OBS capture showed that the sign artwork itself was intact; black
triangular fragments from a coplanar scene surface were winning the depth test
over the lit poster/wall decals. The bounded `GL_LINEAR` workaround therefore
did not repair the failure and has been removed from the launcher defaults.

The later 30 fps audit showed that disabling depth across scene atlases was the
wrong repair. It was removed. The final runtime keeps gameplay on `GL_LESS` and
restores the star stack with four space-background keys plus
`TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE=1`.

## Final RTX 4090 OBS verification (2026-10-23)

Two independent A/Bs changed only the depth behavior:

1. `TOUCHHLE_DS_DEPTH_LEQUAL=1` restores the bright nebula behind the opening
   planet. Without it, the earlier coplanar black background layer owns the tie
   and the starfield appears black.
2. `TOUCHHLE_DS_TEXNODEPTH_HASH` over the eight opening atlases removes the
   triangular cuts on the station, the Chapter 1 safety poster, and the lit
   cockpit interior.

The old poster `GL_LINEAR` filter was removed in the same run; the poster stayed
intact, confirming it was not needed. A second run with `NO_FOG_HASH` disabled
turned the cockpit walls and poster black again, so the scoped fog exclusion is
still required.

The first launcher verification still showed one black wedge across the poster.
The launcher's depth-overlay key list omitted the base opening-platform atlas
`e2593d6c`. After adding it, a fresh run through the real launcher showed the
complete cream poster border, both red text panels, the yellow field, and both
black hand silhouettes. The black hand artwork is part of the texture; the
failure mode is the diagonal black wedge that replaces the sign border and
cuts through the red/yellow artwork.

Verified frames:

- `E:/CodexProjects/DeadSpaceLegacy/logs/auto-safe-posternodepth-depthlequal-32shots-walk/contact-small.jpg`
- `E:/CodexProjects/DeadSpaceLegacy/logs/auto-safe-depthlequal-allopeningnodepth-nofogh-noposterfilter-28shots-walk-minimal/contact-small.jpg`
- `E:/CodexProjects/DeadSpaceLegacy/logs/launcher-28-verified-fxoff-nomouse/contact-small.jpg`
- `E:/CodexProjects/DeadSpaceLegacy/logs/launcher-28-poster-final-check/contact-poster.jpg`
- `E:/CodexProjects/DeadSpaceLegacy/logs/poster-final-check-crop.jpg`

The last capture launched through the real `DeadSpace Launcher.exe`. FSR and
fast-present were disabled only for this OBS pass because the current OBS
window source loses that swapchain and records white frames.

## Residual opening scanlines

A residual pattern remains on some large hull surfaces during the opening
cutscene. The following A/B runs did not remove it:

- global PVRTC mip rebuilding;
- 16x anisotropic filtering;
- texture-wide no-mip sampling for `7bcfa36c/780d00eb`;
- forced clamp-to-edge wrapping for those atlas keys;
- LOD bias 0 or -1 instead of the existing +2 opening bias.

The current runtime therefore keeps the verified clean-mip chain and does not
enable any of those experimental overrides. The remaining artifact is isolated
to the hull texture/decode path rather than fog, depth, poster filtering, or
presentation.

The bundled runtime uses the iPad2 resource tree. Its content fingerprints are
different even though the logical opening atlases are the same:

| Role | Base IPA | iPad2 runtime |
| --- | --- | --- |
| wall/poster/porthole | `7027023c` | `56c62e8a` |
| hull/ring | `7bcfa36c` | `18c949fb` |
| lower-building | `780d00eb` | `d1feb687` |
| main opening platform | `e2593d6c` | `2131afd5` |

The clean-mip repair now covers both sets. Poster and porthole depth repairs
also cover both sets.

## Mouse capture

`TOUCHHLE_DS_MOUSE_LOOK=all` is the launcher default. VM keyboard-event testing
confirmed this sequence:

```text
start: relative mouse ON
Alt:   CLICK mode, relative mouse off
Alt:   LOOK mode, relative mouse on
Win:   CLICK mode, relative mouse off
```

`F8` continues to toggle the same two modes.

## Current project runtime

The directly runnable project copy is:

`E:\CodexProjects\DeadSpaceLegacy\run-host\Runtime-PC\DeadSpace Launcher.exe`

Shortcut:

`E:\CodexProjects\DeadSpaceLegacy\run-host\启动 Dead Space.lnk`

Current binaries:

- `touchHLE.exe`: `9b616b66edd96f447d06448b3a295a838321ce50344999558f7347eb0f6b035f`
- `DeadSpace Launcher.exe`: `3e8bb4f719513ca515d0c082befd6d0d55a2c786984cc038570b86bfb6621a1c`

This runtime keeps the failed tex55 depth/triangle experiments disabled,
preserves perspective scene fog, isolates the opening porthole wall atlas from
invalid fog, and fixes the coplanar opening layers with scoped depth behavior.
