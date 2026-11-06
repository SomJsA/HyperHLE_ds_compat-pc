# Android deck scanlines and distant station depth — 2026-10-26

Scope: Android native GLES1 only, tested on Xiaomi tablet 2405CRPFDC / Android 14
(`adb 10.31.0.232:42417`). PC is an accepted reference and was not rebuilt or
changed by this work. No claim is made about other Android devices.

## Reproduction and cause

The user's 125.905-second recording still shows moving bands across the deck and
broken distant station faces around 60 seconds. The earlier alpha-mip acceptance
covered texture bleed in selected shots, not these remaining depth artifacts.
The installed baseline log confirms that all five alpha-aware mip fixes are active.

Read-only reference: `E:/BaiduSyncdisk/下载 (9)/官方构建【原生安卓】/官方构建【原生安卓】-1.mp4`.
PC investigation and current rendering code were inspected as references; the
Windows runtime, launcher and saves were not changed.

Runtime instrumentation exposed the actual scene depth downgrade:

```text
[DSnativeDepth] attached depth rb=3 framebuffer_bits=24
[DSnativeDepth] attached depth rb=2 framebuffer_bits=16
```

`SDL_GL_DEPTH_SIZE=24` configures the host window. The game subsequently attaches
its own explicitly allocated D16 renderbuffer, replacing the EAGL auto-depth
attachment. Distant station geometry therefore still uses 16-bit depth. Disabling
only the scoped background LEQUAL repair does not remove the artifacts.

## Repair

`src/gles/gles1_native.rs::RenderbufferStorageOES` promotes requested D16 storage
to D24 only when `TOUCHHLE_DS_NATIVE_DEPTH24=1`, dimensions/target are valid and
the native driver advertises `GL_OES_depth24`. It verifies the allocated depth
size; unsupported cases retain the original D16 request. No texture or triangle
is removed, and depth testing, writes, projection, UVs and fog remain intact.

`src/lib.rs` enables the flag only in the Android defaults for
`com.ea.deadspace.bv`. `TOUCHHLE_DS_NATIVE_DEPTH24=0` is the explicit A/B bypass.
Desktop defaults and rendering implementation are unchanged.

## Evidence and validation

Evidence root: `E:/CodexProjects/DeadSpaceLegacy/report/android-deck-station-20261026/`.

- Original tablet sandbox: `save-backup/`, 30 files / 68,551 bytes.
- `baseline-opening.mp4`: current installed alpha-mip package, original settings.
- `depthoff-actual.mp4`: only scoped LEQUAL disabled; still faulty.
- `depth16-actual.txt`: independent actual-16-bit instrumentation.
- `depth24-probe.mp4` and `.log`: opt-in D24, complete opening through Chapter 1.
- `depth-compare-16.jpg`, `depth-compare-20.jpg`: corresponding camera positions;
  D24 restores station faces/windows and smooths the quantized deck bands.
- Probe actual allocation: `960x1440 actual_bits=24`, scene framebuffer depth 24.

Final clean release APK: 584,975,953 bytes; SHA-256
`5177aa3afa8a2a5ede9ee9bdf65e259ec7d97f7512b96f05c33c90a9668c36e5`.
All 35 Gradle tasks executed successfully. apksigner v1/v2 passed. ZIP validation
found no missing prior entries, the same bundled IPA ID, the correct arm64
library, and no stale placeholder files or x86_64 libraries.

`final-off.mp4` reproduces the damage with flag=0. `final-default.mp4` starts
through the normal Launcher without diagnostic extras and restores the surfaces.
Both use existing user settings: internal3x, FXAA/CAS/FSR3 off. The final default
log confirms scene depth24 and framebuffer-complete status `0x8cd5`. Its95-second
recording covers the porthole and lit Chapter1 tutorial without new black regions
or fatal/panic markers. Other devices were not tested.

The original10 active save files were restored through Launcher ZIP import.
All SHA-256 values match the pre-test backup, with no extra active files and
application UID10265 ownership. The app is left idle at the Launcher.

Delivery: `E:/BaiduSyncdisk/DeadSpace/Android-Depth24-Fix-20261026/`, containing
the APK, a25-second moving comparison, full final recording, checksums and notes.
Files were placed in the local Baidu sync directory; remote upload completion
was not independently queried.

## Build reproducibility

The Windows checkout contained text placeholders instead of the SDL Java,
`touchHLE_fonts`, and `touchHLE_dylibs` symlink directories. Those three
placeholders were preserved under the evidence directory and materialized from
the repository's existing source. Original game icons and IPA were retained.
The final build uses Gradle `:app:clean` first, because incremental asset outputs
had retained the old placeholder entries. No reset/clean of source or saves was
performed. Existing unrelated dirty files are preserved.

Build command is the existing `tools/build-android-fsr3.ps1`, release arm64-v8a,
targetSdk24, scale3, FastPresent=false, using JDK19 / NDK25.2.9519653 / Gradle8.11.1.
