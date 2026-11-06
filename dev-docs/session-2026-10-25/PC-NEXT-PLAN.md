# PC follow-up: rendering, launcher, input and saves

> Latest status: [PC-HANDOFF-NOW.md](PC-HANDOFF-NOW.md). Opening fix accepted; user testing nearby fog with copied production saves. Launcher/save/keyboard changes staged, not deployed. User requested immediate handoff documentation; new implementation stopped. Subsequent sections retain historical experiments and must not override the latest handoff.

Updated 2026-10-25. User priority: fix opening partial blackout FIRST.

## Confirmed acceptance
F telekinesis on newly spawned monster drops; E large/small grapple and full-body swarm QTE. Runtime title DeadSpace IOS移植. E-policy candidate compiled and smoke-tested, awaiting gameplay acceptance: QTE > pickup > scene, no E telekinesis, failed pickup suppressed 3 seconds.

## Active experiment
Isolated opening runtime: E:/CodexProjects/DeadSpaceLegacy/builds/pc-opening-diagnostic-20261025.
Started through tools/pc-runtime/Start-OpeningCapture.ps1 after user confirmed exit.
User performs new game, F7 before blackout, F5 after blackout, then pauses. Evidence: logs/pc-opening-mark-20261025. No computer-use. Current hypothesis is unproven; partial environment loss differs from full black and Android far-plane issue. Do not label diagnostic hooks a fix.

## Authorized next work, in order
1. Capture and diagnose opening environment disappearance; implement narrow rendering fix; compare before/after.
2. Native PC launcher, read-only reference to WII thread 01a16ead-f0e6-7872-87b9-7ba5a63767be and Android thread 01a16f39-97be-75e1-a7eb-f43d099431cc. Near-black blue, numbered sidebar, restrained cyan, original iOS identity. Independent window/runtime controls above graphics presets. Postprocessing profiles must reflect actual supported rendering options.
3. Mouse capture with explicit release and focus-loss behavior; inspect existing keyboard/mouse and controller designs before replacing them. Investigate lower-level movement/camera input, preserve analog controller behavior and expose remapping. Do not merely relabel simulated touch as native input.
4. Save editor minimum: current-save toggles for three paid plugins. Discover actual schema, preserve unknown fields, backup before atomic writes, refuse live-game writes. Extend to items/progress only after validating fields.
5. Progress catalog: user explicitly prohibits playing through progress. Load existing checkpoints individually and produce labelled long contact sheet for user selection. No gameplay automation; clarify operational loading method if needed.

## References, read-only
C:/Users/somjs/Documents/ChatGPT/DSWII/launcher/README.md
ANDROID-LAUNCHER-REDESIGN.md in this folder.
WII reference includes WPF/C# services, per-profile preferences, graphics preset readback and remapping validation. Android includes original iTunesArtwork and backup-based save import; save editor was not implemented.

## Handoff discipline
Keep findings, exact evidence paths, build hashes, accepted vs pending behavior, and next experiment current. Scope commits to our changes; extensive preexisting formatting/SDL changes remain. No modification of reference projects. See PC-QTE-KINESIS-FOLLOWUP.md for native addresses and accepted build.

## Opening capture 1 — 19:29 local
User pressed F7 at f3618 and F5 at f3905 exactly during the blackout; cannot pause because control is still cinematic. F5: logs/pc-opening-mark-20261025/state-1792927740-f003905.dsstate. OBS video in obs-record.json, 976–1006s overview in obs-opening-timeline.jpg. Partial world loss confirmed at 990–996s; returns by 998s. Recorded duration is stretched by diagnostic overhead, not a timing measurement of normal build.
Diagnostic failure: legacy clear-time dump and trigger read wrong/empty FBO; every=0 means all frames in legacy clear dumper, but disabled in present dumper. F5 mark consumed by clear dumper; F7 first mark correctly at present. Do NOT infer full-screen black from raw clear dumps. Correcting to resolved drawable FBO at present; suppress legacy dumps/mark consumption when DUMP_AT_PRESENT set. New optional GLES diagnostic hook defaults to no-op; no renderer behavior fix yet. Build log logs/pc-opening-resolve-20261025-build.txt. User asked to exit after evidence captured. OBS recording stopped after saving clip.

## Existing implementation inventory (read-only)
- src/window.rs MouseMotion consumes xrel/yrel when ds_look_on, then emits a touch drag; no SDL relative-mode/capture call found. Need distinguish host raw relative input from guest touch emulation.
- TODO-INPUT-2026-10-21.md records native action paths; its older failure list is superseded by current user acceptance.
- Outer SAVE_FORMAT.md section 20 supersedes earlier layout guesses. tools/ds-save-stats.py and sbobjects.py already parse typed checkpoint objects and can edit quantities/types and dlc_modules. Existing ds-dlc.cmd touches all saves, with obsolete sandbox root: do not expose it directly as a current-save editor. Validate three bit assignments and preserve other bits. Item insertion/removal was not implemented; changing existing slots was validated historically.
- Older notes suggest PostMessage/mouse automation; these are reference only and do not override current NO computer-use constraint.

Second diagnostic build succeeded in 4m13s and launched after user exit. SHA256 56b165a5452ea7bb76a86bb6753482e40939f7a9b36b2b1e937e55c062e626e6. Evidence logs/pc-opening-resolve-20261025; resolved-FBO trigger, present frames every30, capped terse texture traces after f3000. OBS recording requested again. Await same F7/F5 user test. No visual fix claimed.

## Capture 2 result / capture 3 active
Resolved-FBO trigger correctly fired f8971, F5 f8977, full mark f8978. marks-review.jpg confirms partial geometry loss; trigger image needs -90deg orientation (the first review rotated +90). Snapshot state-1792928525-f008977.dsstate. Pixel-per-draw trace emitted no changes: draws still target multisample source, so per-draw ReadPixels cannot be treated as evidence of no rendering. Terse draw log hit 12000-signature limit at f6714 before user F7 f8819. Thus no root-cause assertion yet.
After user exited, runtime log archived to logs/pc-opening-resolve-20261025/runtime.txt. Same executable relaunched with evidence logs/pc-opening-gated-20261025. New watch_opening_trace.py creates DRAW_TRACE_TRIGGER only when F7 present-time artifact exists; removes after F5 snapshot appears or 120 seconds. No UI control. Draw limit40000, no guessed frame threshold, UV_PROBE_ON_DARK adds geometric point-hit records. User asked to repeat same F7/F5. OBS recording from capture2 still active (started by us); stop and save path when capture3 complete.

## Capture 3 result and next discriminating test
F7 f2927; F5 request f2988, mark f2989. User clarifies F5 after black returned bright. Gate captured 4106 unique draw rows f2927..2988 and91 UV hits. Saved draw-trace.txt/uv-trace.txt/runtime.txt, transition-review.jpg. Sparse every30 frames does not itself show the brief blackout. Pixel trigger f2950 fired on a naturally dark pixel before actual fault; do not treat it as exact black onset! UV hits include interior atlas60bb1a47 at z .98 and exterior batches with crossing near/far depths. No definitive clipping/depth root cause.
Important code lead: nofog_begin already disables degenerate LINEAR fog when end<=0 or end<=start. A transition from positive end towards zero may temporarily saturate whole interior black before bypass engages; character atlas56c62e8a exempt from fog, matching symptom. Next test FOG_RED=1 plus LOG_FOG=1 to distinguish actual fog contribution. No fix inferred yet. User requested exit for this no-build experiment. OBS clip stopped, output recorded in capture3/obs-record.json (contains capture2 and3).

## Root cause established by red-fog test
User confirms the formerly black region turns red with FOG_RED=1. Logs show normal LINEAR 10..25 fog collapses to start=.0016622568/end=.004155642 then ramps back to10..25. Existing end<=0 bypass missed tiny positive ramp values. Source/log evidence in logs/pc-opening-fog-red-20261025 (runtime.txt, fog-timeline.txt, red-marks.jpg, obs-record.json).
Candidate implementation src/gles/ds_opening_fog.rs: per-context state learns10..25, requires abrupt collapse within2 frames, holds learned range during matching0.4 ratio recovery, releases at normal range, bounded240 frames. Backend further restricts to black LINEAR fog and perspective near=.05. Restores guest GL fog settings after each draw, including skinned early return. Does not disable normal fog globally. Opt-out TOUCHHLE_DS_OPENING_FOG_FIX=0. Three pure regression tests passed. Build ongoing logs/pc-opening-fog-fix-20261025-build.txt. Candidate visual acceptance pending; user asked to exit red test for normal-color fixed build. OBS red clip saved and recording stopped.

## Latest status 2026-10-25 20:24 local
User accepted opening blackout fix bd949378 / SHA256 7dbcc24cf145ec6f590e16beb7963d0147b41ffce66ef91efd44dc231531864c. Near-distance fog regression remains pending. User requested a saved-game launch because isolated new-game profile cannot reach later chapters. Game had already exited (no touchHLE process); backed up both profiles under builds/save-backups/near-fog-20261025-202434, copied production Runtime-PC sandbox into diagnostic runtime, verified all 13 top-level var files by SHA256. Production saves unchanged. Opened verification launcher with F5/F7 evidence logs/pc-near-fog-saved-20261025. User controls gameplay; do not close game or take foreground while they test. New progress belongs to the diagnostic profile; preserve it before future deployment. Continue launcher/input/save-editor background work.
