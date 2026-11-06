param(
    [string]$RuntimeDir = 'E:\CodexProjects\DeadSpaceLegacy\builds\pc-opening-diagnostic-20261025',
    [string]$EvidenceDir = 'E:\CodexProjects\DeadSpaceLegacy\logs\pc-opening-mark-20261025',
    [string]$Point = '350,230,8',
    [switch]$FogRed
)
$ErrorActionPreference = 'Stop'
if (Get-Process touchHLE -ErrorAction SilentlyContinue) {
    throw 'Exit the current game before starting the isolated opening capture.'
}
$launcher = Join-Path $RuntimeDir 'DeadSpace Launcher.exe'
if (-not (Test-Path -LiteralPath $launcher)) { throw "Missing prepared launcher: $launcher" }
New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null
$env:TOUCHHLE_DS_PIXEL_TRACE_WAIT_FOR_MARK = '1'
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DARK = $Point
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_AFTER_BRIGHT = '1'
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_CONFIRM_FRAMES = '2'
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_OPPOSITE_FRAMES = '15'
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DUMP = $EvidenceDir
$env:TOUCHHLE_DS_PIXEL_TRACE = $null
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_BRIGHT = $null
$env:TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_FRAME_DARK = $null
$env:TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME = $null
$env:TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME = $null
$env:TOUCHHLE_DS_DIAG_PRESERVE_FRAME_DUMP = '1'
$env:TOUCHHLE_DS_DUMP_FRAMES = $EvidenceDir
$env:TOUCHHLE_DS_DUMP_AT_PRESENT = '1'
$env:TOUCHHLE_DS_DUMP_FRAME_EVERY = '30'
$env:TOUCHHLE_DS_DUMP_FRAME_FROM = $null
$env:TOUCHHLE_DS_DUMP_FRAME_TO = $null
$env:TOUCHHLE_DS_DUMP_FRAME_ROTATE = '90'
$env:TOUCHHLE_DS_DUMP_FRAME_SCALE = '2'
$env:TOUCHHLE_DS_STATE_DIR = $EvidenceDir
$env:TOUCHHLE_DS_TEXTURE_TRACE = '1'
$env:TOUCHHLE_DS_TRACE_TERSE = '1'
$env:TOUCHHLE_DS_DRAW_LIMIT = '40000'
$env:TOUCHHLE_DS_TRACE_FROM_FRAME = $null
$env:TOUCHHLE_DS_DRAW_TRACE_TRIGGER = Join-Path $EvidenceDir 'draw-trace.enabled'
$env:TOUCHHLE_DS_UV_PROBE = '350,230'
$env:TOUCHHLE_DS_UV_PROBE_ON_DARK = '1'
$watcher = Join-Path $PSScriptRoot 'watch_opening_trace.py'
$env:TOUCHHLE_DS_FOG_RED = $null
$env:TOUCHHLE_DS_LOG_FOG = '1'
if ($FogRed) {
    $env:TOUCHHLE_DS_FOG_RED = '1'
    $env:TOUCHHLE_DS_TEXTURE_TRACE = $null
    $env:TOUCHHLE_DS_UV_PROBE = $null
    $env:TOUCHHLE_DS_UV_PROBE_ON_DARK = $null
} else {
    Start-Process -FilePath 'python' -ArgumentList @('"' + $watcher + '"', '"' + $EvidenceDir + '"') -WindowStyle Hidden
}
# This launcher lives in a separate directory, hence uses a separate sandbox.
# The production save files are not copied, overwritten or restored.
Start-Process -FilePath $launcher -WorkingDirectory $RuntimeDir
