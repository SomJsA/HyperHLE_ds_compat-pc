# General F7 diagnostic dump / PNG bookmarks

2026-11-07 GPU Test1 user revision: automatic diagnostics and the performance
HUD default off. The first upgrade to policy `manual-f7-20261107` resets their
old default-on values; later explicit saved choices are respected. F7 remains
independent: no manual session/worker/files are created before a press. The
separate on-demand client never populates ACTIVE or ordinary collectors. It
saves numbered PNG, request/final JSON and minimal session/build identity;
it does not record prior/post frames, resource events, periodic metrics or
automatic incidents. Missing history is explicit, not manufactured as a30s
capture. The current session's manual records are included in normal export.
Existing automatic mode still uses the original before/after pipeline when
explicitly enabled. Both clients share bounded admission/normal shutdown and
the screenshot readback/PNG orientation logic; native pause is not invoked.

Regression: `manual_worker_saves_only_explicit_captures_and_stays_idle` writes
a real PNG, ignores unsolicited Frame/Event inputs, verifies no periodic frame/
timeline/health files or idle changes, and drains normally. The real hidden
launcher test `PcManualProblemDefaultsTest.cs` covers fresh/missing/legacy-on
defaults, explicit on/off after migration, launch environment, no diagnostic
directory creation while off, and save/reload. Native F7 and automatic-mode
shutdown regressions remain required for the actual installer.

Latest user clarification: use the earlier diagnostic-dump stutter, not native pause. F7 triggers a deliberate one-off dump stall, captures the current game frame, saves numbered diagnostics, automatically continues and shows the problem number. No pause-menu call, native state change, OS suspension, E interception or manual resume. This supersedes all earlier native-pause proposals. Every press gets a new issue number; the key is for all gameplay issues, not only QTE.

F7 and explicit inbox `issue` share the same request. Each session allocates monotonically numbered issue-0003-before.json, issue-0003.png and issue-0003.json; session directory identifies the run. Background writer immediately saves available30s pre-history, encodes/writes one explicitly requested game output viewport PNG, and later finalizes about3s of available post-history. Capture occurs at the next completed present before swap. No automatic screenshots/desktop capture/ordinary key trace. Existing M/legacy mark remain separate explicit frame tools.

The render owner does one bounded128MiB RGBA readback, restores PACK_ALIGNMENT and waits for actual pre-context+PNG completion up to2s, with no forced minimum stall and no wait for the3s post window. Background writer owns PNG encoding/IO. Capture queue<=4, pending context jobs<=16; one RGBA buffer at a time. Failures/timeouts are explicit; gameplay automatically continues. PNG is top-down without changing STB globals. Existing files use create_new. Success/partial/error notices reflect actual writes; no missing screenshot can be reported fully saved. The saved-before file survives if a later tail/capture is unavailable. Early exit preserves available data and labels partial coverage.

Dump/readback/wait cost is tagged explicit_issue_capture_readback / explicit_issue_dump_wait and must not be blamed on normal gameplay or mixed into observer-off FPS claims. All8actual GL21 ReadPixels sites in the Windows GLES1-on-GL2 backend are observed exactly once; same calls/arguments, no additional reads except explicit F7. Other backends remain a coverage limit. New runtime_wall_span14=native QTE input,15=tap geometry/cache,16=swipe geometry render observation are inclusive nested wall spans. Native_qte_stage values include player state, raw grapple stage, observed present; state9 stage5=tap,1/7=swipe, unknown stages retained. No fixed0.5s trigger.

Launcher's recording status requires fresh health and positive persisted frame/event counters after flush; not just a requested flag/directory. Export includes all issue JSON/PNG and the full session set. Testers provide a number or simply upload all sessions; missing/unsupported measurements and coverage gaps are disclosed. Backend growth/readback, launcher start/export, disabled/writer-error and native gameplay/QTE dump tests are required before distribution. i5/4060 tester version/exact config remains unknown. Screenshot/context numbering, PNG orientation/bounds/conflicts and partial-exit units have passed before this final no-native-pause simplification; rerun current source and actual native validation pending.
