# Input compatibility backlog (2026-10-21)

Reverse engineering and engine-level investigation are authorized.

## Reported on current build

| Action | Result |
| --- | --- |
| `E` on door interaction ring | Unreliable |
| `E` on shop / workbench / console | Unreliable |
| `F` kinesis | No response |
| `E` melee | Works |
| QTE tap / swipe | Not retested |

## Next investigation

1. Capture the guest event sequence for successful melee versus failed door
   interaction at the same touch coordinates.
2. Determine whether `E` is dispatching a generic tap, a synthetic swipe, or a
   stale held touch.
3. Trace `F` from the host key mapping through guest event delivery into the
   kinesis input state.
4. Move supported interactions to the same engine-level action path instead of
   relying on screen-coordinate tap heuristics.
5. Re-test pure taps, directional swipes, repeated taps, and hold gestures
   with the interaction ring at near, edge, and outside distances.

## Decompile status

- All blue-ring scene interactions use action code `0x10`, including doors,
  stores, benches, pickups, and stomp boxes.
- `InputForwarderTapToAction::SendAction` is `0x000e55cc`; its event eventually
  reaches the owner object's `OnInteract` vtable slot at `+0x170`.
- The current `E` implementation already calls the object forwarder through
  `SendAction`; failures therefore point at forwarder discovery, region
  liveness, or the event hop rather than SDL key mapping.
- Kinesis is a player action (`0x06`), while `F` currently attempts only code
  `0x06`. A fallback path should distinguish "no enabled 0x06 forwarder" from a
  valid kinesis action that the engine rejected.
- Melee (`0x0e`), stomp (`0x0d`), reload (`0x0a`), stasis (`0x0b`), and cutter
  rotation (`0x07`) already have engine-level mappings.
- The 2026-10-22 opening trace confirms the docking-bay draw itself is already
  `GL_LEQUAL`; the residual visual issue is overlapping scene ownership, not a
  missing global depth-function switch. UV probes at 15 seconds resolve to
  texture content key `25a08a82`.

## VM door-save probe (2026-10-22)

- Restored the recorded chapter-one door profile and replayed the normal menu
  path; the before-action screenshot is the closed blue door.
- `E` produced:
  `interact via engine: code=0x10 fwd=0x30d926a0 obj=0x30d918c0
  rect=(167,124,49,49) (no screen tap)`.
  The after-E screenshot shows the door open and the player standing in it.
- `F` selected the player-owned code `0x06` forwarder and reported
  `aiming=0 -> 1` on both test presses. The after-F screenshot shows a changed
  player/weapon pose, proving the action reached `GameObjectPlayable`.
- Therefore SDL key receipt, forwarder discovery and the direct-engine call are
  functional at this location. Remaining failures need to be attributed
  position-by-position: missing/late live regions, a mod-specific state guard,
  or a prompt stage that expects a second action.
- Evidence bundle:
  `F:\CodexProjects\DeadSpaceLegacy-package-staging\input-probe-20261022`.

## Default engine fallback (2026-10-22)

**Further correction, 2026-10-25 02:00:** live leaves include 24-byte circles
as well as 28-byte boxes. The rectangular reader rejected kinesis, store and
bench circles. Decode by vtable. Native OnUpdatePostCamera already calls
CanInteract with each target node's transform; the extra player-transform
call was invalid and has been removed. The hidden-target fallback is now
removed entirely. See `dev-docs/session-2026-10-25/CIRCLE-INTERACTION-FOLLOWUP.md`.

**Superseded 2026-10-25:** the final-boss replay selected a hidden/sentinel
target with distance2=69817.1328 and teleported the player into a stuck
interaction. The fallback is now OFF unless explicitly enabled with
`TOUCHHLE_DS_INTERACT_CAN_FALLBACK=1`. Virtual CanInteract rejection must
not be overridden by calling the base-class implementation. F code 0x06
only proved aiming, not successful kinesis acquisition; use a visible
telekinetic object's own action. See the root FRAME2016 worklog.

The room replay exposed why E could keep operating the door behind the player:
the door forwarder remained enabled after the player crossed it, while the new
room object's `InputRegion` rectangle was still at its `(-120,-120,40,40)`
sentinel. Selection by screen-centre distance therefore chose the stale door.

The current source now:

1. requires each leaf `InputRegion` object's enabled byte at `+0x08`;
2. obtains the player's physical body position from
   `[GameObjectPlayable+0x4c]+0x40`;
3. calls the target object's own virtual
   `GameObjectInteractive::CanInteract(btTransform const&)` before accepting a
   live rectangle;
4. if no usable rectangle exists, scans the scene forwarders and selects the
   nearest object accepted by `CanInteract`, then calls the object's own
   `InputForwarderTapToAction::SendAction`.

VM evidence from the room replay with the defaults enabled:

```text
door: CanInteract accepted; normal forwarder path
near room object: distance2=22.6223 -> SendAction
second room object: distance2=8.8892 -> SendAction
```

The captured UI changed to an object interaction panel after the fallback, so
this is not merely a successful function call. Diagnostic opt-outs:

- `TOUCHHLE_DS_INTERACT_CAN_FILTER=0`
- `TOUCHHLE_DS_INTERACT_CAN_FALLBACK=0`
- `TOUCHHLE_DS_INTERACT_TRACE=1` for per-object details.

PC hardware and Android device validation remain required before this is
declared final.

The failed render A/B package was:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261022-0229.zip`

SHA-256:

`e45b8bf71d61f5890555c8bb54fa6452ccac3c9b5ce6020a996f5ee79e580db9`

It must not be used as the current build because user-PC testing found a
starfield black ring and more cut lines. The next package restores the old
renderer defaults and carries only the validated interaction selector.

Rollback test package:

`E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261022-0305.zip`

SHA-256:

`a225502dea0a3b5a4272b55b047897873075fbb92366af766ed63a5063a231db`

## Distance fade and mip follow-up

See `DISTANCE-RENDER-2026-10-22.md`. The GL2 `NO_FOG` presence-vs-value bug has
been fixed, and the launcher now exposes fog independently from the general
compatibility checkbox. Global PVRTC mip rebuilding is available for poster
stripe A/B testing.
