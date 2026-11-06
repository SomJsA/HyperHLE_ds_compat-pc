param(
    [string]$BaseZip = 'E:\BaiduSyncdisk\DeadSpace\Current\PC\DeadSpace-PC-OneClick-20261022-0340.zip',
    [string]$GameExe = 'E:\CodexProjects\DeadSpaceLegacy\run-host\touchHLE-texkey.exe',
    [string]$LauncherExe = 'E:\CodexProjects\DeadSpaceLegacy\run-host\pc-bootstrap\DeadSpace Launcher.exe',
    [string]$RuntimeDir = 'E:\CodexProjects\DeadSpaceLegacy\run-host\Runtime-PC',
    [string]$ShortcutPath = ''
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($ShortcutPath)) {
    $shortcutName = ([string][char]0x542F) + ([char]0x52A8) + ' Dead Space.lnk'
    $ShortcutPath = Join-Path (Split-Path -Parent $RuntimeDir) $shortcutName
}

if (-not (Test-Path -LiteralPath $RuntimeDir)) {
    if (-not (Test-Path -LiteralPath $BaseZip)) {
        throw "Base package missing: $BaseZip"
    }
    New-Item -ItemType Directory -Path $RuntimeDir -Force | Out-Null
    Expand-Archive -LiteralPath $BaseZip -DestinationPath $RuntimeDir
}

if (-not (Test-Path -LiteralPath $GameExe)) {
    throw "Game executable missing: $GameExe"
}
if (-not (Test-Path -LiteralPath $LauncherExe)) {
    throw "Launcher executable missing: $LauncherExe"
}

Copy-Item -LiteralPath $GameExe -Destination (Join-Path $RuntimeDir 'touchHLE.exe') -Force
Copy-Item -LiteralPath $LauncherExe -Destination (Join-Path $RuntimeDir 'DeadSpace Launcher.exe') -Force

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($ShortcutPath)
$shortcut.TargetPath = Join-Path $RuntimeDir 'DeadSpace Launcher.exe'
$shortcut.WorkingDirectory = $RuntimeDir
$shortcut.IconLocation = (Join-Path $RuntimeDir 'DeadSpace Launcher.exe') + ',0'
$shortcut.Description = 'Dead Space PC compatibility runtime'
$shortcut.Save()

$game = Get-Item -LiteralPath (Join-Path $RuntimeDir 'touchHLE.exe')
$launcher = Get-Item -LiteralPath (Join-Path $RuntimeDir 'DeadSpace Launcher.exe')
"RUNTIME=$RuntimeDir"
"SHORTCUT=$ShortcutPath"
"GAME_SHA256=$((Get-FileHash -LiteralPath $game.FullName -Algorithm SHA256).Hash.ToLowerInvariant())"
"LAUNCHER_SHA256=$((Get-FileHash -LiteralPath $launcher.FullName -Algorithm SHA256).Hash.ToLowerInvariant())"
