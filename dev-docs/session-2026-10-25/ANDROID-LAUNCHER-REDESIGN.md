# Android launcher redesign and save import

Date: 2026-10-25

## Read-only visual reference

The launcher appearance was researched from Codex thread
`01a16ead-f0e6-7872-87b9-7ba5a63767be` and the local Dead Space: Extraction
launcher renders. No file in that project was changed.

The visual language distilled from it:

- near-black blue background;
- one narrow numbered navigation rail;
- charcoal panels with hairline blue-gray borders;
- cyan used sparingly for active state and the primary action;
- large restrained headings and compact metadata;
- a wide hero visual combining artwork, title, and status chips;
- minimal corner radius and consistent section numbering.

The Android launcher borrows that system, not Extraction's poster or game
identity. Its hero uses the original Dead Space iOS app icon extracted from the
game IPA's `iTunesArtwork`.

## Product identity

- Visible launcher title: `死亡空间IOS移植版`
- Android label and `BuildConfig.APP_NAME`: `死亡空间IOS移植版`
- Original game icon source: `iTunesArtwork` from the user's Dead Space 1.3.34
  IPA
- The previous HyperHLE gear icon is replaced for adaptive, round, and legacy
  launcher icons

## Launcher layout

The launcher now contains:

1. `01 概览` hero with original game icon, title, and recommended configuration.
2. `02 画质` presets and configurable rendering options.
3. `03 实验参数` FSR3/Vulkan diagnostics, capability status and advanced flags.
4. `04 存档与文件` ZIP import and the planned save-editor warning.

The fixed bottom action bar retains `恢复默认` and `启动游戏`.

## ZIP save import

Supported archive layouts:

- `Documents/var/*.sb`;
- `touchHLE_sandbox/com.ea.deadspace.bv/Documents/...`;
- direct `.sb` files (placed under `Documents/var`).

Safety behavior:

- only `.sb` files are copied;
- absolute paths and `..` traversal are rejected;
- archive expansion is capped at 256 MB and 10,000 entries;
- an import with no `.sb` file is rejected;
- the existing `Documents` tree is backed up to:
  `Android/data/org.touchhle.android/files/save-backups/Documents-<time>.zip`;
- extraction uses a staging directory, then replaces the active Documents tree.

The picker accepts all files because some Android providers classify ZIP files
as `application/octet-stream` or fail to classify them. ZIP and `.sb` validation
happens after selection.

`SaveArchiveCore` is pure Java and is covered by a host probe:

```powershell
javac -encoding UTF-8 -d <temp> `
  android/app/src/main/java/org/touchhle/android/SaveArchiveCore.java `
  tools/tests/SaveArchiveCoreProbe.java
java -cp <temp> org.touchhle.android.SaveArchiveCoreProbe
```

Expected output: `save_import_probe=ok`.

## Save editor

The save editor is explicitly marked as planned, not implemented. The launcher
states that editing `.sb` can make saves unreadable (risk level 1: high risk)
and that a future editor must retain automatic backups and validation.

## Bundled IPA

`tools/build-android-fsr3.ps1` now accepts `-BundledIpaPath`. The final Android
package must contain:

```text
assets/touchHLE_bundled/DeadSpace.ipa
assets/touchHLE_bundled/bundle.id
```

The final package was built from commit `e1306dcc` and passed a clean-install
launch on Sony without an external `ipa_path`.

```text
APK bytes: 585,311,562
APK SHA-256:
c4fc0f71585516e1817c19ff18dbfe55abf0523fb520ee91f72e65236dd9fc5e
```

Delivery directory:

`E:\CodexProjects\DeadSpaceLegacy\report\deliverables-e1306dcc`
