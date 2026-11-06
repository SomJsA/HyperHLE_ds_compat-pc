# F kinesis and E store/workbench follow-up

> Current: this correction is installed as part of combined build 1d0a9e3b.
> **2026-10-25 USER ACCEPTED:** “EF也修好了”. E/F correction passed the user playtest.
> User also accepted the ending face light-strip fix. No new per-scenario checklist or recording was supplied.
> See [TAKEOVER.md](TAKEOVER.md); staging/PID statements below are historical.

## Reproduction and evidence

The installed aea05e3d candidate failed gameplay acceptance: repeated F logged
`no enabled visible telekinetic target`, although the kinesis icon was visible.
The user also reported E worked on doors but not stores/workbenches. A direct
mouse click on the same box's kinesis icon DID pick it up (user confirmed).

F5 captured `state-1792864842-f006506.dsstate`, 56,939,492 bytes, from PID 63752.
Evidence is retained outside Git in the outer project's
`logs/kinesis-visible-target-20261025/`. No restore was attempted.

The snapshot identifies:

| Owner | Object / region | Shape | Geometry / enabled |
| --- | --- | --- | --- |
| StompBox | 0x37466000 / 0x36afbf30 | Circle, vptr 0xa62ca8 | center (293,204), radius 30; enabled |
| Door | 0x36afc880 / 0x36b13730 | Box, vptr 0xa63278 | (295,105,48,48); leaf enabled, owner disabled |
| Store | 0x36b1e1a0 / 0x36b1ef80 | Circle | center (50,162), radius 30; leaf disabled at this position |
| Bench | 0x36b1f5f0 / 0x36b21110 | Circle | center (407,226), radius 30; leaf disabled at this position |

## Two source defects

1. The region reader assumed every leaf was a 28-byte rectangle. Circles have
   only 24 bytes: x/y are CENTER coordinates and +0x14 is RADIUS. Reading
   +0x18 consumed unrelated allocator data (`-33686019` in this capture),
   treated as height, so the target was rejected. Native Circle IsPointWithin
   is ARM 0xd6f1c; SetCenter/GetRadius confirm this layout.
2. The extra CanInteract call passed the PLAYER transform. Native
   GameObjectInteractive::OnUpdatePostCamera (0xd5e60) builds each TARGET
   interaction-node transform, applies the kinesis offset at 0xd6008–0xd6074,
   then calls virtual CanInteract at 0xd608c. It enables/projects the region
   only after acceptance. Rechecking with an unrelated transform is invalid.

## Correction and validation

- Decode circle/box separately and share decoding across dispatch, live-region
  diagnostics and the legacy tap diagnostic. Unknown/hidden geometry is rejected.
- Use the game's forwarder/list/leaf enabled state plus on-screen geometry.
  Also reject disabled owners that retain stale leaf geometry during HUD fades.
- Remove the incorrect duplicate CanInteract check and the unsafe hidden-target
  fallback entirely, including their player-transform allocation helper.
- Preserve F's native CanTelekinesis and IsTelekinesisable checks and target
  SendAction path. Preserve the user-confirmed zero-g parameters.
- Four Rust regression tests pass, including a six-word circle fixture that
  fails immediately if the decoder reads outside the actual circle allocation.

Release build passed in 4m58s. Candidate is staged outside Git at
`builds/circle-interaction-20261025/touchHLE.exe`, SHA256
`bffb6d57b018c7e7c70fcddf97e032a99b897ab4815420208af18076a42e9eef`.
At that time it was NOT installed: the user had progressed PID 63752 to a boss phase near
the next rendering bug and paused. Preserve this foreground game. OBS was
verified actively recording scene dsm at that time. Runtime then remained aea05e3d.
The user later quit that process, and the combined 1d0a9e3b build was installed.

An additional native Documents backup at
`savebank/20261025-020809-paused-near-boss-bug-20261025` still contains only old
chapter 1 checkpoints, not this boss phase. A fresh F5 diagnostic snapshot
`state-1792865354-f036252.dsstate` is archived in `logs/boss-paused-20261025/`;
it is NOT a verified restorable save. No F9 was attempted.

Mouse-click success confirms the original game path, NOT this new F build.
Original acceptance checklist (user has since confirmed E/F fixed overall):
F grabs the box; E opens store/workbench when their
prompts are active; E still opens doors; empty-space E cannot teleport.
