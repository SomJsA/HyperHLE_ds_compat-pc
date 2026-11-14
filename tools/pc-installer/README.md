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

R4.1 identifies applications by the SHA-256 of the uncompressed main executable
and Info.plist, plus the required standard / high-tier resource directories.
The source filename is never used as a version identifier. The installer and
launcher share `tools/pc-runtime/ResourceProfiles.cs`:

| Recognized application | Native lighting | External HD packs / enhanced text and controls |
| --- | --- | --- |
| 1.3.34 | Standard / advanced | Matching marker 2x / 4x / 6x; supported |
| iPad 1.0.3 | Standard / advanced | Original textures and native text / touch only |
| iPhone 1.0.3 (build 1.3.8), World 1.0.1 | Standard only | Original textures and native text / touch only |
| Other fingerprints | Unverified native path | Disabled pending adaptation |

All four known applications can use original textures without an external pack.
Resolution, MSAA and AF remain host controls; this is not a claim that every
hardware / high setting was validated. The recommendation and Community issue
entry are bilingual. 1.3.34 is the most fully adapted edition.

Selecting a different IPA or marker completely switches the active resource
selection and its graphics configuration. `GameData/active-resource.json` records
the selection explicitly, so reinstalling a previously cached marker works even
when another content directory has a newer timestamp. Failed selection keeps the
old pointer; invalid or escaping pointers do not fall back to another edition.
Old texture paths are replaced, unsupported components are disabled, and the
first-launch graphics review is reset on a resource change. Existing source
IPAs, markers, cached generations, personal bindings and saves are preserved.
This is an installation record, not a new marker format. A program-only upgrade
keeps the selected resources. `--no-register` updates isolated defaults only and
never the real user's Documents configuration.

Legacy launches keep progress under `legacy-ipa/<profile>` within the selected
save root. In particular, World 1.0.1 must not read 1.3.34's existing sandbox just
because both use `com.ea.deadspace.bv`. Switching back restores the current save
root without copying, overwriting or deleting progress. Unported legacy save
editing and transfer tools are disabled; native saving remains available.

Build:

```powershell
Build-Installer.ps1 `
  -ProgramDirectory E:/path/to/staged/program `
  -OutputDirectory E:/path/to/new-installer-output
```

The output directory contains internal build inputs and the final installer.
Only the final installer and `vandal.marker` belong in the user delivery.
