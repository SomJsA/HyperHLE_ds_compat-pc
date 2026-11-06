# R15 performance work — active

User authorization 2026-11-02: sustained work without a budget ceiling; reverse engineering permitted. Android target is3x native rendering, with texture packs/upscaling permitted. PC target is3x native rendering plus4x atlas pack,2x/4x MSAA and modest2x/4x filtering, targeting stable60 real frames. Compare60/70/uncapped actual game progression; do not change the game clock or report synthetic frames as performance.

This commit removes two avoidable presenter costs while keeping fallback controls:

- CPU compatibility transfer retains its RGBA scratch allocation and texture storage. Same-size frames use TexSubImage2D; first-use/resize still defines storage. Pixel format/order, readback, filtering and explicit CPU-path Finish are unchanged.
- GPU framebuffer copy and later sampling remain in the same GL command stream, so they no longer force CPU completion before every copy. `TOUCHHLE_DS_PRESENT_COPY_FINISH=1` restores the prior wait for matched driver testing. Cross-context texture upload and Android GLES/Vulkan/AHB synchronization remain unchanged.
- Android adopts the PC absolute-present-deadline contract: composition/readback/swap time is deducted before sleeping. The old Android path computed a relative delay before presentation and slept that obsolete full duration afterwards. No clock multiplier is introduced.
- PC launcher/control inbox/runner and Android launcher expose70 FPS alongside30/60/uncapped. Existing saved user choices remain respected.

Primary synchronization references: [OpenGL2 specification](https://registry.khronos.org/OpenGL/specs/gl/glspec20.pdf), [Khronos synchronization](https://wikis.khronos.org/opengl/Synchronization). This same-context argument does not license removing cross-context synchronization.

Six pure deadline/storage/wait-policy tests pass. Full PC DS regression suite173passed/1optional ignored at the first build checkpoint. Further native build/device/image/performance checks are **pending**; do not treat this commit or a successful build as final acceptance. GPU-copy and GPU-text production defaults are not promoted by this commit.

Current machine evidence: `E:/CodexProjects/DeadSpaceLegacy/report/performance-push-20261102`. The initial GPU-copy ABBA observation was ~14.0ms CPU-copy versus8.44ms GPU-copy at4x/MSAA2 in the same Store room. Captures were coherent, but a dependency build overlapped; classify as exploratory and repeat without build contention at the requested3x/4x-atlas target. Native timer/run-loop disassembly is recorded there; no guest timing patch applied.

OriginalIPA, accepted releases, UI/text/input/save contracts remain protected. Further work includes controlled70/uncapped game-time validation, target3x+4x-atlas/memory/performance, Android3x/device validation and optional textures, heavy/cross-level correctness, shader/copy fallback and remaining original acceptance gates.

## Opt-in original game-time observer

`TOUCHHLE_DS_GAME_CLOCK_TRACE=1` verifies the original ARM instructions at
M3GApplication::OnUpdate (0x530a84) and Timer::Tick before arming a process-local
breakpoint observer. The original STR r0,[sp] is replayed with the same value;
registers/flags and all game-clock values remain unchanged. Every two seconds
it reports native accumulated milliseconds versus wall time, update count,
min/max delta and invalid negative deltas. First/owner-change deltas are
excluded because they predate the observation window. Unknown binary words
disable the observer. No original IPA is changed. This diagnostic incurs
breakpoint overhead: use it for game-speed comparisons, and disable it for
final throughput samples. Two pure metrics tests cover half-speed detection,
negative values and independent windows.

The first Android build caught a PC-only telemetry call accidentally included
in the present-local port; it was removed. That failed build is not a release
candidate. Full Android rebuild and native observer validation remain required.

## Actual native survival and complete PC frame journal

The user clarified the requested play method: select the native **5 Minutes
to Kill / 夺命5分钟** mode and run randomly until death. This is not a requirement
to survive five wall-clock minutes. The native mining arena was selected through
the menu, with05:00 and04:51 captures, movement, enemy grapple state9 and native
death/results state26. Four continuously controlled PC trials reached death in
24.0/40.0/39.2/23.8 seconds. They retained3x rendering,4x textures, actualMSAA4
andAF4. The last uncapped trial, without mid-run screenshots, measured97.89FPS
over15.66 seconds before the death/results window. These are short, dynamically
diverging samples with probe overhead, not an all-scene stability or causal
clock/cap comparison. All source saves remained unchanged.

PC now adopts Android's bounded `TOUCHHLE_DS_FRAME_TIMES=<new CSV path>`
diagnostic at the actual guest-present boundary. The shared journal uses
create-new semantics, buffered writes, a one-million-interval bound and disables
itself on I/O error. It preserves long loading/suspend intervals; offline
analysis must explicitly select the gameplay window. The HUD rolling history
is unchanged. Existing captures from before this change have snapshot
percentiles only and must not be relabelled as raw whole-run percentiles.

An Android loading-tip sample was incorrectly classified as gameplay earlier;
all four such samples are explicitly rejected. An objective event or a player
pointer can appear underneath that tip. Actual timer/world images and native
state progression are required together. Device retries use fresh isolation
journals and restore original APK/data/preferences with byte verification.

Latest local-PC stress configuration requested by the user:4K output, ultra
native rendering and8x. The next local arm explicitly requests3840x2160 output,
8x internal rendering (native3840x2560 landscape),8x texture pack,MSAA4/AF4,
native presentation without FSR/frame generation, in the same survival mode.
The earlier3x/4x captures remain baseline evidence. Effective dimensions,
texture uploads and fallback behavior must be checked before reporting results.

## Residual QTE scan found by actual8x combat

The baseline native-state trace (`pc-actual-play-4k8x-05-state`) captured the
whole sequence through firststate26, including grapple state9. Full-run
uncapped/70/uncapped averages were85.29/64.97/84.73FPS, but each grapple
contained36 frames exceeding50ms, including warm repeats with the monitor off.
Low-frequency probe boundaries had missed some of this heavy phase. The new
analysis retains exact native state-change frame counters; do not replace those
results with a lighter last-non-death-probe window.

E's main native interaction path had already moved to memory, but the separate
`ds_publish_qte_overlay` still called the legacy colour-ring finder. At8x it
read40MB and ran multiple full-frame mask/component passes over9.83million
pixels. If detection returned no hint, it retried each frame. This residual
path was overlooked; generic gameplay-input assertions did not cover it.

The new PC QTE path removes both tap/swipe framebuffer searches. Tap bounds
come from the native `INNER_CIRCLE_TAP` entity in Hud+0x1d0 via the original
Layout::GetEntityRect. Swipe bounds observe the native, already-clamped
SpriteGraphics translation/scale/rotation and original TextureImage size.
The process-local observer replaces only MOV r0,r4 at0x18bdfc, guarded against
the original MOV/BL/function signatures, and replays that MOV with unchanged
other registers/flags. It never alters game/QTE timing, events, health or IPA.
Unknown code or missing geometry leaves the original native glyph visible;
there is no automatic pixel-scan fallback. Native spatial/rotation conversion
tests3PASS. Compile, full regression and actual8x UI/performance validation
are still required before claiming this fixes the observed long frames.

Original disassembly and symbols: `report/performance-push-20261102/` files
`player-render-grapple.asm`, `hud-swipe.asm`, `hud-tap.asm`, `layout-rect.asm`,
`image-region-rect.asm`, `sprite-rotate.asm`, `qte-hud-xrefs.json`. The two native
callers distinguish grapple from stomp/slash; no actor position or input gate
is synthesized.

Native signature verification caught an important tap-layout detail before
runtime packaging: RenderHUDTap's originX is175.0 times the original tier
pixel fudge factor, rather than zero. The implementation now mirrors that
native call, reads the factor through the original getter and guards the
175.0 literal. Seven original binary checks and the corrected source compile
check pass. The first native build was deliberately cancelled before a
candidate was produced; its logs are preserved, not reported as a test failure
of a release or used for performance conclusions.

The corrected memory build (implementationc4b7a8c3,EXE70511ca26dcbadd7e9d945045b5c267095108a780072ccf8823554916b8f0a9f)
passed179 DS tests and actual4K/8x native timed play through death. No legacy
ring-finder call was logged; original swipe parameters were observed. In the
warm uncapped grapple phase, p99 fell from59.028ms to19.169ms and >50ms frames
from36 to0. The70FPS grapple sample averaged70.004FPS,p99 19.986ms,max26.836ms.
Cold first-use still had3 grapple frames >50ms; all-scene stability is open.

Visual regression07 caught padded native arrow bounds making the E/A plate
too large. The anchor remains native; tap/swipe labels now retain the prior
52x32 minimum keycap plate independent of transparent texture margins. It also
caught an early E test during the grab intro, before an actual QTE prompt.
The revised test waits for native prompt publication, then checks native
gesture dispatch; controller completion alone is not acceptance. The stale
"E will look for a ring" log was corrected to describe its native context action.

## First multicore resource stage

The user explicitly asks for multicore optimisation. Pure alpha-aware RGBA
mip reduction now splits independent output rows across scoped worker threads
for output levels >=1024x1024. Default up to8 workers onPC,4 onAndroid, bounded
by available CPUs; TOUCHHLE_DS_TEXTURE_WORKERS=1 retains serial execution.
Small levels remain serial. Workers borrow immutable pixels and disjoint
output slices; they perform no GL calls, guest access, game clock/state or
input mutation. All workers join before upload. Failed thread creation falls
back to recomputing the level serially after successful workers have joined.
Integer reduction/transparent RGB/alpha order remains bit-exact. Five pure
tests cover original golden chains, malformed input, thin axes, odd edges
and serial-versus0/2/4/8-worker byte parity, including a2053x1027 texture.

On thisPC, an8192x8192 full mip-chain benchmark(warm median3 samples, warmup
separate) measured243.116ms with1 worker,76.060ms with4,46.062ms with8.
Windows process CPU/wall ratios were0.97/3.39/5.77 active cores, respectively.
The final pixel matched; full-byte parity is independently tested. This proves
parallel resource computation, **not** parallel ARM gameplay or a combat FPS
increase. Native upload/actual texture visuals andAndroid runtime validation
remain required before promoting the next candidate. Profile/build evidence
is in report/performance-push-20261102/parallel-mip-*.json.

Next multicore work: PNG/PVRTC predecode/preload with bounded memory and
owned results; assess rendering preparation/submission separation without
sharing mutable Environment or violating ordered GL/readback semantics.
Actual native game execution still runs on the emulator owner thread.
