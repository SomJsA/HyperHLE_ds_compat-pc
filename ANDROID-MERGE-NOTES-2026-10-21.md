# Android merge notes (2026-10-21)

This branch merges the remote Android fork's real-device performance work with
the current PC rendering fixes. The PC rendering path remains authoritative for
textures, projection and compatibility logic.

## Merged performance work

On the OnePlus PJZ110 / Adreno 830 test device, changing render resolution did
not remove the main bottleneck:

| Present path | Average FPS | 1% low | Minimum |
| --- | ---: | ---: | ---: |
| Slow present + `glFinish` | 44.5 | 12.0 | 10.3 |
| Slow present + skip Finish | 47.1 | 14.0 | 10.6 |
| Fast present + skip Finish | 59.4 | 21.8 | 17.2 |

Changes retained in the merged source:

- `--fast-present` bypasses the forced composition/readback path when a
  fullscreen EAGL layer is available.
- The slow readback path reuses a source FBO owned by the EAGL context. The
  renderbuffer is reattached every frame so GL name recycling cannot leave a
  stale attachment.
- `TOUCHHLE_DS_SKIP_READBACK_FINISH=1` remains a device-validated diagnostic
  switch; the safe default still calls `glFinish`.
- Actual present timing is recorded independently of compositor/vsync callbacks
  for the Android FPS overlay and 1% low/minimum FPS.
- Android battery queries run on the SDL parent stack, avoiding the JNI
  `StackOverflowError` seen after difficulty selection.
- Fullscreen/immersive mode is reasserted after SDL creates its window.

## PC rendering work carried into Android

- The native GLES1 path now uses the final PC projection policy: a conservative
  depth-row scale plus automatic infinite-far conversion for the opening
  camera. The old hard-coded `PROJ_Z_SCALE=0.85` Android default was removed.
- `GL_LESS -> GL_LEQUAL` compatibility is retained for coplanar background
  layers, with wrapper-side depth/fog state tracking to avoid emulator GLES1
  encoder crashes.
- PVRTC atlases `7027023c`, `7bcfa36c` and `780d00eb` rebuild their mip chains
  from decoded level 0. Later app mip uploads are ignored, eliminating the
  striped poster/hull issue while retaining mip filtering.
- Glyph remapping is applied in the two `CGContextShowGlyphs...` paths that
  previously bypassed `gid_remap`, fixing save-slot numbers and timestamps.
- Only Dead Space's exact retired-server alert is suppressed.
- Android Dead Space defaults use ScreenScale=2, no-fog, glyph scale/dy and the
  updated depth/projection behavior.

## Build

From the repository root:

```powershell
.\tools\build-android.ps1 -Abi arm64-v8a -BuildType release `
  -BundledIpaPath <path-to-DeadSpace.ipa>
```

Set `-FastPresent $false` to force the slow present path for an A/B run. The
device launcher also exposes fast present, skip-Finish, FPS, logging and frame
dump toggles.

## Not merged blindly

- The Android launcher and JNI overlay remain Android-specific.
- The Android fork's old `PROJ_Z_SCALE=0.85` default was replaced by the final
  PC behavior rather than copied.
- Fast present is a fullscreen-game path. Non-fullscreen drawables still use the
  compositor, and the toggle can be disabled per device if a driver alternates
  direct and compositor frames.
