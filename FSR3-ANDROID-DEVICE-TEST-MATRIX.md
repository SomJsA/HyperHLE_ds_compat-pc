# FSR3 Android device and compatibility matrix

Status: execution plan plus 2026-10-24 Adreno 740 async/direct-output evidence.
This is still not a claim of cross-device support.

## Evidence boundary

The only Android FSR3 execution evidence currently available is from:

| Field | Value |
| --- | --- |
| Device | OnePlus PJE110 |
| SoC | Snapdragon 8 Gen 2 / SM8550 / kalama |
| GPU | Adreno 740 |
| Android | 15 / API 35 |
| ABI | arm64-v8a |
| GLES | 3.2 |
| Vulkan | 1.3.128-class |
| Driver | `V@0676.69` |

Additional 2026-10-24 evidence on the same device:

- synchronous FSR, `320x480 -> 1620x1080`: 23.8 FPS median;
- async FSR, same input: 49.3 FPS;
- async FSR, `640x960`: 48.5 FPS;
- async plus direct AHB output, `640x960`: 50.0 FPS / 41.7 1% low;
- async plus direct AHB output, `960x1440`: 44.9 FPS / 35.9 1% low;
- native `640x960`: 60.3 FPS / 46.3 1% low.

Async/直接输出仍是默认关闭的实验路径，并固定增加约一帧显示延迟。

This does not establish support for any other GPU family, driver branch,
resolution, panel refresh mode, Android release or window lifecycle.

## Compatibility tiers

| Tier | Path | Required inputs | Default |
| --- | --- | --- | --- |
| 0 | Native GLES present | None | On for all devices |
| 1 | FSR1/EASU+RCAS, CAS or bicubic | Final color only | Optional fallback |
| 2 | FSR3.1 temporal upscaling | Color, linear depth, motion vectors, jitter, reset policy and HUD handling | Off, capability gated |
| 3 | FSR3.1 frame generation | Tier 2 plus validated low-latency paced presentation and UI separation | Off, never automatic |

The current Async/Direct implementation is an internal FSR3.1 reset-probe, not a
fully qualified Tier 2 path, because motion vectors, jitter, useful history and
HUD separation are still absent.

Tier labels are not interchangeable. FSR1/CAS must never be reported as FSR3.

## Resolution matrix

Run each mode at the following Surface sizes on a 60 Hz mode first. Restore
`wm size` and `wm density` after every run.

| Label | Surface override | Input after `scale_hack=2` | Stress |
| --- | --- | --- | --- |
| Low | 720x1600 | 360x800 | Small bandwidth and occupancy |
| Native-1080 | 1080x2376 | 540x1188 | Current handoff baseline |
| Tall | 1080x2400 | 540x1200 | Aspect-ratio and viewport crop |
| High | 1264x2780 | 632x1390 | Physical-panel output |
| Wide | 1440x2560 | 720x1280 | AHB and FSR memory pressure |

For every resolution record:

- input, Surface and FSR output dimensions;
- `AHardwareBuffer_allocate` result and stride;
- EGL/GL/Vulkan capability log;
- frame-time mean, 1% low, minimum and maximum;
- PSS/Graphics memory before and after 10-minute soak;
- screenshot with HUD, subtitles, text, weapon, particles and transparent UI;
- surface resize and HOME/resume behavior. Dead Space is a fixed-landscape
  game, so user-driven rotation is not an acceptance requirement.

## Refresh-rate matrix

On the device panel test 60, 90 and 120 Hz where exposed. A 120 Hz mode is not
evidence of stable frame generation. Record display mode ID and frame pacing.
Disable frame generation if:

- rendered input remains below 30 FPS;
- interpolated cadence is outside the VRR window;
- QTE/menu latency increases;
- pacing emits repeated long frames or large minimum-frame spikes.

## Device-family matrix

Priority is based on risk, not vendor-name allowlists.

| Priority | GPU family | Why |
| --- | --- | --- |
| P0 | Adreno 740 | Only existing functional evidence |
| P0 | One older Adreno, e.g. 650/660 | Queue, FP16 and driver-behavior regression |
| P0 | Mali-G78 or newer | Tile-memory, precision and external-memory differences |
| P0 | Immortalis-G715 or newer | Modern flagship Mali coverage |
| P1 | Adreno 8xx | Current-generation validation |
| P1 | Xclipse | AMD RDNA-derived mobile Vulkan implementation |
| P2 | PowerVR | Older devices and compute-path limitations |
| P2 | Android emulator | API/lifecycle smoke tests only, never performance claims |

Run the runtime capability probe before allocating FSR. At minimum require:

- Android API 31 or newer;
- arm64-v8a;
- Vulkan 1.2 or newer and compute support;
- AHardwareBuffer external memory;
- `VK_ANDROID_external_memory_android_hardware_buffer`;
- required queue families and timeline/synchronization support;
- EGL/GL external-image import;
- `EGL_KHR_gl_renderbuffer_image` for real depth capture;
- supported RGBA8 sampled/color-output AHB at the requested extent and stride.

Unknown devices must fall back to native or Tier 1. A failed probe must not leave
the game on a black frame.

## Practical Android policy

Do not whitelist by SoC name alone. Decisions should come from runtime features,
measured frame time and memory pressure:

1. Keep native GLES as the default and mandatory fallback.
2. Probe AHB external memory, Vulkan compute, D16 EGLImage, external storage
   output and required queue capabilities before allocating FSR.
3. Start at `scale_hack=2` on 8 GB or larger devices with a 1080p-class Surface;
   drop to scale 1 if p95 frame time exceeds 25 ms or free memory becomes tight.
4. On 1440p-class Surfaces, prefer scale 1 or reject Async FSR unless the device
   has demonstrated headroom. Never infer success from physical panel size.
5. Use the 3:2 content rectangle for this iPhone-only Dead Space package; apply
   letterboxing around it. Do not force a 4:3 iPad framebuffer.
6. Treat the Async path as an explicit latency tradeoff. Disable it for QTE or
   input-sensitive validation until input-to-photon latency is measured.
7. Rebuild the process after a persistent Surface-size change until in-process
   session destruction is proven not to leak or lose performance.
8. VRR and 90/120 Hz remain unverified. A higher refresh display is not evidence
   that the game or FSR path can pace at that rate.

## Lifecycle matrix

Exercise each FSR mode through:

1. cold launch;
2. menu -> chapter load;
3. HOME and resume;
4. screen off/on;
5. resolution override change;
6. split-screen/freeform if supported;
7. app force-stop and relaunch;
8. chapter transition, cutscene and save load;
9. thermal-throttled gameplay.

Every Surface/EGL context change must destroy the private shared context, EGL
images, AHardwareBuffers, Vulkan command resources and FSR context before the
new size is initialized.

## Visual acceptance scenes

Use the same save and scripted camera/input for all modes:

- starfield and star streaks;
- moving posters/signs;
- portholes and transparent glass;
- scanlines, film grain and particles;
- HUD edges and small text;
- subtitles and QTE prompts;
- weapon and character movement;
- rapidly rotating camera;
- dark gradients and bright specular highlights.

Reject a mode if text becomes unreadable, HUD ghosts, transparencies smear,
camera cuts accumulate history, or the final frame differs in aspect ratio.

## Performance acceptance

No performance claim is allowed without a physical-device A/B run using the
same build, save, scene, thermal state and resolution. Store raw frame intervals
and compute mean, 1% low, minimum/maximum and p50/p95/p99. Also capture:

```text
dumpsys meminfo <package>
dumpsys gfxinfo <package>
dumpsys thermalservice
dumpsys battery
top -b -n 1
```

Minimum candidate thresholds before Tier 2 can be called deliverable:

- no unexplained 2x PSS growth;
- no frame-time cliff relative to native;
- repeatable improvement over the same native-resolution workload at equal
  final quality;
- no crash, Vulkan device loss, black frame or unbounded queue growth over a
  ten-minute run.

Tier 3 additionally requires a latency test and separate real/interpolated frame
timestamps. Aggregate FPS alone is not acceptance evidence.
