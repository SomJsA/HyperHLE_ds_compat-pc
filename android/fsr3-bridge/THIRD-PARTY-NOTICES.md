# Third-party notices for the FSR3 bridge

## AMD FidelityFX SDK

- Source: <https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK>
- Android-port baseline tag: `v1.1.4`
- Android-port baseline commit:
  `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`
- License: see `LICENSE-AMD.txt`.

The prebuilt arm64 static libraries in `bin/ffx_sdk/` and the bridge shared
library contain AMD FidelityFX code. The official `v1.1.4` `sdk/LICENSE.txt` is
MIT-style and requires preservation of its notice. Bundled third-party
shader-compiler dependencies retain their own licenses. AMD's newer SDK
`2.3.0` license is binary-only and prohibits reverse engineering, so moving to
that SDK requires renewed license review.

## Snowapril/fsr3-android

- Source: <https://github.com/Snowapril/fsr3-android>
- Source commit used by the device experiment: `3a369b3`
- License: see `LICENSE-SNOWAPRIL.txt`.

The Android/Vulkan port, build fixes, test app and host shader-compiler changes
originate from this project.

## Local artifact record

Tested JNI library:

```text
android/app/src/main/jniLibs/arm64-v8a/libdsfsr3bridge.so
size:   6636016
sha256: EF0D1CC7D88EBD4AF999063C491DC013EB620AFEBF004545337E906216FD2FDC
```

Historical tested APK:

```text
DeadSpace-arm64-fsr3-es2-depth-v6-20261021.apk
size:   592581765
sha256: C0E45BDA8306F607A86FA4A792795FCFC7DF959DFDA09665176C51A233D691B8
```
