# Requested font roles

User request, 2026-11-04: title/frontend/chapter Chinese uses MStiffHei;
story subtitles and native in-game UI (including pause) use Swei Shear Sans
CJK SC Regular. The exact supplied files are embedded, without changing the IPA.

| Role resource | User source SHA256 |
| --- | --- |
| `res/overlay/TitleChinese.ttf` | `c14c692b526830a48bfa7d946afb3beb350f078abad3cd67d245877fed52b751` |
| `res/overlay/StoryChinese.ttf` | `df140c929d10307ca270de1161134d20bf1cc3b12aaee7b6df50ebfb4b8ceb2d` |

Sources are the user's `mstiffhei-title.ttf` and
`SweiShearSansCJKsc-Regular-story.ttf` in the specified font-spacing-candidate4
directory. Both cover every Chinese character in the current 996 text entries.
These hashes are authorized new resources, not a resampling of a failed baseline.
The PC source guard pins them, and package validation checks their actual bytes
are embedded in the executable. Existing chapter resources remain for Android
and historical tools; PC chapter presentation directly uses the title resource
for Chinese, English and chapter numbers.

UIKit CJK measurement, wrapping and drawing share the same cached role font.
Frontend/bootstrap layouts use Title; HUD/gameplay/pause layouts use Story, and
returning to a frontend page selects Title again. Guest font glyph IDs and its
Latin font provider remain unchanged. This avoids the historical mismatched
glyph-ID substitution which corrupted profile numbers and clocks.

Host chapter rendering uses MStiffHei. Subtitle and body cards use Story;
objective labels and tutorial headings use Title. Existing sizes/styles are
preserved, with layout measured using the actual selected font. No new pixel
readback or framebuffer scan is used for font selection. Native role logging is
explicit (`TOUCHHLE_DS_FONT_ROLE_TRACE=1`) and capped. `TOUCHHLE_DS_FONT_ROLES=0`
disables native UIKit role selection for a fresh diagnostic process.

Component regressions cover all chapter titles, glyphs/numbers/highlight,
all 85 objective bodies, caption wrapping/timing and bootstrap/gameplay/pause/
return-to-menu roles. Scoped native menu, chapter1, subtitle, repeat-pause and
return-to-menu captures now pass on the new normal release. These opening
observations do not replace all-chapter or final human acceptance.
See project report `pc-font-roles-20261104/STATUS.json` for current evidence.

This is the PC implementation stage. Android's frontend/input/overlay baseline
is different; do not copy the PC controller or full platform files into it.
Android role integration, complete build and native acceptance remain open.

## Native glyph-cache role keys

The first native candidate exposed cached frontend glyphs in pause UI even when
UIKit later selected the Story face. The native GlyphBuffer consumers key the
glyph map by IFont::GetID alone. Merely changing the UIKit fallback cannot update
that existing map. Keep the failed/partial native evidence in the report.

The correction verifies the complete 8-byte FontCGFont::GetID and FontUIFont::GetID entries, the three
GlyphBuffer return sites and all seven instructions of IFont::CreateID before
arming. Each environment owns at most 512 (original font ID, role) mappings.
New cache IDs reserve the next value from the verified native font-ID counter at
0x00aeed94, so later native fonts cannot reuse them. This is one font-metadata
counter write per new role mapping; no font object's original ID, glyph cmap,
input, progress or IPA is rewritten. Do not replace this allocator with guessed
high-bit tags that could collide with native font IDs.

Only the known GlyphBuffer callers receive the role key. Other getter callers
retain their original value. Unsupported code withholds native role fonts;
capacity/domain failures retain the original cache key and are reported, so a
run with such fallback cannot certify font-role completeness. Entry dispatch
replays LDR r0,[r0,#8/#0x10];BX LR without guest calls at the breakpoint. Original
ARM/Thumb returns and GPR/CPSR/FPSCR/stack equivalence have production-JIT tests.
Native visual/cache-transition acceptance remains separate from these tests.

The UIFont-only cache attempt did not reserve any IDs in the native game run.
FontManager actually builds FontCGFont from the guest TTF; its Chinese fallback
still calls UIKit. The CGFont constructor's verified allocator call and +8 ID
store are checked as well. Both classes now participate. Existing CGFont bytes,
glyph-number translation and Latin provider remain intact; only GlyphBuffer's
font-identity keys are separated. Explicit native probes report class hit counts,
reserved keys and fallback counts to logs/diagnostics; process exit may bypass
Rust Drop, so a missing Drop summary is not zero-call evidence.

## Pause modal scene ownership

Native-04 hit 13,799 CGFont GetID calls with five reserved role keys and zero fallback, but the pause face remained MStiffHei. The pause input owner intentionally makes ds_current_hud_player unavailable, which publishes HUD UNKNOWN. Font role retains the last verified gameplay scene across this absence; a real frontend state change resets it. It is independent of transient input availability. The regression explicitly covers gameplay -> pause -> UNKNOWN -> pause -> frontend -> UNKNOWN. The verified third cache consumer, PrepareCharactersInString (return 0x56b1fc), is also included.

## Scoped normal-release observations

Implementation5b2f77ed, normal EXE SHA256 c789a70477c2dcf093667a7b0e4125b8b69d93f286e4f1996930da4e344d09ca.
Native06/07/09 show MStiffHei frontend and chapter1 Chinese/English/numbers/red
acrostic, Swei story subtitles and native pause. Repeat pause/resume retains the
same face. Quitting this chapter returns to chapter menu12, then native pages
9 -> 6 -> 5 restore the frontend face. Navigation waits for actual transformed
widget bounds; a state-transition log alone is not animation readiness.
Final viewport captures are1620x1080, reduced to1350x900 with no extra cropping.
The GPU text/copy opt-in also passes the scoped visual sequence; it remains
opt-in, since capped capture/probe runs cannot certify performance gains.

Native09 default CPU path exited normally in1.891s; native07 GPU path5.078s;
the actual packaged direct-native startup smoke exited in15.031s. Earlier
12s/45s close waits failed and remain an open shutdown stress/root-cause gate.
Do not attribute them to font selection or claim they are fixed from later
successful exits. The local v9 candidate passes24 scoped source/artifact
checks, including exact embedded role fonts and unchanged accepted original IPA.
Original saves, supplied fonts and delivered v8 remain unchanged. Android,
performance, shutdown stress and full R15 acceptance remain open.
