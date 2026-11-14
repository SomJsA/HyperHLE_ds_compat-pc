# PC multilingual HD text overlay

The launcher now treats every language bank present in an adapted IPA as an
HD overlay choice. It extracts the selected `published/stringdata/<lang>/`
SBIN and, when needed, the same IPA's `ENG_US` SBIN into the private data
directory. The original IPA is never rewritten. The host overlay maps its
stable IDs against those banks, uses the selected language first, and falls
back to English for empty/placeholder records such as the incomplete Japanese
bank.

The overlay font is selected from the same language ID. Chinese uses the
existing OFL Swei role face for the native Chinese presentation; Japanese uses
Noto Sans CJK JP, Korean uses Noto Sans CJK KR, and French/German/Italian/
Spanish/English use Liberation Sans. Noto and Liberation files are distributed
with the existing SIL Open Font License notices in `touchHLE_fonts/LICENSE.noto`
and `touchHLE_fonts/LICENSE.liberation`. The Korean Noto file source is the
official `notofonts/noto-cjk` Korean OTF tree.

Chapter headers and objective labels are localized by the selected language,
while the English chapter secondary line remains available as a fallback. The
native Windows CJK role also selects JP/KR coverage so Japanese and Korean
menus do not render through the Chinese-only face.

This change is local and unlisted. It requires PC runtime and multi-language
opening/HUD regression before any release decision. Android still uses its
separate launcher path and needs an independent device acceptance pass.
