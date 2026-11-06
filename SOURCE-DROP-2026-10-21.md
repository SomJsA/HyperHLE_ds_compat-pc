# Android source drop - 2026-10-21

This source tree combines the current PC rendering mainline with the remote
Android fork's real-device performance work.

## First build command

```powershell
.\tools\build-android.ps1 `
  -Abi arm64-v8a `
  -BuildType release `
  -BundledIpaPath <path-to-DeadSpace.ipa>
```

The IPA is intentionally not included.

## Important defaults

- Internal render scale: 2x
- Frame cap: 60 FPS
- Fast present: enabled by default in the launcher
- Skip readback finish: enabled only as the device-validated fallback option
- PC compatibility: enabled
- Debug logs and frame dumps: disabled
- Android minimum SDK: 21; target SDK remains configurable and defaults to 24

If a different GPU alternates black/bright frames, disable fast present in the
launcher. To force the slow present path from the command line, omit
`--fast-present`.

## High-value test coverage

1. Cold launch and built-in IPA copy.
2. Difficulty selection and JNI battery query (previous StackOverflow case).
3. Save screen: `档案 1+`, `第 12 章`, `05:04:33`.
4. First chapter opening: black layer, station rings, red/yellow poster.
5. 10-20 second hull panning: horizontal texture banding.
6. Fast present on/off: brightness stability, menus, HUD and touch input.
7. Chapter 12 store/workbench/kinesis/pickup flow.
8. Home/pause/resume and surface recreation diagnostics.

Collect app-only logcat, `dumpsys SurfaceFlinger --latency`, `dumpsys meminfo`,
GPU/GL properties, screenshots before/after toggles, and APK SHA-256.

## Source provenance

- Merge commit: recorded in `MERGE-COMMIT.txt` in the handoff package.
- Base PC-only changes and all Android-specific edits are fully materialized in
  this source tree.
- No save files, game IPA, account/network notification data or API keys are
  included.
