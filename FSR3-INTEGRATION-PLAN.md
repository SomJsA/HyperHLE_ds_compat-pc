# Dead Space Android FSR3 integration plan

Updated: 2026-10-21

## Evidence and constraints

- Real test device: OnePlus `PJE110`, Android 15 / SDK 35, arm64-v8a.
- GPU: Adreno 740, OpenGL ES 3.2, Vulkan hardware version 1.3 with compute.
- The old note that called this device Adreno 830 is incorrect; use the current
  `dumpsys SurfaceFlinger` and package-manager values as authoritative.
- AMD's current FidelityFX SDK checkout (`60f4ea8`, SDK 2.3.0) has no Android or
  Vulkan Android backend. It cannot be linked directly into this app.
- The candidate integration base is `Snowapril/fsr3-android`
  (`https://github.com/Snowapril/fsr3-android`, current inspected commit
  `3a369b3`). It is an Android arm64 Vulkan port of FidelityFX SDK 1.1.4 with
  upscaler, optical flow, and frame interpolation. License is AMD's MIT-style
  FidelityFX license.
- That port requires `minSdk 31`; current app is `minSdk 21`. FSR3 must be a
  capability-gated API 31+ path, not the only Android launch path.
- FSR3 frame generation in that port needs four distinct Vulkan queues assigned
  to game/compute/transfer/present roles. If fewer queues are exposed, it falls
  back to FSR3 upscaling only.
- Frame generation has not yet been validated end-to-end on a physical device by
  the upstream port. It is not enough to merge the sample and claim success.

## Target architecture

```text
guest GLES1 rendering
  -> offscreen color renderbuffer
  -> GPU copy / AHardwareBuffer export
  -> Vulkan FSR3 bridge
       color + depth + synthetic motion vectors + reactive mask
       -> FSR3 temporal upscaling
       -> optional FSR3 frame interpolation
  -> Vulkan frame-interpolation swapchain OR shared output buffer
  -> presentation / pacing
```

The Android bridge must keep three modes:

1. Native present (existing path): compatibility fallback.
2. FSR1/FXAA/CAS present: current GLES-side fallback on unsupported devices.
3. FSR3 Vulkan: API 31+, Vulkan 1.3, required queue/extension and FP16 checks.

## Required Android pieces

### 1. Build FSR3 core

- Vendor only the `sdk/` sources and shader compiler assets needed by the
  Android sample, not the Windows bins or Cauldron framework.
- Build the host `FidelityFX_SC` shader compiler, then generate SPIR-V shader
  headers for arm64.
- Add `ffx_backend_vk_android_arm64`, `ffx_fsr3upscaler_android_arm64`,
  `ffx_frameinterpolation_android_arm64`, and
  `ffx_opticalflow_android_arm64` to the Android CMake build.
- Keep FSR3 in its own JNI library so a build failure cannot break the native
  GLES-only launcher.

### 2. Capability probe and launcher gate

Report and persist:

- Android API level and ABI
- Vulkan version and physical-device properties
- available queue families and distinct queue count
- FP16 / float16-int8 support
- `VK_KHR_synchronization2`, descriptor indexing, external-memory support
- `AHardwareBuffer` format/usage support
- GLES EGLImage import support

Only expose FSR3 frame generation when all required checks pass. Otherwise show
FSR3 upscaling if possible, or FSR1/native fallback.

### 3. GLES to Vulkan frame bridge

Preferred path:

- Allocate an `AHardwareBuffer` with GPU sampled/color-output usage.
- Import it into GLES as an EGLImage target.
- Import the same buffer into Vulkan through
  `VK_ANDROID_external_memory_android_hardware_buffer`.
- Copy the final game color into the shared buffer on the GPU, never through
  `glReadPixels`.

Keep a CPU-readback fallback only as a diagnostic build, not the normal FSR3
path.

### 4. Depth, motion and masks

The guest game does not provide FSR3 motion vectors.

First implementation:

- Capture depth through a GPU copy of the guest depth renderbuffer.
- Track the last 3D view/projection matrices at the present boundary.
- Generate camera reprojection motion vectors from current/previous depth and
  matrices in a GLES3 or Vulkan compute pass.
- Generate a reactive mask for HUD, weapon, particles, subtitles and QTE UI.
- Set motion to zero and force reset/history invalidation on level load,
  cutscene, menu, camera cut and save-load transitions.

This is an approximation, not game-provided motion vectors. Character and
alpha-tested geometry ghosting must be measured before enabling frame generation
by default.

### 5. Presentation queue and pacing

- Use the Vulkan frame-interpolation swapchain when the device exposes the
  required queues.
- Otherwise use a custom low-latency queue that alternates real and interpolated
  frames without changing the guest's simulation cadence.
- Do not generate frames for menus, cutscenes, QTEs, loading screens or when the
  frame rate falls below the FSR3 safety threshold.
- Preserve a one-tap fallback to native/FSR1 presentation.

## Milestones

### M0 - standalone port validation

- Build the community Android FSR3 sample with the current NDK/toolchain.
- Install only the sample app first.
- Verify Vulkan queue assignment, FP16 path, upscaler output and, if queues allow,
  two presents per rendered frame.
- Capture logcat and a short screen recording.

### M1 - capability probe in Dead Space app

- Add the probe to the launcher without enabling FSR3.
- Confirm the real queue count and extensions on the Adreno 740.

### M2 - zero-copy color bridge

- Add AHardwareBuffer/EGLImage sharing.
- Verify that the GLES frame reaches Vulkan unchanged with no CPU readback.

### M3 - FSR3 upscaling only

- Supply color, depth, camera-reprojected motion vectors and reactive mask.
- Validate against FSR1 at the same save and camera.
- Keep FSR3 upscaling usable even when frame generation is unavailable.

### M4 - frame generation

- Enable the frame-interpolation swapchain only when its queue requirements pass.
- Validate pacing, HUD stability, QTE latency, camera cuts, level loads and
  Home/resume surface recreation.
- Add explicit safe-mode exclusions and crash fallback.

### M5 - deliverable

- Launcher mode and capability report.
- Adreno A/B screenshots, logs, frame-time traces and memory usage.
- arm64 APK with unique name, SHA-256, README and build info.
- Existing APKs, saves and backup packages are never overwritten.

## Acceptance criteria

- Native present fallback still works on API 21+ devices.
- FSR3 path does not use per-frame CPU color readback.
- FSR3 upscaler output is visible and stable on the Adreno device.
- Frame generation, if enabled, presents two frames per rendered frame and
  survives a 10-minute gameplay run without deadlock or visual corruption.
- Menu/cutscene/QTE safe-mode behavior is demonstrated.
- The app falls back cleanly when queues, extensions or FP16 support are absent.

## Current blockers and risks

- The community port's physical-device frame-generation path is still unvalidated.
- Adreno queue-family assignment must be tested on this exact phone.
- Motion vectors are synthetic because the guest game does not expose them.
- GLES/Vulkan external-memory interop is the largest engineering risk.
- FSR3 frame generation below its target input frame rate can look worse than
  native presentation; it must remain opt-in until measured.
