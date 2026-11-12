# PC installer distribution

2026-11-06 user revision. This supersedes the temporary "extract a program ZIP
and keep it beside a marker" handoff as the user delivery shape. The program ZIP
remains an internal build input and may be used as a GitHub Release update
asset, but the tested delivery is exactly:

```
DeadSpace-R15-PC-Installer-YYYYMMDD.exe
vandal.marker
```

## Paths

| Purpose | Default | Rule |
| --- | --- | --- |
| Program files | `%LOCALAPPDATA%\Programs\DeadSpaceLegacy` | User selectable |
| Installed game data | `%LOCALAPPDATA%\DeadSpaceLegacy\GameData` | User selectable |
| Launcher configuration | `Documents/DeadSpaceLegacy/config/pc` | Not a game-data location |
| Original three save slots | `Documents/DeadSpaceLegacy/touchHLE_sandbox/...` | Preserved across program updates |

The program and GameData directories may not contain one another. GameData may
not be placed anywhere under the user's Documents directory. The installer
writes its selected GameData path to `installer.json` in the program root and
to the current-user product registry key. The launcher reads that path before
falling back to the LocalAppData default.

## Install and upgrade

The installer embeds the lean program payload. It never embeds the original IPA
or texture packs. On first install it requires either `vandal.marker` or an IPA.
A marker next to the installer is auto-detected and filled into the source
field, but installation still requires explicit confirmation.

The launcher performs the actual resource installation through its tested
`--install-resource` path. The marker is opened read-only. The selected base IPA
and texture tier are copied to `GameData/marker-content/<marker-id>` with an
installation record and per-file hashes. Later tier additions preserve already
installed tiers. A direct IPA install has an empty texture-tier list; it must not
invent or enable texture packs.

An upgrade can leave the resource source empty when GameData already has a valid
installation record. Upgrade copies the new program files, preserves unknown
files in the selected program directory, and does not delete GameData or
Documents state. Changing either path requires the user to select the existing
path or install resources into the new GameData location deliberately.

## Integrity and cleanup

- The source marker's SHA-256 must remain unchanged.
- The selected marker entries are size/hash checked while extracted.
- Program extraction rejects path traversal and requires the launcher and native
  runtime.
- Start Menu and uninstall registration use the current user only.
- The uninstaller removes the program, shortcut and registry entry. It does not
  remove GameData or the Documents save/config root.
- The install test uses a synthetic marker plus one scoped actual-marker install;
  neither substitutes for full-game or human acceptance.

## Installer UX revision (2026-11-06)

The installer and launcher's resource page support selecting any combination of
2x/4x/6x textures. No selection installs only the original base IPA. Existing
tiers are preserved. The texture radio buttons on the graphics page still
select one active tier; installing several does not use them simultaneously.
The resource CLI accepts `--tiers 2,4,6` or `--tiers none`; legacy `--tier 4`
remains supported. Complete selections are validated before resource writes.

Desktop and Start Menu shortcuts are independently selectable, both checked
by default. Windows Known Folder paths support redirected desktops. Both links
target the launcher, share its embedded game icon, and set its working directory.
Uninstall removes links only when their target belongs to this installation.
`--no-shortcut` skips both; `--no-desktop-shortcut` and
`--no-start-menu-shortcut` skip either individually. All registration stays in HKCU.

The installer uses a scrolling automatic layout, resource-size/free-space hints,
and a progress indicator. Editing options is disabled while installation runs.
The launcher and installer declare system DPI awareness and run asInvoker.
The launcher keeps its action buttons outside the scrolling settings pages.

`tools/pc-installer/Test-Ui.cs` checks all eight launcher pages and expanded
sections at 100/125/150/200 percent scale, including labels and compact heights.
This is offscreen geometry/render evidence, not physical monitor/DPI acceptance.
`Test-Installer.ps1` exercises multi-selection, eight combinations, upgrades,
direct IPA, and invalid selections. `Test-Shortcuts.cs` verifies real Windows
links and registry registration with HKCU redirected to a disposable test key;
existing user links are backed up and restored in finally.

Windows no longer inherits the macOS-only SDL HIDAPI disable workaround.
Virtual-controller enumeration is regression evidence only; physical Xbox,
PlayStation and Switch controller acceptance remains open.
