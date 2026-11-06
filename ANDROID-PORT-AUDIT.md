# Android port audit

This document records the Android-specific data path and compares it with the
desktop and iOS paths. It is based on the current source, built artifacts and
measured failures; it is not a claim that the Android runtime is complete.

## 1. Build and ABI

| Item | Android | Desktop | iOS |
|---|---|---|---|
| Host CPU target | arm64-v8a by default; x86_64 debug opt-in | x86_64 Windows/Linux/macOS | ARM iPhone/iPad |
| Minimum OS | API 21 | host OS | host iOS version |
| Toolchain | Gradle 8.11.1, AGP, SDK 35, NDK 25.2, CMake/Ninja, cargo-ndk | Cargo/CMake | Xcode/iOS SDK |
| Entrypoint | `SDLActivity` -> `SDL_main(argc, argv)` | native `main` | iOS app lifecycle |
| IPA data | APK asset, first-launch copy to app external files | ordinary IPA file path | installed `.app` bundle |

`minSdk 21` is the practical minimum for AArch64 + Dynarmic. `compileSdk 35` is
only the build SDK; it does not mean the device must run Android 15.

The default build only emits `arm64-v8a`. x86_64 is available through
`-PtouchHleAndroidAbi=x86_64` for emulator diagnostics.

## 2. IPA loading path

The Android path is:

```text
APK assets/touchHLE_bundled/DeadSpace.ipa
  -> MainActivity.getArguments()
  -> getExternalFilesDir(null)/touchHLE_apps/DeadSpace.ipa
  -> SDLActivity nativeRunMain()
  -> SDL_main(argc, argv)
  -> touchHLE main(args)
  -> normal bundle/IPA loader
```

`MainActivity` writes a `.part` file and renames after a successful copy. A
`bundle.id` asset containing the IPA SHA-256 lets later launches skip the copy
when the embedded IPA has not changed.

Important historical bug fixed in `9a4369d9`: the Android `SDL_main` initially
discarded `argc/argv`, so the path was lost at the native boundary. The current
entrypoint converts all argv values and logs `[Android] SDL_main argc=...`.

SDL Android supplies `argv[0] = "app_process"` and appends the Java
`getArguments()` strings from `argv[1]`.

## 3. Window, lifecycle and display

- Android uses a fullscreen SDL surface and lets SDL manage Android pause/resume
  and surface recreation.
- `Window::new` sets `SDL_ANDROID_BLOCK_ON_PAUSE=0` so touchHLE can keep its own
  event/render loop behavior.
- Android requests an OpenGL ES 1.1 context before the window is created; the
  guest GLES1 calls are translated by the Android-native backend.
- SDL is configured to prevent touch events from being synthesized into mouse
  events and mouse events from being synthesized into touch events.
- Desktop can resize the host window; Android is expected to stay fullscreen.
  Resolution and DPI must be verified on the device rather than inferred from
  desktop screenshots.

## 4. GLES backends

The source has native Android/ES backends rather than the desktop GL translation
path:

- `gles1_native` / ES 1.1 passthrough for legacy GLES1 calls.
- `gles2_native` / ES 2.0 passthrough.
- `gles3_native` / ES 3.0 passthrough.
- `gles1_on_gl2` exists as a translation fallback and contains the compatibility
  work for framebuffer, texture and projection issues.

`create_gles2_ctx` and `create_gles3_ctx` assert that native context creation is
on the main coroutine stack. A wrong-stack or failed context creation should
produce a panic/log line; it must not be silently treated as a rendering bug.

The current Android test build does not yet expose a device-side settings UI for
FSR1, MSAA or anisotropic filtering. Those switches are implemented in the
renderer but need a device-side A/B path and GPU capability reporting.

## 5. Audio

Audio Queue APIs are implemented through the OpenAL Soft wrapper. Android links
OpenAL Soft statically through the existing Rust feature set. On a new level,
AudioQueue/AudioUnit objects can be created and started, so audio initialization
is a plausible point for a level-transition crash and must be visible in logcat.

The crash collector should retain the log lines around AudioQueue, OpenAL,
AudioSession and AudioUnit rather than filtering only for SIGSEGV.

## 6. Input and touch

The Android surface receives SDL touch events. `window.rs`:

- rotates/clamps touch coordinates for the current orientation;
- separates touch and mouse channels;
- supports the Dead Space virtual stick and E/Z/F/C/R action paths;
- supports QTE tap, swipe and repeated tap injection through the existing
  action/gesture paths;
- supports controller input and touch fallback.

The Android-specific requirement is that the touch ring's live screen rectangle,
not a visually estimated radius, is used for hit testing. Ring edges and nearby
decorative textures must not be mistaken for a control.

## 7. Storage and save data

The user data base is SDL's Android external files directory. The expected
layout is:

```text
Android/data/org.touchhle.android/files/
  touchHLE_apps/       imported/bundled IPAs
  touchHLE_sandbox/    guest Documents and save data
  touchHLE_fonts/      emulator font resources, from APK assets
  touchHLE_log.txt     emulator file log
```

The `DocumentsProvider` exposes the same root to the Android document picker.
Android scoped-storage changes mean files under `Android/data` may not be
browsable through ordinary file managers on newer devices; ADB or the in-app
provider is the authoritative path.

## 8. Fonts, text and textures

- Android embeds `SweiShearSansCJKsc-Medium.ttf` under
  `touchHLE_ds_fonts` and uses it as the emulator font override.
- Guest Latin/digit fonts remain untouched by default to avoid the known
  save-time glyph regression.
- Text replacement and HD texture packs use the same host-side compatibility
  code, but Android packaging and texture-size limits still need device A/B.
- The `ipa` asset is stored uncompressed; the game data already is an archive.

## 9. Diagnostics

The current Android build logs to both logcat and `touchHLE_log.txt`. The native
entrypoint logs argc/argv. The current signal handler writes:

```text
touchHLE-crash: touchHLE native crash: SIGSEGV
```

before restoring the default handler and re-raising, preserving the normal
Android tombstone.

The crash collector should capture:

- `logcat -b all`
- `dumpsys activity exit-info`
- `dumpsys meminfo`
- GPU/OpenGL properties
- `touchHLE_log.txt` via `run-as`

## 10. Current evidence and remaining gaps

Evidence completed:

- arm64 debug and release APK builds;
- APK manifest, ABI and v1/v2 signature checks;
- embedded IPA and font/resource presence;
- release optimized build and noCompress IPA entry;
- Java self-loader DEX strings and native argv diagnostic strings;
- branch snapshot bundle clone test.

Not yet proven:

- a successful Android launch with the embedded IPA path received by native;
- stable difficulty selection and level transition;
- main-menu and in-level 3D rendering on the user's Android GPU;
- native touch ring, pickup, QTE, audio and lifecycle behavior on the device;
- Android GPU A/B for FSR1, MSAA and anisotropic filtering.

The next evidence needed is the user's `crash-*` directory from the SignalDiag
package, not another static APK inspection.
