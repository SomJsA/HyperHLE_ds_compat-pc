# Android four-device validation

Date: 2026-10-25

## Devices

The following are four physical devices. The Xiaomi phone briefly had both its
IP transport and an mDNS alias; those are the same phone and are not counted as
separate hardware.

| Device | ADB | SoC / GPU | Android | RAM | Display |
| --- | --- | --- | --- | ---: | --- |
| OnePlus ACE3 `PJE110` | `10.31.2.194:37953` | SM8550 / Adreno 740 | 15 / API 35 | 16 GB class | 1264x2780, override 1080x2376 |
| Xiaomi `M2012K11AC` | `10.31.2.162:37753` | SM8250 / Adreno 650 | 13 / API 33 | 12 GB class | 1080x2400 |
| Sony `SO-02K` | USB `BH902MM0B5` | MSM8998 / Adreno 540 | 9 / API 28 | 4 GB class | 720x1280 |
| Xiaomi tablet `2405CRPFDC` | `10.31.0.232:42417` | SM6225 / Adreno 610 | 14 / API 34 | 6 GB class | 1200x1920 |

The user temporarily enabled wireless debugging on the two Xiaomi devices.
Pairing ports are not reusable data ports; mDNS or the reported connection port
must be used.

## Findings and fixes

### Android 9 launcher crash

The first merged APK crashed on Sony API 28:

```text
java.lang.NoSuchMethodError:
No static method isSupported(IIIIJ)Z
in class Landroid/hardware/HardwareBuffer;
```

`Fsr3Capabilities.summary()` called the modern static method after an API 26
guard, but Sony's API 28 framework does not expose that method. The probe is now
loaded only inside a nested class on Android 12+, with a defensive catch. Older
devices show the native/FSR1 fallback instead of crashing.

The corrected APK starts the launcher on API 28 and all four devices without an
AndroidRuntime fatal.

### FSR3 availability

Only ACE3 passes the current basic FSR3 gate.

| Device | UI result | Vulkan feature | Reason |
| --- | --- | --- | --- |
| ACE3 | `FSR3 实验` enabled | 1.3 class | API + Vulkan + arm64 + compute |
| Xiaomi phone | unavailable | 1.1 feature level | Below project Vulkan 1.2 gate |
| Sony | unavailable | Old/insufficient path | Also below Android 12 gate |
| Xiaomi tablet | unavailable | 1.1 feature level | Below project Vulkan 1.2 gate |

This is capability fallback, not a claimed FSR3 failure test. The FSR3 path
itself remains an experimental D16 probe without complete motion/jitter/history.

### Redmi external file-access edge case

For the ad-hoc test IPA, pushing through `adb` created a shell-owned
`touchHLE_apps` directory that the Android 14 app could not read. The same IPA
was copied into app-private storage through `run-as`, and the game started.

This is an ADB test-fixture ownership problem. A clean install with the bundled
IPA lets the app create its own directory. The final package must be tested
without an external `ipa_path` override.

## Runtime smoke test

The same game IPA was installed on all devices and launched with:

- native 3x internal render;
- FXAA enabled;
- CAS 0.5;
- FSR3 disabled;
- PC content compatibility enabled;
- GPU present copy enabled.

All four `MainActivity` processes stayed alive and reached visible gameplay/menu
content.

| Device | Observed result |
| --- | --- |
| ACE3 | Main menu visible; first overlay sample about 42 FPS |
| Xiaomi phone | Difficulty selection; about 60.5 FPS, 1% low about 41.8 |
| Sony | Difficulty selection; steady about 59.4 FPS, 1% low about 40 |
| Xiaomi tablet | Difficulty selection; about 89.8 FPS on its 90 Hz mode |

ACE3's initial sample is marked as a sample, not a stable benchmark. The
simultaneous multi-device run is not a valid thermal or sustained-performance
benchmark.

Adreno drivers emitted repeated `glTexEnvfv: GL_INVALID_ENUM` messages on all
participating Adreno GPUs. They are non-fatal and the game rendered, but they
remain a rendering-cleanliness issue for a later code-level audit.

## Bundled final package clean-install test

Sony was used for a clean-install bundled-package test because its previous save
tree was backed up first. The app was uninstalled, the bundled APK was installed,
and no external `ipa_path` was supplied.

The app copied:

```text
/sdcard/Android/data/org.touchhle.android/files/touchHLE_apps/DeadSpace.ipa
```

from the APK and reached MainActivity. After first-run copy and shader warm-up,
the stable sample was:

```text
FPS 59.4  1% low 42.5  min 40.9  16.84 ms
```

This accepts the self-contained package path. It does not claim FSR3 quality or
frame-generation support.

## Evidence

Local evidence:

`E:\CodexProjects\DeadSpaceLegacy\report\android-3device-validation-20261025`

Important files:

- `four-device-final-smoke.jpg`;
- per-device `*-game-30s.png` and `*-game-30s.log`;
- launcher UI XML/log/screenshot snapshots;
- save-import probe source/result;
- original game icon extracted from `iTunesArtwork`.

No APK was uninstalled, no application data was cleared, and no game save was
deleted during this validation.
