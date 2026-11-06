param(
    [string]$RuntimeDir = 'E:\CodexProjects\DeadSpaceLegacy\builds\pc-opening-diagnostic-20261025',
    [string]$EvidenceDir = 'E:\CodexProjects\DeadSpaceLegacy\logs\pc-opening-fixed-20261025'
)
$ErrorActionPreference = 'Stop'
if (Get-Process touchHLE -ErrorAction SilentlyContinue) { throw 'Exit the current game first.' }
New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null
foreach ($name in @('FOG_RED', 'LOG_FOG', 'TEXTURE_TRACE', 'TRACE_TERSE', 'DRAW_TRACE_TRIGGER',
    'UV_PROBE', 'UV_PROBE_ON_DARK', 'PIXEL_TRACE', 'PIXEL_TRACE_TRIGGER_DARK',
    'PIXEL_TRACE_TRIGGER_BRIGHT', 'PIXEL_TRACE_TRIGGER_FRAME_DARK')) {
    # PowerShell binds $null to an empty string for this .NET overload.
    # Rust var_os().is_some() still enables an empty diagnostic variable.
    Remove-Item -LiteralPath ('Env:TOUCHHLE_DS_' + $name) -ErrorAction SilentlyContinue
}
$env:TOUCHHLE_DS_OPENING_FOG_FIX = '1'
$env:TOUCHHLE_DS_STATE_DIR = $EvidenceDir
$env:TOUCHHLE_DS_DIAG_PRESERVE_FRAME_DUMP = '1'
$env:TOUCHHLE_DS_DUMP_FRAMES = $EvidenceDir
$env:TOUCHHLE_DS_DUMP_AT_PRESENT = '1'
$env:TOUCHHLE_DS_DUMP_FRAME_EVERY = '0'
$env:TOUCHHLE_DS_DUMP_FRAME_ROTATE = '90'
$diagnostics = @('FOG_RED', 'LOG_FOG', 'TEXTURE_TRACE', 'TRACE_TERSE', 'DRAW_TRACE_TRIGGER',
    'UV_PROBE', 'UV_PROBE_ON_DARK', 'PIXEL_TRACE', 'PIXEL_TRACE_TRIGGER_DARK',
    'PIXEL_TRACE_TRIGGER_BRIGHT', 'PIXEL_TRACE_TRIGGER_FRAME_DARK')
foreach ($name in $diagnostics) {
    if (Test-Path -LiteralPath ('Env:TOUCHHLE_DS_' + $name)) { throw "Diagnostic still present: $name" }
}
@{ fogRedPresent = $false; heavyTracePresent = $false; openingFogFix = '1';
    executableSha256 = (Get-FileHash (Join-Path $RuntimeDir 'touchHLE.exe')).Hash;
    startedAt = (Get-Date).ToString('o') } | ConvertTo-Json |
    Set-Content -LiteralPath (Join-Path $EvidenceDir 'launch-settings.json') -Encoding utf8
# Only user F7/F5 marks are saved; no per-draw tracing or periodic readbacks.
Start-Process -FilePath (Join-Path $RuntimeDir 'DeadSpace Launcher.exe') -WorkingDirectory $RuntimeDir
