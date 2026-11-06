# Final opening-render verification

> Historical verification, superseded on 2026-10-25. The accepted asteroid
> fix restores normal depth writes and original culling, rather than forcing
> GL_ALWAYS. See `dev-docs/session-2026-10-25/WORKLOG-2026-10-25-FRAME2016.md`
> for the installed artifact, user-confirmed zero-g parameters, pending E/F
> gameplay checks, and the still-unimplemented manual checkpoint feature.

## Render configuration

- GPU: NVIDIA GeForce RTX 4090 D
- Drawable path: 4x MSAA with multisample depth cleared after resolve
- Station foundation exception: `02d3486c/count=405` disables only back-face
  culling for that batch
- Foreground asteroid exception: `d1feb687/count=600` uses targeted culling
  relief plus `GL_ALWAYS` depth compare while retaining depth writes
- Failed per-FBO depth, global no-cull and skip-triangle experiments are off

## Source

- `bb420b34 fix: restore opening station foundation rock`
- `e6476b0d docs: record launcher opening render verification`
- `d4c65100 fix: restore foreground asteroid during opening fly-through`

## Fixed runtime

- Shortcut:
  `E:\CodexProjects\DeadSpaceLegacy\run-host\启动 Dead Space.lnk`
- Launcher:
  `E:\CodexProjects\DeadSpaceLegacy\run-host\Runtime-PC\DeadSpace Launcher.exe`
- Launcher SHA-256:
  `15a82742dccb8c9559c33cf5ed23a1b2a31dbc4404042692c8253d967aaf5630`
- touchHLE SHA-256:
  `e41758376439321183503c6a6a948e3e0586e4be49a95a7d92d7a68a90ddafee`

The shortcut points directly at the launcher in `Runtime-PC`.

## OBS captures from the real launcher

The old-save -> chapter-select -> Chapter 1 route was run through
`DeadSpace Launcher.exe`, not the diagnostic wrapper.

- `launcher-fixed-frames-60fps.mp4`
  - 1920x1080, 60 fps, 15839 frames, 263.98 seconds
  - SHA-256:
    `15eb38e1dbf1ffcc71df4681187ccedf029e4a903b95ffa8af03df1a82e7ae46`
- `launcher-fixed-frames-10fps.mp4`
  - 1920x1080, 10 fps, 2640 frames, 264 seconds
  - SHA-256:
    `4ceca5ee1a276d9a037e3b2e53e0bf08cd576e9cfc402eaf32a4ba4056df1aa6`
- `depth-always-frames-60fps.mp4`
  - SHA-256:
    `22c757a5d54545249d42d435822876b2a706c5d490ffede79be14ab8c0c3ea90`
- `depth-always-frames-10fps.mp4`
  - SHA-256:
    `8138bbd71cd01d21191ecbea65a7c63e5271a2dfd740a9121f1c84b6acbdf554`

Both videos have a burned-in frame number.

## Visual checks

- Star field and nebula remain visible and intact.
- Station hull/tower scan-line artifacts are absent in the corresponding
  current shots.
- Space-station foundation rock matches the official Android capture:
  `official-vs-default-fix-rock.jpg`.
- Porthole shot shows the lit room, character, poster and lights rather than a
  black interior.
- Corridor shot shows the character weapon with its material rather than a
  solid black block.
- Safety poster is complete, with no triangular diagonal strip.
- `10fps FRAME 2016`: the rectangular cavity in the foreground asteroid is
  filled while preserving depth writes; the weapon remains textured.

Supporting comparisons:

- `logs/candidate-review-20261024/old-vs-new-requirements.jpg`
- `logs/foundation-default-fix2-20261024-0110/official-vs-default-fix-rock.jpg`
- `logs/launcher-fixed-20261024-1920/launcher-overview.jpg`
- `logs/depth-always-launcher-20261024-2025/official-vs-frame2016.jpg`
- `logs/candidate-review-20261024/depth-always-corridor.jpg`
