# Ending face obscured by helmet lights

> **2026-10-25 USER ACCEPTED:** “测试灯带问题已消失”. The ending face overlay
> is resolved in the installed 1d0a9e3b build according to the user playtest.
> No new acceptance recording was supplied. The user subsequently also confirmed “EF也修好了”.
> Current handoff: [TAKEOVER.md](TAKEOVER.md).

## Captured reproduction

User reached the final boss in PID 63752 and captured three F5 snapshots:

| Guest frame | Snapshot | User's timing |
| --- | --- | --- |
| 36252 | state-1792865354-f036252.dsstate | Boss phase near the rendering repro |
| 37353 | state-1792865381-f037353.dsstate | Last attack before the ending CG |
| 39261 | state-1792865422-f039261.dsstate | Face visible with white helmet-light strips over it |

All three writes completed. These are diagnostic captures, NOT verified
restorable saves. No F9 was used. The user confirmed the CG has already ended.
The user subsequently quit PID 63752 (`User requested quit, exiting.` in the
final session log). The agent did not terminate it. Do not assume the face
is still on screen or restart a later user session without authorization.

Evidence is archived outside Git at `logs/ending-face-20261025/` in the outer
DeadSpaceLegacy project, including SHA256 manifest, original user screenshot,
three snapshots, log, offline inspection scripts, and `boss-ending-obs.mp4`.
The OBS clip is 285.783 seconds; the face is visible around 90–100 seconds.
The defect persists through the close-up and pull-back, not just one frame.
Source video: `E:/DS2-Severed-Patch/artifacts/r1859_native249/2026-10-25 02-08-51.mp4`.
OBS was stopped to finalize this segment and restarted after an initial
StartRecord error; that subsequent short recording was later found stopped.
Do not claim OBS is still recording without another status check.

## Proven scene-state defect

The actual player is 0x370f04a0, with Model 0x36b30c80. Walking that model's
scene tree confirms SkinnedMesh 0x31824360 is part of the live player, not a
cached template. It owns TWO submeshes named `MESH_isaac_suit_helmet_04`:

| Submesh | Appearance | Before CG | Face snapshot |
| --- | --- | --- | --- |
| 0x319e6fb0 | isaac_suit_04 | visible=1 | visible=0 |
| 0x319e6610 | nodim_eyes_level_04 | visible=1 | **visible=1** |

The player Carrie-head flag (+0x560) changes from 0 to 1. Head replacement
ran, but the separately rendered emissive helmet layer was not hidden.
Other `nodim_eyes_level_01/02/03` submeshes are also present/enabled in this
player tree; the game's complete cleanup includes all four suit variants.

ARM `GameObjectPlayable::SwitchToCarrieHead` (0x3d948) branches on
`Application::IsTierIPad2` (0x6758):

- Phone path: build the current helmet name, call `Model::FindSubmesh(name)`
  at 0xc68c4 and `Submesh::SetVisible(false)` at 0x4422b8. Only the first
  submesh with this name is hidden, leaving the emissive duplicate.
- iPad2 path: loop the existing 12-entry table at 0xa7a3d0, with mask 0xfaa.
  Entries 1/3/5/7 are helmet bodies; 8/9/10/11 are nodim eyes. It calls
  `Model::FindSubmesh(appearance,name)` at 0xc67a8, checks for null, and hides
  each result. Body, health and stasis appearances are not selected.

This matches the user's description of armor lights covering the face. No
face-texture replacement, depth override or glow removal during gameplay is
needed for the candidate correction.

## Candidate correction

At app load, before CPU/JIT creation, replace the conditional branch at
0x3d974 (`0x0a000098`, beq phone path) with ARM `mov r0,r0` (`0xe1a00000`).
This uses the game's existing complete helmet cleanup only when switching
heads. It does not change the device tier or the original IPA on disk.

Require bundle ID `com.ea.deadspace.bv`, the exact exported function address,
and eight matching instruction words. Unknown binaries are skipped with a
log message. `TOUCHHLE_DS_CARRIE_HEAD_FIX=0` disables this candidate for A/B.
Successful startup log: `[DScarrie] verified 1.3.34: use complete helmet/glow
cleanup on head switch`.

Validation so far: three Rust tests verify the instruction guards, rejection
of unknown/Thumb symbols without touching memory, and complete cleanup mask.
Offline snapshot audits confirm this mask selects helmet/glow submeshes and
excludes the body, health and stasis submeshes. This is not a visual playtest.
Release build passed in 5m08s. An isolated headless startup applied the guarded
patch successfully; only that smoke-test child process was stopped. Three patch
tests and four circle-region tests passed. The user subsequently confirmed
the ending face light strips disappeared; visual acceptance is now passed.

After confirming the user's game had exited, the combined candidate was installed
in `run-host/Runtime-PC/touchHLE.exe` using an explicit staged executable path.
The existing `run-host/启动 Dead Space.lnk` launches this runtime.
SHA256: `1d0a9e3b3308f50546e8a9352a7830ec2866e4971991c1cc57dbb579e66b9d80`.
Build artifact: `builds/ending-head-circle-20261025/touchHLE.exe`.
See `ending-head-candidate-manifest.json` for deployment and validation status.

The candidate includes the separate circle-interaction correction for F/E.
The user subsequently confirmed that correction works too (“EF也修好了”). Native instant-save
restore and the opening partial blackout remain separate open issues.
