# Stasis acquisition gate (2026-10-30)

A new-game first-chapter checkpoint pre-creates WeaponStasis with clip=1 but available=0. The C/gamepad/inbox shortcut dispatched action 0x0b while the original HUD hid the button, spending that charge and then flashing the empty-energy warning.

The shortcut now checks the same acquisition byte as original CanStasis (0x3b080): current player's WeaponStasis (+0x30c), available (+0x164). The gate runs before both native-action and diagnostic-touch paths. It does not check chapter or remaining charges, so inherited first-chapter stasis remains usable and an acquired empty weapon retains native failure feedback. No save/player state is written.

Validation: 116 DS Rust tests; 33 original ARM CanStasis read-only cases; six original real-vtable OnTryFire cases reproducing the unowned charged shot and acquired-empty result 2. Real isolated first-chapter saves: unacquired repeated C leaves hidden charge 1->1; inherited acquired stasis fires 4->0 and stays empty. Candidate C86E665E347290CAA8AF7EF43331B5D44071F53BBE839354243D904C14AE3E6F passed both. The helper was subsequently isolated from the uncommitted G/Z module to keep this commit focused; offline cargo check passes for that final source.

Evidence: workspace perf-continuation-20261030/R6-live-stasis-{newgame,ngplus-ch1}/result.json; r6-stasis-original-arm-standalone.json. Workbench/store navigation and prompt/subtitle issues are separate unfinished work. Preserve this correctness gate across performance presets and remapping.
