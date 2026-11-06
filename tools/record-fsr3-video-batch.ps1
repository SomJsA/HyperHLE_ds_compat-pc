param(
    [Parameter(Mandatory = $true)]
    [string]$Adb,
    [Parameter(Mandatory = $true)]
    [string]$Serial,
    [Parameter(Mandatory = $true)]
    [string]$ScrcpyExe,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot,
    [string]$Package = 'org.touchhle.android',
    [int]$VideoSeconds = 20,
    [int]$SampleSeconds = 30,
    [int]$WarmupSeconds = 15
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Adb)) {
    throw "adb not found: $Adb"
}
if (-not (Test-Path -LiteralPath $ScrcpyExe)) {
    throw "scrcpy not found: $ScrcpyExe"
}

$runner = Join-Path $PSScriptRoot 'run-fsr3-device.ps1'
$batchStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$batchDir = Join-Path ([IO.Path]::GetFullPath($OutputRoot)) $batchStamp
New-Item -ItemType Directory -Force -Path $batchDir | Out-Null

$profiles = @(
    [pscustomobject]@{ Name = '01-native-scale1'; Mode = 'native'; Scale = 1 },
    [pscustomobject]@{ Name = '02-native-scale2'; Mode = 'native'; Scale = 2 },
    [pscustomobject]@{ Name = '03-native-scale3'; Mode = 'native'; Scale = 3 },
    [pscustomobject]@{ Name = '04-sync-fsr-scale2'; Mode = 'upscale-depth-gpu'; Scale = 2 },
    [pscustomobject]@{ Name = '05-async-fsr-scale1'; Mode = 'upscale-depth-gpu-async'; Scale = 1 },
    [pscustomobject]@{ Name = '06-async-fsr-scale2'; Mode = 'upscale-depth-gpu-async'; Scale = 2 },
    [pscustomobject]@{ Name = '07-async-fsr-scale3'; Mode = 'upscale-depth-gpu-async'; Scale = 3 },
    [pscustomobject]@{ Name = '08-async-direct-scale2'; Mode = 'upscale-depth-gpu-async-direct'; Scale = 2 },
    [pscustomobject]@{ Name = '09-async-direct-scale3'; Mode = 'upscale-depth-gpu-async-direct'; Scale = 3 }
)

$profiles | ConvertTo-Json |
    Set-Content -LiteralPath (Join-Path $batchDir 'profiles.json') -Encoding utf8

foreach ($profile in $profiles) {
    $profileRoot = Join-Path $batchDir $profile.Name
    Write-Output "=== $($profile.Name) mode=$($profile.Mode) scale=$($profile.Scale) ==="
    & $runner `
        -Adb $Adb `
        -Serial $Serial `
        -Package $Package `
        -Mode $profile.Mode `
        -ScaleHack $profile.Scale `
        -WarmupSeconds $WarmupSeconds `
        -SampleSeconds $SampleSeconds `
        -NavigateChapter8 `
        -ShowFps `
        -RecordVideo `
        -VideoSeconds $VideoSeconds `
        -VideoBitRate 20000000 `
        -ScrcpyExe $ScrcpyExe `
        -OutputRoot $profileRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Profile failed: $($profile.Name)"
    }
    Start-Sleep -Seconds 3
}

Write-Output $batchDir
