# Android stale readback regression

On return from five-minute timed survival, an old level selector could appear
before the black transition and normal return animation. Direct EAGL presents
left a CPU readback from the earlier menu marked as usable by Core Animation.

A direct present now retires that CPU image. The allocation and drawable
dimensions remain available for reuse; a new readback makes the image current
again. CA uploads and draws only current readbacks.

Validation: the cache lifecycle regression test passes, including allocation
reuse and dimensions after invalidation. Android arm64 release built and its
isolated installation matched SHA256
`7F2C0491F62E747197F9501E3EE6B41CC13B5F971F1DC76C72505B498FA0245F`.
The user accepted the timed-survival exit regression on 2026-10-30. That APK
was built from the wider working tree; this commit contains only the accepted
readback-cache fix and its test, without the pending PC work.

Rollback diagnostic: `TOUCHHLE_DS_CA_STALE_FRAME_GUARD=0`.
