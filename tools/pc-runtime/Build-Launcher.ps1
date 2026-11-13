param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$output = Join-Path (Resolve-Path $OutputDirectory) 'DeadSpace Launcher.exe'
# Permanent accepted quit baseline (R7): keep the owned native legacy dialog.
# Rebuilding an old output directory must not reactivate the experimental theme.
$quitMarker = Join-Path (Split-Path -Parent $output) 'DeadSpace-quit-dialog-v1.txt'
if (Test-Path -LiteralPath $quitMarker) {
    $marker = Get-Item -LiteralPath $quitMarker
    if ($marker.PSIsContainer -or ($marker.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $marker.Length -gt 32 -or [IO.File]::ReadAllText($quitMarker).Trim() -ne '1') {
        throw 'Unrecognized quit-dialog marker; output files were preserved.'
    }
    Remove-Item -LiteralPath $quitMarker
}
$sources = @('Program.cs', 'LauncherShell.cs', 'TextureSettings.cs', 'HardwareDefaults.cs', 'SamplingSettings.cs', 'SaveSettings.cs', 'KeyBindings.cs', 'SaveStorage.cs', 'StorageControls.cs', 'GamepadBindings.cs', 'SaveDocument.cs', 'SaveEditor.cs', 'SaveBackup.cs') | ForEach-Object { Join-Path $PSScriptRoot $_ }
$sources += Join-Path $PSScriptRoot 'PcRenderResolution.cs'
$sources += Join-Path $PSScriptRoot 'DistributionLayout.cs'
$sources += Join-Path $PSScriptRoot 'GameSession.cs'
$sources += Join-Path $PSScriptRoot 'QuitConfirmation.cs'
$sources += Join-Path $PSScriptRoot 'VisualCompatibility.cs'
$sources += Join-Path $PSScriptRoot 'DistributionSettings.cs'
$sources += Join-Path $PSScriptRoot 'SaveBundle.cs'
$sources += Join-Path $PSScriptRoot 'StartupReview.cs'
$sources += Join-Path $PSScriptRoot 'FirstLaunchReview.cs'
$sources += Join-Path $PSScriptRoot 'DocumentsSettings.cs'
$sources += Join-Path $PSScriptRoot 'PlayDiagnostics.cs'
$sources += Join-Path $PSScriptRoot 'MarkerInstaller.cs'
$sources += Join-Path $PSScriptRoot 'UiLanguage.cs'
$sources += Join-Path $PSScriptRoot 'LauncherActions.cs'
$sources += Join-Path $PSScriptRoot 'LauncherLayout.cs'
$sources += Join-Path $PSScriptRoot 'ReleaseUi.cs'
$sources += Join-Path $PSScriptRoot 'LauncherAssemblyInfo.cs'
$sources += Join-Path $PSScriptRoot 'FlickerWarning.cs'
$hardwareProfile = Join-Path $PSScriptRoot 'HardwareProfiles.json'
$visualProfile = Join-Path $PSScriptRoot 'DeadSpace-VisualCompat.ini'
$poster = Join-Path $PSScriptRoot 'assets/DeadSpace-Poster.png'
$icon = Join-Path $PSScriptRoot 'assets/DeadSpace.ico'
if (!(Test-Path -LiteralPath $poster)) { throw 'Launcher poster asset is missing.' }
& $compiler /nologo /target:winexe /platform:x64 /optimize+ "/win32icon:$icon" "/win32manifest:$PSScriptRoot/Windows.manifest" "/out:$output" "/resource:$poster,DeadSpaceBootstrap.DeadSpace-Poster.png" "/resource:$icon,DeadSpaceBootstrap.DeadSpace.ico" "/resource:$visualProfile,DeadSpaceBootstrap.VisualCompat.ini" /r:System.Windows.Forms.dll /r:System.Drawing.dll /r:System.IO.Compression.dll /r:System.Web.Extensions.dll "/resource:$hardwareProfile,DeadSpaceBootstrap.HardwareProfiles.json" $sources
if ($LASTEXITCODE -ne 0) { throw 'Launcher compilation failed.' }
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'Test-VisualCompatibility.ps1') -LauncherPath $output -OutputDirectory (Join-Path $OutputDirectory 'launcher-build-check') | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Embedded visual compatibility regression check failed.' }
if (Test-Path -LiteralPath $quitMarker) { throw 'Legacy quit default was overwritten by build output.' }
Get-FileHash -LiteralPath $output
