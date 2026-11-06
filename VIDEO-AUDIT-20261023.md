# Opening video audit (2026-10-23)

## Review files

- Full review master, 30 fps with burned-in frame numbers:
  `E:/CodexProjects/DeadSpaceLegacy/logs/DeadSpace-opening-final-frames-20261023.mp4`
- 10 fps review copy with burned-in frame numbers (1849 frames):
  `E:/CodexProjects/DeadSpaceLegacy/logs/DeadSpace-opening-final-10fps-frames-20261023.mp4`
- Cockpit section, 10 fps frames 1310-1420:
  `E:/CodexProjects/DeadSpaceLegacy/logs/porthole-frames-1310-1420.mp4`
- Safety-poster section, 10 fps frames 1500-1660:
  `E:/CodexProjects/DeadSpaceLegacy/logs/poster-frames-1500-1660.mp4`

The frame numbers burned into the 10 fps copy are indices in that copy. They
are not guest renderer frame counters.

## Findings

### Star field

Frames around 2700-3000 show the planet, nebula, stars, and asteroids. The
previous black cover over the nebula is absent.

The working setup is scoped to the four iPad2 space-background atlases:

```text
TOUCHHLE_DS_DEPTH_LEQUAL_HASH=2131afd5,18c949fb,d1feb687,f4d178fa
TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE=1
```

Using global `TOUCHHLE_DS_DEPTH_LEQUAL=1` leaves black wedges in gameplay.
The background tie break must remain scoped and must not write depth.

### Cockpit

Frames 1310-1420 show the exterior docking structure and cockpit. The lit room
inside still has mostly black walls; the character and the right-hand lit panel
are visible. This remains a real unresolved defect.

### Chapter 1 safety poster

Frames 1590 and 1610 show two red panels, a yellow field, and large black hand
silhouettes. These black areas match the source artwork in texture
`56c62e8a`, not an overlaid stripe texture. Rendering the atlas as a flat colour
covers the entire rectangular poster geometry, so the poster quad is present.
The reviewer still considers this rendering incorrect, so it remains open
pending a same-camera reference that resolves whether the dark hand shapes are
expected.

The review copy at
`E:/CodexProjects/DeadSpaceLegacy/logs/poster-source-vs-final1590.jpg` puts the
source texture and frame 1590 side by side.

### Station

Frames 3000-3900 do not show the former large black missing triangles over the
station. Thin panel seams remain visible and match the source geometry.

### Confirmed residual geometry failures

- Frame 1000: the large white station cylinder contains many dense horizontal
  and diagonal dark bands. The closest official reference is its 15 second
  frame (`hle1000-vs-official15.jpg`); the official surface is substantially
  smoother. This is a real mismatch.
- Frame 1253: the left-hand building shows the same kind of repeated diagonal
  cuts.
- Frames 1310-1420: the cockpit walls are still black apart from the character
  and one lit panel.

The frame-1000 mismatch survives all of these A/B runs:

- global depth test disabled without global LEQUAL;
- automatic opening mip rebuild disabled;
- back-face culling disabled.
- global infinite-far projection forced on;
- a strong positive LOD bias on the scene atlases.

This rules out simple depth fighting, the current clean-mip rebuild, and
back-face culling as the primary cause. The next target is the indexed
triangle/UV batches and their texture matrices, using the indexed draw data
rather than additional global texture filters.
