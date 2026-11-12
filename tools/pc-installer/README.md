# DeadSpace R15 PC installer

The installer is a single self-contained Windows executable. It embeds the
lean program package and keeps `vandal.marker` as the second and only delivery
file.

The default program root is:

`%LOCALAPPDATA%\Programs\DeadSpaceLegacy`

The default game data root is:

`%LOCALAPPDATA%\DeadSpaceLegacy\GameData`

Both paths are user selectable. Documents remains reserved for launcher
configuration and the original three save slots. Program updates overwrite
program files but do not remove GameData, Documents configuration, saves or
unknown files in the selected program directory.

The first installation requires a `vandal.marker` or IPA. A marker beside the
installer is detected and filled into the source field, but is not installed
until the user confirms. The source marker is opened read-only. After install,
the launcher reads the installed GameData and no longer needs the marker until
the selected resource generation is changed.

An existing installation can be upgraded or downgraded with only the new
installer EXE. Leave the resource source empty; the installer follows the
registered program and GameData roots, replaces program files, and preserves
the installed IPA, textures, configuration, saves and unknown files. A marker
is needed again only when adding or changing resource tiers.

Build:

```powershell
Build-Installer.ps1 `
  -ProgramDirectory E:/path/to/staged/program `
  -OutputDirectory E:/path/to/new-installer-output
```

The output directory contains internal build inputs and the final installer.
Only the final installer and `vandal.marker` belong in the user delivery.
