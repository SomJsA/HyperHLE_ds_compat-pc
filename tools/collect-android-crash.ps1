param(
    [string]$Serial = "",
    [int]$TimeoutSeconds = 600
)

$ErrorActionPreference = "Stop"

$adbCommand = Get-Command adb -ErrorAction SilentlyContinue
if ($null -eq $adbCommand) {
    throw "adb was not found in PATH."
}
$adb = $adbCommand.Source
$serialArgs = @()
if (-not [string]::IsNullOrWhiteSpace($Serial)) {
    $serialArgs = @("-s", $Serial)
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $PSScriptRoot "crash-$stamp"
New-Item -ItemType Directory -Path $out -Force | Out-Null
$logPath = Join-Path $out "all-logcat.log"
$summaryPath = Join-Path $out "summary.txt"

& $adb @serialArgs wait-for-device
& $adb @serialArgs shell am force-stop org.touchhle.android
& $adb @serialArgs logcat -c

$logcatArgs = $serialArgs + @("logcat", "-b", "all", "-v", "threadtime")
$logcat = Start-Process `
    -FilePath $adb `
    -ArgumentList $logcatArgs `
    -NoNewWindow `
    -RedirectStandardOutput $logPath `
    -PassThru

try {
    & $adb @serialArgs shell monkey -p org.touchhle.android 1 | Out-Null
    Write-Host "Launched. Select your difficulty now." -ForegroundColor Green

    $seenProcess = $false
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $pidText = (& $adb @serialArgs shell pidof org.touchhle.android 2>$null | Out-String).Trim()
        if (-not [string]::IsNullOrWhiteSpace($pidText)) {
            if (-not $seenProcess) {
                Write-Host "touchHLE PID: $pidText"
                $seenProcess = $true
            }
        } elseif ($seenProcess) {
            Write-Host "touchHLE process exited; collecting crash evidence..." -ForegroundColor Yellow
            Start-Sleep -Seconds 5
            break
        }
        Start-Sleep -Seconds 1
    }
} finally {
    if (-not $logcat.HasExited) {
        Stop-Process -Id $logcat.Id -Force
    }
}

$deviceInfo = @()
$deviceInfo += "collected=$((Get-Date).ToString('o'))"
$deviceInfo += "model=$(& $adb @serialArgs shell getprop ro.product.model)"
$deviceInfo += "manufacturer=$(& $adb @serialArgs shell getprop ro.product.manufacturer)"
$deviceInfo += "android=$(& $adb @serialArgs shell getprop ro.build.version.release)"
$deviceInfo += "sdk=$(& $adb @serialArgs shell getprop ro.build.version.sdk)"
$deviceInfo += "abi=$(& $adb @serialArgs shell getprop ro.product.cpu.abi)"
$deviceInfo += "size=$(& $adb @serialArgs shell wm size)"
$deviceInfo += "density=$(& $adb @serialArgs shell wm density)"
$deviceInfo += "board=$(& $adb @serialArgs shell getprop ro.board.platform)"
$deviceInfo += "opengles=$(& $adb @serialArgs shell getprop ro.opengles.version)"
$deviceInfo += "vulkan=$(& $adb @serialArgs shell getprop ro.hardware.vulkan)"
$deviceInfo += ""
$deviceInfo += "PACKAGE"
$deviceInfo += (& $adb @serialArgs shell dumpsys package org.touchhle.android 2>&1)
$deviceInfo | Set-Content -LiteralPath (Join-Path $out "device-info.txt") -Encoding UTF8

$exitInfo = @()
$exitInfo += "=== activity exit-info ==="
$exitInfo += (& $adb @serialArgs shell dumpsys activity exit-info org.touchhle.android 2>&1)
$exitInfo += ""
$exitInfo += "=== crash logcat ==="
$exitInfo += (& $adb @serialArgs logcat -b crash -d -v threadtime 2>&1)
$exitInfo += ""
$exitInfo += "=== meminfo ==="
$exitInfo += (& $adb @serialArgs shell dumpsys meminfo org.touchhle.android 2>&1)
$exitInfo | Set-Content -LiteralPath (Join-Path $out "exit-info.txt") -Encoding UTF8

$applicationLogPath = Join-Path $out "touchHLE_log.txt"
$applicationLogCandidates = @(
    "/sdcard/Android/data/org.touchhle.android/files/touchHLE_log.txt",
    "/storage/emulated/0/Android/data/org.touchhle.android/files/touchHLE_log.txt"
)
$applicationLogCollected = $false
foreach ($candidate in $applicationLogCandidates) {
    $content = (& $adb @serialArgs shell run-as org.touchhle.android cat $candidate 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0 -and
        $content -notmatch "No such file|Permission denied|not debuggable" -and
        -not [string]::IsNullOrWhiteSpace($content)) {
        $content | Set-Content -LiteralPath $applicationLogPath -Encoding UTF8
        $applicationLogCollected = $true
        break
    }
}
Add-Content -LiteralPath (Join-Path $out "device-info.txt") -Value @(
    "",
    "application-log-collected=$applicationLogCollected"
)

$patterns = @(
    "FATAL EXCEPTION",
    "Fatal signal",
    "DEBUG",
    "BACKTRACE",
    "Abort message",
    "OutOfMemory",
    "EGL",
    "GL_",
    "SDL",
    "touchHLE",
    "libtouchHLE",
    "Could not prepare bundled IPA",
    "Launching bundled IPA"
)
Get-Content -LiteralPath $logPath -ErrorAction SilentlyContinue |
    Select-String -Pattern $patterns -SimpleMatch |
    Set-Content -LiteralPath $summaryPath -Encoding UTF8

Write-Host ""
Write-Host "Send this entire folder back:" -ForegroundColor Green
Write-Host "  $out"
