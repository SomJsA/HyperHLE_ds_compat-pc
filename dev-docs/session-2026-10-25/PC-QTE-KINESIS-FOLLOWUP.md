# PC QTE / kinesis / opening blackout follow-up

User report (2026-10-25): E misses the swarm tap QTE and the second horizontal
grapple swipe; F misses the objects in two supplied magnet-prompt screenshots.
The earlier general E/F acceptance covers doors, stores, benches and a box,
not every QTE or telekinetic object. Window title requested: `DeadSpace IOS移植`.
User explicitly requests NO computer-use; any interactive reproduction is done
by the user, with F5 diagnostic snapshots or other explicit markers.

## Verified native findings

- `GameObjectPlayable::OnEvent`, `0x54a50..0x54a80`: main state `+0x208 == 9`
  routes to normal grapple; state `10` routes to `OnGrappleSwarmEvent` at
  `0x3fc60`. The old E implementation never handled the latter.
- `OnGrappleGestureEvent`, `0x40cd0..0x40d00`: positions at event offsets
  `0xc,0x10,0x14,0x18` are signed INTEGER words. The handler subtracts them
  before converting to float. The old injected event incorrectly used f32.
- The import stub at `0x8bcbe0` resolves through Mach-O indirect symbols to
  `_atan2f`. Its arguments are **dx, dy**, in that order. The previous note
  claiming an upward swipe yields the accepted -90 degrees was incorrect.
  Leftward produces -90; upward produces 180. Native guards remain unchanged:
  grapple state 1 accepts 12..78 degrees; state 7 accepts -123..-57 degrees.
- `InputForwarderGestures::SendGestureEvent`, `0xe191c..0xe1960`, confirms
  the event header: GestureEvent vptr `0xa62e58`, type 1007, flags 15.
- Swarm handler `0x3fc60`: type 1001 PointerSingleTapEvent with RTTI; integer
  x/y at +8/+12. It visits eight smart-pointer entries at player +0x3f4,
  checks virtual +0x10c and object flags +0x40, obtains position through
  virtual +0xc0 and projects with camera method `0x78da4`. A successful tap
  detaches one matching creature. The candidate mirrors candidate projection
  and passes a typed tap event to this native handler, preserving its hit test.

## Candidate changes

- Use integer event words and leftward second swipe; gate grapple handling
  with the main player state so stale substate fields cannot trigger it.
- Free temporary event/projection allocations after synchronous guest calls.
- Route swarm E to one projected, eligible attached target and native tap.
- Retry F after refreshing action-forwarder cache on a miss, just as E already
  does. Keep native CanTelekinesis, IsTelekinesisable and visible-region checks.
  Runtime evidence now confirms this for a newly severed enemy limb (below).
  The second screenshot's other object still needs independent acceptance.
- Rename the Dead Space runtime window to `DeadSpace IOS移植`.
- Log grapple state before/after swipe and swarm tap acceptance.

## Validation and pending work

Three standalone Rust regression tests passed: both vectors satisfy the native
angle guards; the old upward f32-word vector fails state 7; non-swipe states
produce no gesture. Compilation/deployment status is recorded separately after
the build finishes. These tests do not replace in-game QTE acceptance.

The original opening partial-blackout contact sheet was reviewed:
`logs/frame2016-final-20261025/review-partial-black-3508-3556.jpg`.
At video frames 3524..3540, walls/floor vanish while player, lamps, poster and
HUD remain. No renderer change is justified yet. Whole-frame black detection
cannot validate this defect. Use a short scene-specific capture and per-draw
pixel trace; preserve the existing accepted depth/fog/rock repairs.

Requested first capture: with the current installed version, display one of
the reported magnet prompts, press F once, then F5 and pause; report which
scenario. F5 is diagnostic only, and F9 restoration remains unsupported.

## Confirmed F root cause: dynamic enemy drop

User clarified the tested target is created when severing an enemy, rather than
present at scene load. Snapshot `state-1792926419-f006749.dsstate`:

- player `0x36d8ea30`, main state 0, kinesis unlock byte +0x5b0 = 1;
- GameObjectLimb `0x302b62a0`, IsTelekinesisable byte +0x22f = 1;
- forwarder `0x30cbb3e0`, code 0x10, forwarder/owner/list/leaf all enabled;
- circle `0x310cedc0`, center (272,185), radius 30.

Repeated F logged `no enabled visible telekinetic target`. User then pressed E
on the SAME target and confirmed it could be grabbed. Runtime log explicitly
shows E refreshed the cache and selected exactly `0x30cbb3e0 / 0x302b62a0`,
rect (242,155,60,60). This demonstrates the stale-cache failure without
changing position, loading saves, or bypassing the engine's eligibility checks.
The new F code uses the same refresh/retry behavior with its telekinetic filter.

Evidence: `logs/pc-qte-20261025/first-f-test.{txt,json}` and F5 copy; later
`e-refresh-control.txt` records the successful E control experiment.

## Installed candidate

Release build passed (4m56s), three gesture tests and four region tests passed.
Isolated headless startup passed; existing complete helmet/glow cleanup patch
still matches. User exited the baseline process, then candidate was installed
and launcher reopened. Live PID 64208 reports title `DeadSpace IOS移植`.

- Candidate: `builds/pc-qte-kinesis-20261025/touchHLE.exe`
- SHA-256: `dba3e40eb93daa9eb4ceaa4749fbb532c15da8ae6d8c407de87cd29126caa9ff`
- Prior executable retained at `logs/pc-qte-20261025/previous-touchHLE.exe`.
- Build logs: `logs/pc-qte-20261025-build-final.txt`.
- Runtime gameplay acceptance pending; opening partial blackout remains open.

## User acceptance and revised E requirement

User subsequently confirmed F now responds correctly, the large-enemy grapple
QTE succeeds, and another smaller-enemy QTE succeeds. Runtime evidence includes
state 1->3 and, after the native animation gate permits it, state 7->9. See
`logs/pc-qte-20261025/accepted-f-and-grapple.txt`.
User also explicitly accepted the full-body swarm/tap in original screenshot 1 and F on dynamically dropped monster items. All original QTE/F cases are accepted; evidence: `logs/pc-qte-20261025/accepted-all-qte-f.txt`.

User explicitly changed E policy: no telekinesis; prioritize QTE, then ordinary
inventory pickups, then scene objects such as benches/doors. If inventory is
full and pickup fails, temporarily skip that pickup so it cannot obstruct a
bench. F retains telekinesis.

Implemented (new candidate awaiting deployment): filter native IsTelekinesisable objects out of E,
rank `GameObjectPickup` (vptr 0xa7e158) before scene objects, refresh before
ranking new drops, and skip an unchanged failed pickup for 3 wall-clock seconds.
`GameObjectPickup::OnOpen` at 0x138b18 calls Pickup on its Item (+0x1d8),
checks the remaining quantity at +0x1e0, and enters pickup state 2 when fully
consumed. A failed attempt leaves quantity/state unchanged; its error branch
starts at 0x138cf0. Successful partial pickups must NOT be suppressed.
Four policy tests passed, including partial pickup, per-item expiry, E/F role
separation and pickup-vs-bench priority. Revised behavior still needs a new
compiled deployment and full-bag scene acceptance.

Opening capture support is being prepared separately: optional
`TOUCHHLE_DS_PIXEL_TRACE_WAIT_FOR_MARK=1` waits for F7/M before the one-shot
dark-pixel trigger. This is diagnostic only; no rendering repair is claimed.

## Revised candidate validation

Release build completed successfully (4m40s); isolated headless startup smoke passed.
Candidate: builds/pc-e-priority-20261025/touchHLE.exe
SHA-256: 8d68e2cf4169f3d8878a6654291bee923b501c591d21c09fd141b16a0cb85bfa
This candidate includes E policy and opt-in opening trace support. Current foreground game remains on the previously accepted QTE/F build. Full-bag interaction and opening blackout still require gameplay evidence.
