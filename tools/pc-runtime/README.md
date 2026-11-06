# Local PC runtime support snapshot

Current native launcher source is split across `Program.cs`, `LauncherShell.cs`,
`KeyBindings.cs`, and `SaveSettings.cs`. Build to a staging directory with
`Build-Launcher.ps1 -OutputDirectory <path>`. Do not copy only Program.cs or use
the historical outer bootstrap as the latest source.

Latest validation, deployment boundary and critical save-schema corrections:
`dev-docs/session-2026-10-25/PC-HANDOFF-NOW.md`. New binaries are not deployed.

These files are versioned copies of the outer DeadSpaceLegacy project's
`run-host/pc-bootstrap/Program.cs`, `run-host/shake-dialog.ps1`, and
`tools/install_pc_runtime.ps1` from 2026-10-25. The launcher clears opening
render experiments and the shake dialog defaults to the user-confirmed
`6,3,4,z` parameters.

The installer retains its existing machine-local defaults. Always pass the
newly built executable explicitly with `-GameExe`; its default executable
path is an older build. Do not replace a running user's game.

Large binaries, saves and recordings stay outside Git. Their current paths
and verification status are recorded in `dev-docs/session-2026-10-25`.
