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

Android keeps the HD text switch off by default. The Android GPU text path is
not implemented in this worktree, so enabling the host overlay uses the
existing CPU composition path, which reads the renderbuffer back to RAM when a
card is visible. A missing or incomplete language bank disables only the
optional overlay and leaves native game text running.

## Local verification

- Gradle arm64 release build: passed with JDK 21, NDK 25.2.9519653.
- APK: `report/r6-multilingual-overlay-20261114/android/DeadSpaceAndroid-MultilingualLocal.apk`
- APK size: 543,123,030 bytes; SHA-256:
  `9F1284FDFB7C71A3EBBCA6DA723DF08D42E3046C4311939593268C28C1AD3CD7`.
- The APK contains `NotoSansKR-Regular.otf` (16,433,112 bytes) and a
  72,494,920-byte arm64 `libtouchHLE.so`. The packaged IPA is 439,177,445
  bytes and is copied from the supplied source IPA for this local build.
- Java compilation and packaging passed. A host `cargo test` attempt was not
  usable because the host checkout lacks the native `libxml2s` library; the
  Android release build compiled the Rust target successfully.

This is a local validation artifact only. No release or upload was performed.
