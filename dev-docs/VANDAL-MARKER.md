# vandal.marker distribution format

`vandal.marker` is the compressed external data container for DeadSpace Legacy
/ HyperHLE DS Compat. The file is a ZIP64 archive despite its extension. It
contains exactly two kinds of data: the unmodified original IPA and the final
2x/4x/8x texture packs. No derived IPA is required or created at runtime.

The launcher is distributed separately (planned for GitHub). Translation,
fonts, overlays and the emulator program belong to that launcher, not to
`vandal.marker`.

## Required layout

```text
vandal.marker
  vandal.json
  payload/DeadSpace.ipa
  payload/texture-packs/2/...
  payload/texture-packs/4/...
  payload/texture-packs/8/...
```

`vandal.json` has `schema: 1`, `kind: "deadspace-vandal-marker"`, product and
marker versions, minimum launcher versions, the game bundle identity, target
game version `1.3.34`, and one entry per payload file with size and SHA-256.
`payload/DeadSpace.ipa` must be the exact original IPA bytes.

## Launcher behavior

1. The separate launcher searches for `vandal.marker` next to itself and in
   the documented Documents/DeadSpaceLegacy location. It does not recursively
   scan the disk.
2. If absent, ask the user to choose a `vandal.marker` or a direct `.ipa`.
3. A direct IPA is a supported compatibility path and runs without requiring a
   marker; built-in translation/font resources remain available.
4. Parse and validate the marker manifest. Missing, duplicate, absolute,
   traversal, symlink or malformed entries are rejected.
5. SHA-256, size, marker-version or launcher-minimum mismatches produce a
   prominent warning and require an explicit Continue choice. They do not
   silently fail the launch.
6. Extract only selected payload entries into a content-addressed cache. Never
   modify the marker or the original IPA. Reuse an already verified extraction.
7. Saves default to Documents/DeadSpaceLegacy. Save import/export uses the
   existing three-slot ZIP format on both PC and Android.

8. Installation is optional. When the user installs from a marker, base data
   is mandatory and texture tiers are independently optional. The installer
   UI calls the game payload "base data" rather than asking about an IPA.
   Installation never modifies the marker, hashes every copied entry, stages
   writes, and publishes by content-addressed `marker_id`.

9. Launcher navigation always exposes Game Data / Install / Change / Upgrade
   and Direct IPA actions. The requested texture choices are 2x, 4x and 6x.
   The launcher exposes only tiers declared by the marker and never relabels
   one tier as another. The immutable v2 candidate predates this choice and
   declares 2x/4x/8x; a new marker is required to publish 6x.

10. Ordinary startup validates the manifest and entry metadata without hashing
    the payload. Install, change and upgrade hash only the base data and
    texture tiers selected by that operation. A full archive hash scan is a
    separate release-maintenance action.

11. The distributed marker is immutable. Do not patch or rewrite it in place.
   New content is published as a new version with a new `marker_id`. Saves and
   extracted caches live outside the marker.

12. A different direct IPA version is accepted with a prominent warning that
   DeadSpace Legacy was developed against iPhone 1.3.34.

## Tooling

```powershell
python tools/vandal-marker/vandal_marker.py create `
  --input path/to/marker-root --output path/to/vandal.marker `
  --version R15

python tools/vandal-marker/vandal_marker.py verify path/to/vandal.marker `
  --launcher-android 1 --launcher-pc 1 --base-data --tier 4

python tools/vandal-marker/vandal_marker.py verify path/to/vandal.marker --full

python tools/vandal-marker/vandal_marker.py install path/to/vandal.marker `
  --output Documents/DeadSpaceLegacy --tiers 4

python -m unittest discover -s tools/vandal-marker -p "test_*.py"
```
