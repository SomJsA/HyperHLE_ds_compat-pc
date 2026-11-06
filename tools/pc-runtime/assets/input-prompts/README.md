# Native-control input prompts

Selected sprites from [Kenney Input Prompts](https://kenney.nl/assets/input-prompts), CC0. The original license is in `License-Kenney.txt`; `atlas.json` records the source filenames and downloaded archive SHA-256. The Chinese label uses the project's bundled Noto Sans SC font.

`atlas.png` is a 512×256 RGBA atlas. `Build-InputPromptAtlas.py` rebuilds it from the upstream archive and bundled font. Only the selected sprites are shipped.

The renderer loads `assets/input-prompts/atlas.png` beside the executable, with an embedded copy as fallback. `TOUCHHLE_DS_INPUT_ATLAS` may select another atlas using the same dimensions and sprite positions. Assets are uploaded once per GL context and batched into one draw; replacing the PNG takes effect on the next game launch.

Keyboard E / Enter and gamepad A confirm. Keyboard Q and gamepad X cancel. Direction arrows, D-pad and “选择” identify navigation beside native selection controls. Original game button artwork remains visible.
