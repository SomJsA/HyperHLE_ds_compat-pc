# Android multilingual HD text overlay

This change connects the Android launcher language choice to the selected IPA
string bank without modifying the original IPA. When the optional HD text
layer is enabled, `MainActivity` extracts the selected `deadspace_iphone.bin`
and an English fallback into private app storage, then passes their paths to
the existing host overlay catalog. Japanese remains Japanese-first and falls
back to English for missing entries.

The launcher also selects script-aware native fallback fonts. Noto Sans JP and
the new Noto Sans KR asset are loaded lazily by UIKit text rendering; Latin
languages continue to use the existing Liberation fonts. Hangul is included
in the CJK fallback range.

Android uses an independent Canvas layer for chapter, subtitle, and objective
cards. The native hooks provide event snapshots and suppress the original
duplicate draw only while the Canvas layer is ready. The Android host overlay
and its renderbuffer readback path are disabled in this mode. A missing or
incomplete language bank leaves native game text running and does not make
launch fail.

## Local verification

- Gradle arm64 release build: passed with JDK 21, NDK 25.2.9519653.
- APK: `report/r6-multilingual-overlay-20261114/android/DeadSpaceAndroid-R6CanvasLocal.apk`
- APK size: 543,334,696 bytes; SHA-256:
  `FB34681A4D205518DA5E19B6F7EC71916D4DF6B147122C54EA9D459F2F3A6C9D`.
- The APK contains `NotoSansKR-Regular.otf` (16,433,112 bytes) and a
  72,494,920-byte arm64 `libtouchHLE.so`. The packaged IPA is 439,177,445
  bytes and is copied from the supplied source IPA for this local build.
- Java compilation and packaging passed. A host `cargo test` attempt was not
  usable because the host checkout lacks the native `libxml2s` library; the
  Android release build compiled the Rust target successfully.
- Connected-tablet run reached the difficulty screen and the opening scene.
  Log evidence includes `primary_entries=996 english_entries=996`, Canvas
  readiness, chapter/subtitle/objective hooks, and no `slow path` or
  `copy renderbuffer to RAM` entry.

This is a local validation artifact only. No release or upload was performed.
