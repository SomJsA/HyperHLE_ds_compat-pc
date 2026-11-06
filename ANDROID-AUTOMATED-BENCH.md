# Dead Space Android automated benchmark

Updated: 2026-10-21

## Why this exists

The previous interactive workflow consumed many stepwise screenshots. Large
image payloads made the model request hit HTTP 413. The current benchmark keeps
the device loop deterministic and local:

1. launch the exported launcher with explicit configuration extras;
2. navigate the known save/chapter path with fixed ADB taps;
3. sample `[DSstats]` from logcat;
4. save one final screenshot per run;
5. run OCR and image metrics locally;
6. leave screenshots out of the chat context unless a failure needs inspection.

## Commands

Single run:

```powershell
.\tools\ds-android-bench.ps1 `
  -Preset gpu-copy-fast `
  -WarmupSeconds 20 -SampleSeconds 60
```

Preset matrix:

```powershell
.\tools\run-ds-benchmarks.ps1 `
  -Presets native-fast,gpu-copy-fast,fsr-fast,fsr-quality,fxaa,cas05,cas10 `
  -WarmupSeconds 20 -SampleSeconds 60
```

Aggregate existing runs:

```powershell
.\tools\summarize-ds-benchmarks.ps1 -Root <run-root>
```

Compare two same-resolution final frames:

```powershell
python .\tools\ds-image-compare.py <reference.png> <candidate.png>
```

## Runtime-only test hook

`LauncherActivity` accepts `--ez autorun true` plus the normal launch extras and
forwards them to `MainActivity`. `MainActivity` accepts `--ez stats_log true` to
emit a `[DSstats]` line every two seconds without displaying the FPS overlay.
These hooks use the same startup path as the interactive launcher.

## Caveats

- The fixed tap path assumes the existing Chinese test save and the current
  1080x2376 landscape input coordinate space.
- `1% low` is computed from the last 1024 present intervals. It is a real
  `presentRenderbuffer` cadence metric, not SurfaceFlinger refresh rate.
- A single 60-second run is an A/B signal, not a thermal soak. Use a longer
  `-SampleSeconds` value for release-candidate soak testing.
