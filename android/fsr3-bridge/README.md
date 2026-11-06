# Android FSR3 bridge

This directory contains the default-off JNI bridge used by the FSR3
experiments. It is not part of the native compatibility path and is loaded only
when `TOUCHHLE_DS_FSR3_AHB=1`.

The implementation is based on `Snowapril/fsr3-android` commit `3a369b3`, which
ports AMD FidelityFX SDK `v1.1.4` (FSR 3.1.4) to arm64 Android Vulkan. AMD's
SDK `2.3.0` does not contain a Vulkan/Android backend.

## Runtime gates

- Android 12 / API 31 or newer.
- Vulkan 1.2 or newer with compute and external-memory support.
- `AHardwareBuffer` RGBA8888 support with GPU sampled-image and color-output
  usage.
- Four distinct Vulkan queue roles for frame generation. Upscaling has a
  reduced queue requirement.
- FSR3 upscaling is opt-in. Frame generation remains a separate opt-in and is
  not a deliverable path until real motion vectors, reactive masks and pacing
  are validated.

All three runtime switches are unset by default:

```text
TOUCHHLE_DS_FSR3_AHB=1
TOUCHHLE_DS_FSR3_REAL_DEPTH=1
TOUCHHLE_DS_FSR3_ASYNC=1
TOUCHHLE_DS_FSR3_DIRECT_OUTPUT=1
TOUCHHLE_DS_FSR3_FG=1
TOUCHHLE_DS_FSR3_DIAG_READBACK=1
TOUCHHLE_DS_FSR3_TRACE=1
```

`ASYNC` submits the current upscale without waiting for its Vulkan fence and presents the
previous completed AHB, adding about one frame of display latency. `DIRECT_OUTPUT` is valid
only with `ASYNC` and lets FSR write directly to the GLES-presented AHB instead of copying from
an intermediate image. Both default off; the original synchronous path remains available.

The current prototype still forces `reset=true` every dispatch, so it exercises
the FSR3.1 upscaler API but does not prove temporal-history quality. A
history-enabled build requires real motion vectors, jitter and reset decisions
from the renderer. Frame generation stays disabled unless the synthetic
diagnostic motion source is explicitly enabled; it is not a normal production
mode.

## Reproducible build

The tested `libdsfsr3bridge.so` is checked in under
`android/app/src/main/jniLibs/arm64-v8a/`. To rebuild it, obtain the Android
Vulkan source port and point the script at its SDK tree:

```powershell
.\android\fsr3-bridge\build-android.ps1 `
  -Fsr3SdkRoot <fsr3-android>\sdk `
  -NdkRoot <android-sdk>\ndk\25.2.9519653 `
  -VulkanHeaders <vulkan-headers>\include `
  -UsePrebuiltFfxLibraries
```

Without `-UsePrebuiltFfxLibraries`, also set `-Fsr3ShaderCompiler`; the script
then rebuilds the five FidelityFX static libraries and their SPIR-V shaders.

## Licensing

The AMD SDK `v1.1.4` file `sdk/LICENSE.txt` is an MIT-style license permitting
source and binary redistribution with the copyright and permission notice.
Some bundled shader-compiler dependencies have separate notices, so
`THIRD-PARTY-NOTICES.md` and all dependency notices must remain with source and
binary distributions. AMD changed the top-level SDK `2.3.0` license to a
binary-only/no-reverse-engineering form; that newer license is not inherited by
the existing `v1.1.4` binaries, but a future upgrade requires a fresh license
review.
