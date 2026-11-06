# Opening partial-blackout fix — 2026-10-25

## Symptom and discriminating evidence
During the opening camera handover, walls/floor become black while the character, lights and HUD remain visible. User explicitly confirmed that setting TOUCHHLE_DS_FOG_RED=1 turns the faulty black region red. This establishes fog contribution, unlike prior guesses about depth/clipping.

Evidence directory: E:/CodexProjects/DeadSpaceLegacy/logs/pc-opening-fog-red-20261025.
runtime.txt is the captured game log; fog-timeline.txt links changed fog parameters to the most recent presented-frame marker (approximate frame labels, not exact draw frames). red-marks.jpg and obs-record.json retain visual evidence.

Observed LINEAR fog: start10/end25 before transition, then start0.0016622568/end0.004155642, rising to start10/end24.999996. Existing code only suppressed end<=0/end<=start. Tiny positive ranges saturate visible geometry to black while bypassed character atlases remain lit.

## Candidate behavior
src/gles/ds_opening_fog.rs holds context-local temporal state. It learns the known10..25 baseline, recognizes collapse belowend0.25 within2 presented frames of the baseline, and preserves10..25 during the matching start/end=0.4 recovery. It releases at restored baseline or an unrelated range and times out after240 frames. It is not a global minimum-distance clamp.
Backend checks black LINEAR fog and perspective near0.05. Correction wraps each arrays/elements/skinned draw and restores guest start/end afterward. TOUCHHLE_DS_OPENING_FOG_FIX=0 disables it for diagnosis. Normal distance fog stays enabled. No changes to Android native GLES fog in this candidate.

## Validation
Three standalone Rust tests passed: observed collapse/recovery; unrelated and legitimate short fog; timeout/context independence. Release build and live visual acceptance pending. E-policy changes also included in candidate, with existing four policy tests passed; user gameplay acceptance of full-bag skip remains pending.

## Diagnostic lessons
- Clear-time framebuffer readback can be empty after composition. Trigger uses resolved drawable at present.
- Per-draw reads on multisample targets did not produce useful pixel-change logs; no geometry conclusion can be drawn from their absence.
- Draw-limit logs must start at F7 artifact, not a guessed global frame. watch_opening_trace.py gates logs until F5 artifact without UI control.
- A single dark pixel can trigger on legitimate dark content; correlate with user marks and actual frames.
- DUMP_FRAME_EVERY=0 historically meant all frames in clear dumper but disabled present dumps. Present-only route now avoids duplicate legacy dumping and leaves marks for completed-frame capture.

## Historical remaining work (superseded by acceptance below)
Normal-color live acceptance, preserve normal atmospheric reveal; deploy to production only after game exit. Then continue PC-NEXT-PLAN.md launcher/input/save editor work. User prohibits computer-use; gameplay testing remains user-driven. Do not play through progress to build save catalog.

Release build passed (4m46s) and isolated headless startup smoke passed. Candidate builds/pc-opening-fixed-20261025/touchHLE.exe SHA256 d70530da59a974074d8e860364089f283b6d123079413eb703208fc1c79f1fdb. Deployed to isolated opening runtime after user exit; Start-OpeningVerification.ps1 starts normal color, no heavy draw traces/periodic dumps, user marks only. Await live acceptance. Production runtime still holds original accepted QTE/F build.

## Verification launcher defect and correction
First normal-color verification was INVALID: user screenshot still showed red fog. Child-process experiment proved PowerShell [Environment]::SetEnvironmentVariable(name,$null) binds to empty string here; Rust var_os().is_some() treats empty as enabled. Verification script now uses Remove-Item Env:NAME, asserts diagnostic keys absent, and records launch-settings.json with executable hash. No game binary change. Bad run log preserved in logs/pc-opening-fixed-20261025/invalid-red-runtime.txt. User confirmed exit, clean verification launcher started with evidence logs/pc-opening-fixed-clean-20261025, fogRedPresent=false/heavyTracePresent=false. Visual fog-fix acceptance remains pending.

## Clean verification FAILED; waiting-phase correction in progress
User reports blackout unchanged and pressed F5. No DSopeningFog activation log in clean runtime. Snapshot: logs/pc-opening-fixed-clean-20261025/state-1792929899-f004431.dsstate; mark f4432. Do not mark650275ef accepted. Initial guard used a2-frame baseline recency and began240-frame timeout on zero; this fails to model the long zero-range cinematic before positive recovery.
Revised guard remembers baseline across zero-range wait, starts240-frame timeout only at positive recovery, and observes matching GL parameters even with fog disabled. Four tests pass, including exact observed baseline f270 / zeros271..3869 / positive .001662.. .004155.. f3870. Added DSopeningFogCheck one-time raw eligibility/state log so any future rejection is directly diagnosable. Build logs/pc-opening-fog-recovery-20261025-build.txt. User has exited; no candidate yet deployed.
Offline native evidence from failed F5: player31d42940 -> world311ae030 -> camera30283a60; camera+f4=.05, +f8=30. GameWorld baseline+2fc=10, +300=25. Interpolators current+328=2.669194/+35c=6.672985; target+334=10/+368=25. Native InitFog0x1241c stores baseline and initializes interpolators304/338; ClearFogOverride0x121f8 restores baseline targets via interpolation. This supports keeping normal target ranges during recovery rather than turning fog off globally. OBS clip saved in clean-verification/obs-record.json; runtime.txt archived.

Recovery-wait candidate built successfully4m18s; isolated smoke passed. Commit bd949378. SHA2567dbcc24cf145ec6f590e16beb7963d0147b41ffce66ef91efd44dc231531864c. Launched after user exit via verification script, manifest confirms no red/heavy traces. Evidence logs/pc-opening-recovery-20261025. OBS recording requested. Await user opening test and DSopeningFog/Check logs.

## Final opening acceptance (2026-10-25)
User explicitly confirmed the blackout is fixed with bd949378 / 7dbcc24cf145ec6f590e16beb7963d0147b41ffce66ef91efd44dc231531864c. Runtime log confirmed DSopeningFog activation. Nearby fog regression remains user-driven and pending; current runtime now has a copy of production saves. Do not close the game. Latest exact handoff: PC-HANDOFF-NOW.md.
