# Startup registration

## Windows R6 startup policy

The Windows x64 runtime bypasses the one-time registration question for the
six known ARMv7 executables. The matching 1.3.34 marker and direct IPA share
one executable fingerprint. After loading the guest executable and before
starting the CPU, the runtime verifies the executable fingerprint and the
three native menu calls to the read-only registration notification getter.
It replaces each call with a zero result so the original menu takes its
no-question branch. This also covers the menu paths that would ask again
after leaving a profile. IPA files and the registration setting on disk are
outside this path.

With fast present enabled, the two iPad 1.3.11 builds need the native touch
orientation on the registration question if it is reached. The normal menu
and the registration form use their existing present transform.

The R6 chapter test copy was checked with a saved profile on all seven
resource entries. It opened the main menu, resumed the save, accepted native
movement, and closed normally. An empty iPad 1.3.11 profile still opened the
native difficulty screen. The old runtime reproduced the question on all
seven resource entries. The paired iPhone 1.3.8 test used an isolated copy
of an existing checkpoint with the registration notification re-enabled.
The candidate reached the main menu without writing that flag, then
resumed the checkpoint and accepted native movement.

## Historical popup input regression

2026-11-04. Implementation `7c914700`, normal release SHA256
`0eb0ca1a7ac0c8bf9f263bda24237396f422703f1a1edcdaa984b42575f332d5`.

The user reports the one-time registration question after obtaining the saw /
first checkpoint and fully exiting/restarting. A private copy of their current
Documents reproduces frontend page 2. A fresh empty profile instead enters the
difficulty picker and the introduction; it is not the same reproduction.

`LayerMainMenu::OnDraw` renders its embedded Popup at `owner+0x1f0` through
`WidgetHandler::OnDraw(Renderer)` at `0x002c19f4`. The prior input observer only
hooked `OnDraw(SpriteGraphics)` at `0x002c1174`, so it never observed this popup.
Read-only inspection of an owned hidden test process confirms its expected
Popup vtable `0x00a89128`, mode 2, and active native BTN_OK/BTN_NO widgets.
The old keyboard cancel was acknowledged but did not close the popup.

Observe both verified original ARM entry instructions (`0xe92d40f0`). Existing
modal detection, animation readiness, native bounds and original button methods
remain authoritative. No pixel scanning, guessed coordinates, registration flags
or player progress writes are added. The renderer observer follows the existing
Windows-only input-hook installation gate; Android input is unchanged.

Normal release native regressions with fresh isolated copies:

- Keyboard cancel: exactly one native BTN_NO, page 2 to 5.
- Keyboard confirm: exactly one native BTN_OK, page 2 to 22 (registration form).
- Virtual SDL controller B/A: same cancel/confirm transitions, in both explicit
  keyboard mode and auto mode; auto mode also displays gamepad action hints.
- Held E for one second then release: one native BTN_OK, page 22 retained.
- Restart after cancellation: main menu page 5, question does not reappear.
- All accepted runs exit normally; the original user's 12 save/dependency files
  remain byte-identical. No physical controller/manual acceptance is claimed.

The first virtual-controller attempt attached the controller and pressed its
button in the same command. SDL opened its controller mapping with the button
already down, so no down event was emitted. Those two ineffective samples are
retained, not passed. Prime attach with an UP, wait for opening, then test DOWN/UP.

Evidence: project report `session整理-20261104/STATUS.json`, registration result
JSON files, build manifest, source guards and visual contact images. Existing
delivered v8 was not overwritten; this runtime is a newly built/tested binary,
not an updated published package. Fonts and ReadPixels/CPU work remain pending.
