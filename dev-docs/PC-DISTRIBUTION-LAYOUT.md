# PC distribution layout — 2026-11-04 user revision

New package layout uses English directory names:

```
DeadSpace Launcher.exe
Game/                  # touchHLE.exe, original DeadSpace.ipa, res, native support files, build-info and diagnostic map
Assets/texture-packs/   # installed 2x/4x/8x packs
Defaults/              # missing-only configuration templates, never active settings
Docs/                  # notices, instructions and reports
diagnostics/play-sessions/  # all new sessions, issue PNG/JSON and export inputs
```

The default user root is Documents/DeadSpaceLegacy. Configuration is config/pc;
native saves and editor backups retain the existing touchHLE_sandbox profile
format. Editor imports are copied to this user root too. Portable storage remains
an explicit choice (new layout: UserData); it does not move or merge progress.

First configuration reads physical monitor pixels, applies 1080p=4x,1440p=6x,
2160p=8x, texture/MSAA/AF4x and saves resolved internal/output selections.
The distribution defaults to fullscreen; owned automation uses an isolated
background window and never saves into the user configuration.
Subsequent launches load saved choices. The explicit automatic scale/output
options remain available; only those follow a changed monitor. Quality presets
do not change internal resolution. No unknown-hardware question, performance
test, test button or old benchmark recommendation appears in this PC flow.
Startup status still reports actual runtime feature confirmation and diagnostic
writer acknowledgement. Hardware identity is metadata, not an FPS guarantee.

When the English user root is absent, a legacy Documents/DeadSpace IOS移植 root
is copied to a unique staging directory and SHA256-verified before atomic
publication. The original is retained. All non-diagnostic files, backups and
empty directories are preserved; unknown/damaged layout markers and links are
rejected. An already existing English root is never merged or overwritten.
Old diagnostics stay where they were and remain selectable in export alongside
new program-local sessions. New package updates do not alter old releases.

Flat legacy packages and their active root INI are still readable. New packages
must not ship such an active root INI. Original IPA bytes are copied unchanged;
no IPA patch/repack is involved. Absolute user-selected external IPA/pack paths
remain explicit choices. The in-package default IPA is Game/DeadSpace.ipa.

Upload every file and subdirectory inside the package's diagnostics/play-sessions,
or export all sessions using the launcher. A nonwritable diagnostics directory
must show failure; no silent fallback to Documents and no false recording claim.

This contract supersedes older PC short-test/default Chinese-leaf wording in
storage/resolution documents. Android startup/storage is unchanged by this PC
implementation and still requires its own corresponding release validation.
