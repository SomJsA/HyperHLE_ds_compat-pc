# Native UI source integration (R8, 2026-10-31)

The R7i executable passed isolated native station regressions, but its source
still depended on untracked files and unstaged entry points. A successful build
of that dirty working tree did not prove that a checkout contained the fixes.
This integration packages the native UI dependency chain separately from the
remaining performance and unrelated working-tree changes.

## Included behavior

- Verified original menu, loading, Store, Bench and confirmation callbacks own
  input. Loading accepts E/Enter/A only when the original handler is ready.
- Widget positions include native parent, viewport and scroll transforms.
  Weapon rows and current visible upgrade grids exclude hidden overlapping
  grids. Returning from a popup restores the same owner's row/node once ready.
- Confirmation/back actions retain adjacent Kenney sprites and activation, but
  do not enter directional focus. Mouse clicks keep the original native path.
- Gameplay subtitle cards yield to native menus/stations/dialogs. Keep that
  ownership gate in both CPU and GPU presentation paths.
- G/Z and L3/R3 request verified original locator/quick-turn actions, with no
  synthetic double taps or player/tutorial state writes. C remains stasis and
  retains the acquisition fix from e09c52f2.
- UI ownership and focus loss cancel held gameplay input. The existing window
  cancelled-touch event now reaches UIKit as cancellation, including gesture
  cancellation, rather than running an ended-touch action.

The native shortcut implementation and the UIKit cancelled-touch consumer were
missing from the committed dependency chain. The isolated build exposed both.
Only the cancellation-specific UIKit hunks were included; other UIKit changes
remain separate.

## Evidence and limits

A private Git index based on ecb3e499 selected the UI patches. Its source archive
was compiled in `builds/r8-ui-source-review`, without substituting candidate
source into the main working tree or changing the real index or human runtime
during the check. The command was:

```powershell
cargo test --release --offline --lib ds_
```

Result: **100 passed, 0 failed**, exit 0. Rust source in that directory matched
the private index after CRLF normalization. Existing native wrapper dependency
paths were redirected only in the build copy's Cargo.toml to reuse the local
cache. Vendor sources were shared; this is not certification of a clean vendor
checkout or another driver. The staged whitespace check also passed.

The workspace `perf-continuation-20261030/R8-UI-INTEGRATION` keeps the candidate
tree/selection/source hashes and all three test logs. The first two checks
failed on missing dependencies or omitted archive support files; they were
retained, corrected, and followed by the passing third check.

The R7i full-working-tree evidence remains in
[PC-R7-NATIVE-UI-REGRESSION.md](PC-R7-NATIVE-UI-REGRESSION.md): 118 DS tests and
isolated real Store/Bench transactions. Its 118-test count includes additional
working-tree features outside this UI split. Neither count proves physical
keyboard/gamepad acceptance, fresh tutorial progression, or active-caption
visual correctness. Those remain on the human test checklist.

This source integration is **not a release candidate**. The active R7i human
runtime, its binary, portable saves and launcher remain unchanged. The remaining
accepted visual/performance repairs must also be integrated before producing a
release; do not rebuild an interim source commit and replace R7i with it. GPU
copy policy, loading-state sync A/B, quality matrix, other GPU/driver validation
and final PC/Android release remain pending. Tablet ADB is reserved elsewhere.
