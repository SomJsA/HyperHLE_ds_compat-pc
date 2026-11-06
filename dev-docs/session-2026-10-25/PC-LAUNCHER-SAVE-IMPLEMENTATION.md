# PC launcher and save editor — 2026-10-25

> Superseded status summary: read [PC-HANDOFF-NOW.md](PC-HANDOFF-NOW.md) first. Final staged UI has six pages including keyboard remapping. `KeyBindings.cs` and `src/ds_key_bindings.rs` now implement 15 keyboard actions; three parser tests and final release build passed. Runtime is not deployed or smoke-tested. Native weapon/mode/HUD action work is investigation only. User requested immediate handoff, so implementation stopped.

## Live session boundary

User is testing nearby fog using the accepted opening fix. Do not stop their game,
replace the active executable, change active saves, or open a foreground preview.
No computer-use. New launcher artifacts remain in
`E:/CodexProjects/DeadSpaceLegacy/builds/pc-launcher-next-20261025`.

## Launcher

Versioned source of truth: `tools/pc-runtime/Program.cs`, `LauncherShell.cs`,
`SaveSettings.cs`. Build with `Build-Launcher.ps1 -OutputDirectory <staging-dir>`.
Retains the existing validated renderer environment setup and INI field names.
Native WinForms, near-black blue numbered sidebar, five pages. Postprocessing
presets explicitly set FSR1/CAS fields; no invented temporal upscaler support.
F5 is labelled a diagnostic snapshot, not a normal gameplay save.

Offscreen verification constructs this app's own controls and renders to bitmaps;
it never calls Show/Activate or reads the desktop. Results under staging/verification.
Visual page 0 reviewed: no clipping at 1000x740 client area. Other pages rendered,
not yet all visually reviewed. New launcher is not deployed to the live runtime.

## Critical correction: DLC is NOT a bitmask

Prior outer `SAVE_FORMAT.md` / `ds-dlc.cmd` / `sbobjects --set-dlc` assumptions are
wrong. Do not use those mutation commands. This supersedes their mask advice.

Evidence from backed-up production settings (968 bytes):

- Top-level `dlc_modules` type 0x0f is OHDR reference 1, flag 1 (an object).
- Its named fields: `nodes_2`, `nodes_1_registration`, `attack`, `defense`, `loot`.
- Each refers to an object with an `unlocked` type 0x09 field at relative offset12.
- These objects declare size13: boolean occupies one byte; following CD padding
  is NOT part of the boolean. Change only the first byte, preserve all padding.
- Checkpoint `dlc_modules` flag2 is a type0x0d string-ID list: count2, names
  `nodes_2`, `nodes_1_registration`. It is neither an enabled bitmask nor a scalar.
- Native `SaveGame::HasDLCModuleBeenApplied` at0xf6718 iterates strings;
  `GameObjectPlayable::CheckDLCModules` at0x49a8c consumes installed modules.
- Settings top-level block starts at DATA+4 after an empty four-byte block.
  Name/boolean widths are2; int/float/reference widths4. A field may have2-byte
  alignment padding before its value. Stored top-level offset points4 bytes before
  the value, unlike nested object offsets which point directly to the value.

`SaveSettings.cs` validates SBINv2 bounds and FNV-1 section checksums, resolves
named object references, edits only attack/defense/loot unlocked bytes, recalculates
DATA checksum, and preserves every other byte. Write refuses any active touchHLE
process, checks source unchanged twice, verifies temporary output, and uses
File.Replace with a unique backup in the same directory. No live save edits made.
The setting is shared by profile slots in that runtime; UI states this explicitly.

Verification: all8 toggle combinations parse back correctly; reverting each is
byte-identical to the original fixture; <=7 changed bytes (3 flags +4 checksum);
corruption rejected. Tests use the backup at
`builds/save-backups/near-fog-20261025-202434/production-source/.../settings.sb`.
Actual gameplay enable/disable effects still need user acceptance. Do not edit
checkpoint applied-module history until the native application/removal semantics
are confirmed. Inventory and progress editing remain unfinished.

## Input correction and pending work

Earlier inventory incorrectly claimed no relative-mode API: actual source calls
`SDL_SetRelativeMouseMode` in `src/window.rs`. Host mouse input already uses relative
deltas; guest movement/look still uses touch gestures. These are distinct layers.
Current production INI had mouse_look=off. New UI exposes the existing modes.

Added focus gating and immediate relative-mode release on FocusLost; discard
pending look deltas and release guest look finger. Mouse capture requires an
explicit F8/Alt toggle when returning. With mouse mode Off, controller look remains
able to resume. Runtime source change is not yet deployed or manually accepted.
Keyboard movement focus cleanup, key/controller remapping and lower-level guest
movement/look investigation remain pending. Avoid presenting touch emulation as
native engine input.

## Next

Finish remapping and input-mode selection, verify runtime build; inspect native
DLC effects before claiming all three gameplay toggles complete; extend item editor
only with schema validation. Progress contact sheets need user-controlled loading
of existing saves, never playthrough. Preserve the user's new diagnostic-profile
progress before later deployment.
