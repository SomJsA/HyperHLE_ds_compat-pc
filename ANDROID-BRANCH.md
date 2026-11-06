# Android branch

> **Current handoff (2026-10-25):** read
> [`TAKEOVER-ANDROID-FSR3.md`](TAKEOVER-ANDROID-FSR3.md). The sections below
> describe the first Android bring-up and contain historical crash notes; they
> are not the current FSR3 validation status.

This repository branch contains the Android arm64 adaptation of HyperHLE/touchHLE
for Dead Space 1.3.34. Game IPA data is intentionally excluded from Git.

## Current branch state

Base commit for the first handoff:

```text
6c76fa91d5d9a40a49cec3111e05c90039dae703
android: bundle and auto-load the game IPA
```

The Android package:

- uses `minSdkVersion 21` and `targetSdkVersion 24`;
- builds only `arm64-v8a` by default;
- embeds `touchHLE_ds_fonts/SweiShearSansCJKsc-Medium.ttf`;
- supports an optional IPA asset at `touchHLE_bundled/DeadSpace.ipa`;
- copies that IPA into `touchHLE_apps/DeadSpace.ipa` on first launch and passes the
  absolute path to the normal touchHLE loader;
- supports an x86_64 diagnostic build through
  `-PtouchHleAndroidAbi=x86_64`, but x86_64 has not yet been run in this VM.

## Toolchain

The reference build uses:

- JDK 17
- Gradle 8.11.1
- Android SDK 35
- Build Tools 35.0.0
- NDK 25.2.9519653
- CMake 3.31.6
- Ninja
- Rust target `aarch64-linux-android`
- cargo-ndk

`compileSdk 35` is not a minimum device version. The APK is installable from
Android 5.0 / API 21.

## Materialize Android assets on a clean checkout

Some Windows checkouts represent symlinked resource directories as text
placeholders. Replace them in the build tree before Gradle:

```powershell
.\tools\materialize-android-assets.ps1 -Repo C:\path\to\HyperHLE_ds_compat
```

The script backs up each placeholder and copies the real SDL Java, dylib, font,
default-options and icon resources into `android/app/src/main/assets` and
`android/app/src/main/res`.

## Build an APK with a bundled IPA

```powershell
.\tools\build-android.ps1 `
  -Abi arm64-v8a `
  -BuildType debug `
  -BundledIpaPath C:\path\to\DeadSpace.ipa `
  -ToolchainRoot C:\dsh\android-toolchain `
  -GradleUserHome C:\dsh\gradle-home
```

The IPA and `bundle.id` written by the script are ignored by Git. Build without
`-BundledIpaPath` for the legacy app-picker behavior.

## Outputs

```text
android/app/build/outputs/apk/debug/app-debug.apk
android/app/build/outputs/apk/release/app-release.apk
```

Debug packages have been tested for static installation/package integrity.
Device runtime validation is still in progress.

## Known current issue

The first real-device run reached difficulty selection and then crashed. The
native Android logcat, activity exit-info and tombstone are required before
claiming a root cause. Use `collect-android-crash.ps1` from the test-package
handoff to collect it.
