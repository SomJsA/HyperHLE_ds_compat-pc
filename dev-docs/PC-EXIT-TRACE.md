# Opt-in native shutdown observations

Requested font integration exposed intermittent window-close waits. Successful
samples exited in1.891/5.078/15.031s; other private runs exceeded12/45s. These
samples do not establish a cause or a fix. Keep them as a separate R15 gate.

`TOUCHHLE_DS_LIFECYCLE_TRACE=1` on a Windows Dead Space run logs boundaries around
user-default synchronization, native resign/terminate callbacks, notification
delivery, pool draining and diagnostics completion. While waiting, at most four
snapshots list up to16 guest thread PC/LR/SP, blocked reason and mutex owner.
Snapshots occur only at cooperative host boundaries; a host driver that never
returns may prevent them. No raw input, guest string contents or automatic
screenshot is collected. A trace run cannot certify performance.

The observer belongs to the current environment. It leaves callbacks, preferences,
saves, scheduling decisions and termination unchanged, with no guest calls from
the observer. It does not force an exit, skip a callback or release a lock. The
normal run does not enable periodic polling. Diagnostics receive no new trace
event after admission is closed. Existing v8/v9 packages remain immutable;
new experiments require their own normal binary/source/map provenance.

The four on/off font-role samples in
`report/pc-native-shutdown-20261105/exit-cases-01/results.json` all reached their
expected native popup destination, then exceeded45s at `resign.delegate.begin`.
Thread0 waited for mutex6 owned by the still-rendering engine thread3. Static
code shows `OnDeactivateThreadUnsafe` takes the application render lock and
`OnRunLoopTick` releases that lock after rendering. These observations alone
cannot distinguish an unbalanced recursive lock from cooperative starvation.

The extended observer reports recursive depth/waiters and the first eight final
contended unlocks during close. The separate handoff
lets the existing round-robin scheduler run
after a fully released, contended mutex. It is enabled only when closing a
Windows Dead Space environment; it does not change gameplay scheduling,
recursive decrements, callback order or scheduler relock ownership.

The same-binary ABBA in `exit-cases-02/results.json` confirmed final releases at
native LR0x530a14/0x5307a0 with depth1/waiting1, followed by immediate renderer
reacquisition before the waiter could run. Both disabled closes exceeded45s;
enabled cancel/confirm closed in0.656/0.688s, reaching every original lifecycle
boundary. The subsequent native chapter/CPU↔GPU presentation exercise also
closed normally in1.063s with its original15-file source unchanged. These are
native close-path evidence, not gameplay performance or long-session acceptance.

The Windows Dead Space close path now enables handoff by default;
`TOUCHHLE_DS_EXIT_MUTEX_HANDOFF=0` retains the old behavior for comparison.
Coroutine tests exercise the real unlock and round-robin relock: only the final
recursive decrement yields, the waiter becomes owner exactly once, disabled
immediate reacquisition remains possible, and errors/uncontended release do not
yield. No new API, unsafe scheduling primitive or forced lock ownership is used.

## Finite quiet-runner cleanup

`tools/pc-runtime/Start-QuietPerfRun.py` now requests native window close for its
owned child after the measurement windows. It matches each window's PID while
retaining the Popen handle and posts WM_CLOSE without foregrounding a window or
injecting host input. The hidden private child explicitly sets
`TOUCHHLE_DS_CONFIRM_QUIT=0` so a host confirmation dialog cannot hold automation;
guest resign/terminate callbacks, notifications, saves and scheduler handoff
remain intact. Product startup and confirmation defaults are unchanged.

Cleanup waits at most45s for native exit. A timeout, missing owned window or
nonzero exit is not normal-close success. Bounded terminate/kill is only failure
cleanup for that owned child and remains recorded. `report-status.json` separates
`timing_exit_code` from the overall `exit_code`; valid timing plus failed cleanup
returns2. Historical manifests without cleanup evidence remain timing-only.

This corrects a measurement-controller limitation, not a new guest mutex fix.
The clean7da on/off timing evidence remains in
`report/pc-readback-finish-resume-20261105`: its first runner forced exit, and its
first WM_CLOSE adapter waited at the host confirmation dialog. A separate
same-binary private run with confirmation disabled reached every original
lifecycle boundary and exited0 in1.063s. Keep those outcomes distinct.
