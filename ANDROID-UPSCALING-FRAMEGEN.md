# Android upscaling and frame-generation compatibility

Status: architecture note, not enabled by default in this source drop.

## Compatibility tiers

### Tier 1: spatial upscaling and sharpening

Candidates:

- FSR 1 / EASU + RCAS
- Generic Lanczos/bicubic + CAS
- Existing `TOUCHHLE_DS_PRESENT_SHARPEN` CAS path

This tier only needs the final color image and can run on OpenGL ES 2.0-class
hardware. It is the broadest fallback for Adreno, Mali, Immortalis, PowerVR and
older drivers. Preferred defaults can vary by GPU/frame budget, but the same
shader path should remain available on every device.

### Tier 2: temporal upscaling

FSR2-class temporal reconstruction requires:

- jittered projection
- depth
- camera motion vectors
- object/skinning motion vectors or a stable fallback
- reset/history invalidation at cuts, level loads and resolution changes

The emulator currently controls the final present path but does not yet expose a
complete motion-vector contract from the guest. This tier must therefore be
implemented behind a capability probe and disocclusion tests. UI, alpha
tested foliage and particle effects need explicit reactive masks.

### Tier 3: frame generation

FSR3-style frame generation requires temporal inputs plus a latency-safe present
queue. A generic color-only optical-flow interpolator is not equivalent and can
produce camera tearing, HUD ghosting and input latency. It must be:

- opt-in per device/driver
- disabled for menus, cutscenes and QTE prompts by default
- paired with a low-latency queue and a one-tap fallback
- validated separately on Adreno, Mali/Immortalis, PowerVR and Xclipse

## GPU capability policy

Runtime probing should record at least:

- renderer/vendor/version strings
- GLES version and supported extensions
- maximum texture size
- float/half-float render-target support
- timer-query availability
- estimated external-memory limits
- thermal/power mode where the platform exposes it

Recommended policy:

| Capability | Default |
| --- | --- |
| GLES2 and color-only present | CAS or bicubic spatial upscale |
| Compute/shader capable | FSR1-style EASU + RCAS |
| Complete depth/motion vectors | temporal upscale A/B build only |
| Stable low-latency queue + validation | frame generation opt-in build |
| Unknown driver or prior visual failure | native present, no generated frames |

## Processor-family risks

- Adreno: generally strong ES3 compute; validate tile resolve, half-float and
  YUV/swapchain color-space handling.
- Mali/Immortalis: validate precision qualifiers, texture-format limits and
  per-tile bandwidth; avoid assuming desktop float behavior.
- PowerVR: older devices may lack compute; PVRTC remains native, but temporal
  history and shader precision must be conservative.
- Xclipse/other: require generic extension probing, never a vendor-name-only
  allowlist.

The feature must remain composable with fast present. If frame generation needs
an additional present queue, it should replace the current present stage rather
than stack a second asynchronous path after it.
