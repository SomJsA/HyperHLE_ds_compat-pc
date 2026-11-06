# Android exploration merge audit

Date: 2026-10-25

## Decision

PC mainline remains authoritative for every game-content, rendering and
compatibility behavior already accepted by the user. Android exploration adds
Android platform code and the default-off FSR3.1 probe; it must not replace PC
behavior on conflicts.

The user also explicitly did not ask Android to inherit desktop controls. The
merge therefore keeps:

- Android touch, launcher, Surface, EGL and lifecycle code on the Android path;
- PC launcher, keyboard, mouse, window and desktop diagnostics on the PC path;
- FSR3 behind Android-only compilation paths or default-off switches.

## Merge

| Item | Value |
| --- | --- |
| PC parent | `3c344a7f` |
| Android parent | `d505b64e` |
| Integration merge | `024f313b` |
| Merge repair | `37f55960` |
| Merge policy | `-X ours` for any textual conflict |

The merge initially reported no textual conflicts, but offline Android
compilation found that Git had combined `LauncherActivity.java` and
`MainActivity.java` incorrectly, omitting declarations while retaining their
uses. This was a real merge defect, not a toolchain problem.

Repair:

- restore both Java activities from the complete Android branch semantics;
- align the PC compatibility content value
  `TOUCHHLE_DS_DEPTH_LEQUAL` to the current PC mainline value `0`;
- remove the accidental missing final newline in
  `src/frameworks/opengles/gles_guest.rs`.

## PC authority check

The merge changed none of these PC-authoritative paths against parent
`3c344a7f`:

- `src/environment/ds_carrie_patch.rs`;
- `src/frameworks/opengles/ds_input_region.rs`;
- `src/gles/gles1_on_gl2.rs`;
- `src/lib.rs`;
- `tools/pc-runtime/`.

Shared-file changes were limited to Android FSR3/AHB integration:

| File | Change |
| --- | --- |
| `src/frameworks/opengles/eagl.rs` | Android-only D16/AHB FSR3 present path |
| `src/gles/present.rs` | portable present/FXAA/CAS additions; FSR default unchanged |
| `src/window.rs` | optional viewport stretch switch, default off |
| `src/frameworks/opengles/gles_guest.rs` | final newline cleanup only |

`TOUCHHLE_DS_FSR3_AHB` gates the FSR3/AHB path. The launcher's recommended
preset leaves FSR3 off. `TOUCHHLE_DS_VIEWPORT_STRETCH` is explicit and default
off. No PC keyboard or mouse setting is activated by an Android launch.

## Verification

### PC host

```text
cargo test --release --target-dir target-local-verify --lib ds_carrie_patch
3 passed

cargo test --release --target-dir target-local-verify --lib ds_input_region
4 passed

cargo build --release --target-dir target-local-verify
Finished in 4m09s
```

Merge-built host executable:

```text
target-local-verify/release/touchHLE.exe
bytes=31,142,912
sha256=91c2cd5478945fb1d8df864dbcc2aa1f057cfd1685dd9baeba99d9130f2f46ad
```

This binary was **not installed or launched**. The user explicitly requested
that the foreground remain untouched. The currently accepted installation
remains the previously tested build with SHA-256 `1d0a9e3b...`.

### Android release

Verification used the detached worktree:

```text
E:\CodexProjects\DeadSpaceLegacy\work\HyperHLE_ds_compat-merge-verify-20261025
```

Required clean-checkout preparation:

1. initialize git submodules;
2. provide the non-committed `vendor/boost` tree;
3. create `android/local.properties` with the SDK path;
4. run `tools/materialize-android-assets.ps1` to replace Windows symlink
   placeholders with real assets.

Build command:

```powershell
.\tools\build-android-fsr3.ps1 `
  -JdkHome 'C:\Program Files\Java\jdk-19' `
  -AndroidSdk 'E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3' `
  -GradleBat 'E:\CodexProjects\DeadSpaceLegacy\tools\gradle-8.11.1\bin\gradle.bat' `
  -RustRoot 'E:\CodexProjects\DeadSpaceLegacy\tools\rust' `
  -GradleUserHome 'E:\CodexProjects\DeadSpaceLegacy\work\gradle-home-fsr3' `
  -BuildType release -Abi arm64-v8a -TargetSdk 24 `
  -ScaleHack 3 -FastPresent:$false
```

Result:

```text
BUILD SUCCESSFUL
APK bytes=52,869,760
SHA256=5d77b73e1142dd5d6016f26bc650b15ca46aa8c350e449ae62ca3372d580cf92
```

The APK is a build artifact in the isolated verification worktree. It was not
copied over the recorded Android deliverable, because it is a merge verification
build rather than a new device-accepted release.

## Remaining validation gaps

- Android device is currently unavailable. No install, launch, layout walk,
  preset persistence, HOME/resume or resolution matrix was performed.
- Android gameplay rendering after the merge is not revalidated.
- FSR3 remains a D16 depth probe without motion vectors, camera jitter, history
  accumulation or HUD reactive/T&C masks; frame generation remains NO-GO.
- The merge-built PC executable has compile/test evidence only, not a new visual
  playtest. The installed PC build remains the user-accepted authority.

Do not close these gaps by inference from successful builds.

## Final independent audit

| Check | Result |
| --- | --- |
| PC authoritative paths differ from `2ac477c4` | none |
| PC installed game hash | `1d0a9e3b3308f50546e8a9352a7830ec2866e4971991c1cc57dbb579e66b9d80` |
| PC installed launcher hash | `b145504de089e4b96271859fd55d2abe61973593929f044aefa683e379126a5a` |
| PC installed files overwritten | no |
| Merge-built host SHA-256 | `91c2cd5478945fb1d8df864dbcc2aa1f057cfd1685dd9baeba99d9130f2f46ad` |
| Merge-built Android APK SHA-256 | `5d77b73e1142dd5d6016f26bc650b15ca46aa8c350e449ae62ca3372d580cf92` |
| APK package | `org.touchhle.android` |
| APK version | `024f313b` for the verification artifact |
| APK SDK | min 21 / target 24 / compile 35 |
| APK ABI | `arm64-v8a` |
| APK native libraries | `libtouchHLE`, SDL2, C++ shared and FSR3 bridge all present |
| APK archive CRC | valid |
| Handoff local links | 27 checked, 0 missing |
| Combat patch tests | 3/3 passed |
| Circle interaction tests | 4/4 passed |
| Audit parser compile | passed |
| Local Git history contains AI sign-off trailers | none |

Tracked PC worktree is clean after the final documentation commit. Expected
non-committed state remains:

- `vendor/SDL` submodule modification owned by the user;
- `target-local-verify/` and `target-verify/` build outputs.

The Android exploration worktree also deliberately retains its pre-existing
materialized assets, icon changes and untracked JNI build libraries. These were
not bulk-staged.
