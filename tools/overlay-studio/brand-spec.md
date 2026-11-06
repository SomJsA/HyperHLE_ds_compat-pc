# Design decisions

Artifact: screenshot-based overlay editor, greenfield functional workbench.
Audience: project owner iterating chapter / objective / subtitle presentation at desktop distance.
Visual language: game-adjacent instrumentation, quiet graphite UI with restrained cyan.
Dials: visual variance 3, motion 1, density 8, asset dependence 10, brand fidelity 7.
Palette: #101315 background, #191e22 panels, #2d353b strokes, #dce5e8 primary text, #849198 muted, #66dae2 active.
Typography: bundled Noto Sans SC in game canvas; Segoe UI / system in editor; Consolas for numeric values.
Spacing: 4 px base, 8 / 12 / 16 / 24 steps. Corners: 4 / 8 px. Shadows: subtle only for menus.
Motion: state feedback only; no decorative animation.
Assets: default assets/corridor.jpg and alternate assets/deck.jpg are real post-cutscene Android gameplay captures with a stable character. Historical colour is for layout reference only. Opening CG captures are not selected as backgrounds.
No invented game or brand imagery. The editor uses the game screenshot itself as its visual anchor.
Protected contracts: original game anchors, viewport and touch mapping; edited layers only affect drawing.
