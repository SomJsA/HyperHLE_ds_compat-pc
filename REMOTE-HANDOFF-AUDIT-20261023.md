# Remote handoff audit (2026-10-23)

## Artifact integrity

`E:/BaiduSyncdisk/DeadSpace/DSMB安卓-REMOTE-HANDOFF-2026-10-22.zip`

- Size: 3,441,547,700 bytes
- Expected and actual SHA-256:
  `516457401b9fd057fe93abbf42cebef907bd9a1dcf33f8eb2d7f4afbb9ab0eb1`
- ZIP validation: valid, 94,668 entries, 4,447,755,211 uncompressed bytes.
- This is the source handoff that should be inspected.

`E:/BaiduSyncdisk/DeadSpace/DSMB安卓-FULL-2026-10-22.zip`

- Expected SHA-256: `30f1d706586a4ca9e1a7fab63a0d5112e0f3ce60504fb53b85fbe869b8b78f89`
- Actual SHA-256: `23c6c49c0685212d1b952550b5c43d390ba00c98a1266afcb6199d1d0593f0e9`
- Result: corrupt/incomplete historical archive; do not use it for recovery.
- The file starts with a valid ZIP local-file header but has no ZIP end-of-central-
  directory record in its final 64 KiB.
- The last recognizable local header describes a partially written game asset,
  `Payload/Dead Space.app/published/textures/texture_env_generic_objects_02.m3g`.

## Recovered Git state

Git state read from the valid remote-handoff ZIP:

`E:/CodexProjects/DeadSpaceLegacy/vm/remote-zip-git-20261022-v3/`

- HEAD: `cadd485e13ee39f819afc26aeeb423c2e2bdb00b`
- Commit message: `baseline: Dead Space Android snapshot 9a4369d9`
- Commit time: 2026-10-20 20:21:11 +0800
- Object count: 9,306
- Commit count: 1
- Reflog contains only creation of that initial commit.
- No unreachable/dangling commits are present, so no later remote commits can be
  recovered from this archive.

The committed tree is effectively the 2026-10-20 `9a4369d9` baseline.
The handoff documentation explicitly says the worktree is dirty on purpose.
Comparing its tracked source to the baseline shows only one dirty source file:

- `src/window.rs`: 90 additions, 16 deletions.
- Adds present-frame timing for average/1%-low/minimum FPS.
- Moves the Android battery query onto the SDL parent stack.

Those two changes already exist in the current local branch. The rest of the
remote worktree source matches the baseline and does not contain the later
renderer work.

## Missing render work

The remote-handoff worktree lacks all of these later controls while the current
local branch contains them:

- `TOUCHHLE_DS_NO_FOG_HASH`
- `TOUCHHLE_DS_NO_FOG_SCREEN`
- `TOUCHHLE_DS_TEXFILTER_LINEAR_HASH`
- `TOUCHHLE_DS_AUTO_OPENING_MIPS`

`TOUCHHLE_DS_NO_FOG_ORTHO` is present, so the remote is not a pre-render-fix
snapshot; it is specifically missing the subsequent opening, distance-fog,
porthole and poster work.

Local branch comparison:

- `src/gles/gles1_on_gl2.rs`: 720 additions, 49 deletions vs remote baseline
- `src/frameworks/opengles/gles_guest.rs`: 471 additions, 33 deletions
- `src/window.rs`: 206 additions, 23 deletions

The current verified branch is `codex/android-mainline-merge` at `fc2c7eb8`.

## Synchronized PC deliverables

`E:/BaiduSyncdisk/DeadSpace/Current/PC` still ends at the pre-fix
`DeadSpace-PC-OneClick-20261022-0340.zip`. It does not contain the verified
Chapter 1 poster fix or the updated fixed runtime.

## Recovery recommendation

Do not use the truncated FULL ZIP. The valid remote-handoff package can be used
for its dirty `window.rs` work, but that work is already present locally.
Continue from the intact local branch `codex/android-mainline-merge` at or after
`fc2c7eb8`; the remote handoff cannot recover the later renderer work because it
was never committed there.
