param(
    [Parameter(Mandatory=$true)][string]$ProgramDirectory,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$programDirectory = (Resolve-Path -LiteralPath $ProgramDirectory).Path
$outputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $outputDirectory) {
    throw 'Installer output directory must be new.'
}
if (!(Test-Path -LiteralPath (Join-Path $programDirectory 'DeadSpace Launcher.exe'))) {
    throw 'Program package launcher is missing.'
}
if (!(Test-Path -LiteralPath (Join-Path $programDirectory 'Game/touchHLE.exe'))) {
    throw 'Program package native runtime is missing.'
}
if (Test-Path -LiteralPath (Join-Path $programDirectory 'Game/DeadSpace.ipa')) {
    throw 'Program package must not contain the game IPA.'
}
if (Test-Path -LiteralPath (Join-Path $programDirectory 'Assets/texture-packs')) {
    throw 'Program package must not contain texture packs.'
}

New-Item -ItemType Directory -Path $outputDirectory | Out-Null
$programZip = Join-Path $outputDirectory 'program-payload.zip'
$uninstaller = Join-Path $outputDirectory 'DeadSpace-Uninstall.exe'
$installer = Join-Path $outputDirectory 'DEAD-SPACE-MOBILE-PC-Installer-20261112-R4.1-RC2.exe'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory(
    $programDirectory,
    $programZip,
    [IO.Compression.CompressionLevel]::Optimal,
    $false)

& $compiler /nologo /target:winexe /platform:x64 /optimize+ `
    "/out:$uninstaller" `
    "/win32icon:$PSScriptRoot/../pc-runtime/assets/DeadSpace.ico" `
    /r:System.Windows.Forms.dll `
    /r:System.Drawing.dll `
    (Join-Path $PSScriptRoot 'Uninstaller.cs') `
    (Join-Path $PSScriptRoot 'UninstallerAssemblyInfo.cs')
if ($LASTEXITCODE -ne 0) { throw 'Uninstaller compilation failed.' }

& $compiler /nologo /target:winexe /platform:x64 /optimize+ `
    "/out:$installer" `
    "/win32icon:$PSScriptRoot/../pc-runtime/assets/DeadSpace.ico" `
    "/win32manifest:$PSScriptRoot/../pc-runtime/Windows.manifest" `
    "/resource:$programZip,DeadSpaceInstaller.Program.zip" `
    "/resource:$uninstaller,DeadSpaceInstaller.Uninstall.exe" `
    /r:System.Windows.Forms.dll `
    /r:System.Drawing.dll `
    /r:System.IO.Compression.dll `
    /r:System.IO.Compression.FileSystem.dll `
    /r:System.Web.Extensions.dll `
    (Join-Path $PSScriptRoot 'Installer.cs') `
    (Join-Path $PSScriptRoot 'InstallerForm.cs') `
    (Join-Path $PSScriptRoot 'InstallerLanguage.cs') `
    (Join-Path $PSScriptRoot '../pc-runtime/ResourceProfiles.cs') `
    (Join-Path $PSScriptRoot 'InstallerAssemblyInfo.cs') `
    (Join-Path $PSScriptRoot '../pc-runtime/ReleaseUi.cs') `
    (Join-Path $PSScriptRoot '../pc-runtime/UiLanguage.cs')
if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }

& $installer --verify-embedded --log (Join-Path $outputDirectory 'embedded-check.txt')
if ($LASTEXITCODE -ne 0) { throw 'Embedded program verification failed.' }
Get-FileHash -LiteralPath $installer -Algorithm SHA256
