# PC weapon display and input order

2026-11-06 user request, PC only: top to bottom, 1 Plasma Cutter, 2 Ripper,
3 Line Gun, 4 Rock Corer, 5 Burst Rifle. Display names are 等离子切割器、切割锯、
线性枪、岩石取芯器、大脉冲. Keyboard semantic slots use native indices 0/1/2/3/4.
DPad Up/Right/Down/Left uses slots 1/2/3/4; a 450 ms hold selects slot 5 once.
The existing edge/cooldown, modal ownership, native callbacks and user bindings
remain authoritative. No weapon availability, ammo, save, or player state is written.

The original native RIG is an L-shaped wheel. Windows now mounts a read-only,
length-preserving geometry view of the exact original `published/layoutdata/layouts.xml.sb`
(SHA256 `11a7e8d5f82071a0132baf0675fffcf113d0711c52fb0a45aa8eeddef9a0b0b9`).
Story and Survival weapon sublayouts get five vertical parent rectangles. Their
icons, button identities, anchors and native hit testing are retained. All SBIN
sections/checksums, object IDs and vertex types are validated before patching.
Only the 10 weapon rectangles, two RIG-toggle x positions (to avoid the first
weapon), and DATA checksum change; unknown resources remain intact.
One process-wide immutable buffer backs the VFS file. No modified game resource
is checked into the repository, written to GameData or repackaged into an IPA.
Android has no mount call and its tree is unchanged.

Launcher key labels, controller instructions, tutorial text and save-editor
display names follow this order. The tutorial still reads rebound semantic keys.
Fresh/reset PC settings enable native HUD; an explicit saved opt-out is preserved.

Component regression: compile `tools/tests/PcWeaponOrderTest.rs` with `rustc --test`
and point `PC_WEAPON_LAYOUT_FIXTURE` at a private read-only extraction of the
original layout member. This covers native command IDs, indices, DPad hold,
rebound tutorial text, Story/Survival rectangles, unrelated section preservation,
checksum/truncation rejection. Actual hidden native tests and screenshots belong
in the version's report; components alone do not prove game selection or rendering.
