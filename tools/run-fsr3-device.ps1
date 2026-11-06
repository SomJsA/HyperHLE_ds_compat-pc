param(
    [Parameter(Mandatory = $true)]
    [string]$Adb,

    [string]$Serial = '',
    [string]$Package = 'org.touchhle.android.fsr3dev',
    [string]$Apk = '',
    [string]$IpaSource = '/sdcard/Android/data/org.touchhle.android/files/touchHLE_apps/DeadSpace.ipa',
    [ValidateSet('native', 'upscale', 'upscale-depth', 'upscale-depth-gpu', 'upscale-depth-gpu-async', 'upscale-depth-gpu-async-direct', 'upscale-depth-diag', 'fg-safe', 'fg-opticalflow', 'fg-synthetic')]
    [string]$Mode = 'native',
    [int]$ScaleHack = 2,
    [int]$FpsCap = 0,
    [int]$WarmupSeconds = 20,
    [int]$SampleSeconds = 60,
    [switch]$NavigateChapter12,
    [switch]$NavigateChapter8,
    [switch]$ShowFps,
    [switch]$RecordVideo,
    [int]$VideoSeconds = 20,
    [string]$VideoBitRate = '20000000',
    [string]$ScrcpyExe = '',
    [string]$OutputRoot = ''
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Adb)) {
    throw "adb not found: $Adb"
}
if (-not $Serial) {
    $devices = & $Adb devices
    $online = @($devices | Select-String '^\S+\s+device$' | ForEach-Object {
        ($_.Line -split '\s+')[0]
    })
    if ($online.Count -ne 1) {
        throw "Expected exactly one online device; pass -Serial. Online: $($online -join ', ')"
    }
    $Serial = $online[0]
}

function Invoke-DeviceAdb {
    param([string[]]$DeviceArgs)
    & $Adb -s $Serial @DeviceArgs
    if ($LASTEXITCODE -ne 0) {
        throw "adb command failed ($LASTEXITCODE): $($DeviceArgs -join ' ')"
    }
}

function Save-AdbText {
    param([string]$Name, [string[]]$DeviceArgs)
    $path = Join-Path $runDir $Name
    & $Adb -s $Serial @DeviceArgs 2>&1 | Set-Content -LiteralPath $path -Encoding utf8
}

function Save-Screenshot {
    param([string]$Name)
    $path = Join-Path $runDir $Name
    $process = Start-Process -FilePath $Adb -ArgumentList @(
        '-s', $Serial, 'exec-out', 'screencap', '-p'
    ) -RedirectStandardOutput $path -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "screencap failed: $Name"
    }
}

function Tap-And-Shoot {
    param([int]$X, [int]$Y, [string]$Name, [int]$DelayMs = 1200)
    Invoke-DeviceAdb @('shell', 'input', 'tap', "$X", "$Y")
    Start-Sleep -Milliseconds $DelayMs
    Save-Screenshot $Name
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $OutputRoot) {
    $OutputRoot = Join-Path $PSScriptRoot "..\target\fsr3-device-runs"
}
$runDir = Join-Path ([IO.Path]::GetFullPath($OutputRoot)) "$Mode-$stamp"
New-Item -ItemType Directory -Path $runDir -Force | Out-Null
if ($RecordVideo -and $VideoSeconds -le 0) {
    throw "VideoSeconds must be positive when RecordVideo is enabled"
}
if ($RecordVideo -and (-not $ScrcpyExe -or -not (Test-Path -LiteralPath $ScrcpyExe))) {
    throw "ScrcpyExe is required when RecordVideo is enabled"
}
if ($NavigateChapter12 -and $NavigateChapter8) {
    throw "Choose either NavigateChapter12 or NavigateChapter8"
}

Invoke-DeviceAdb @('shell', 'svc', 'power', 'stayon', 'true')

if ($Apk) {
    if (-not (Test-Path -LiteralPath $Apk)) {
        throw "APK not found: $Apk"
    }
    Invoke-DeviceAdb @('install', '--no-streaming', '-r', $Apk)
}

$appFiles = "/sdcard/Android/data/$Package/files"
$ipaPath = "$appFiles/touchHLE_apps/DeadSpace.ipa"
Invoke-DeviceAdb @('shell', 'mkdir', '-p', "$appFiles/touchHLE_apps")
& $Adb -s $Serial shell test -f $ipaPath
$hasIpa = $LASTEXITCODE -eq 0
if (-not $hasIpa) {
    Invoke-DeviceAdb @('shell', 'cp', $IpaSource, $ipaPath)
}

Invoke-DeviceAdb @('shell', 'am', 'force-stop', $Package)
Invoke-DeviceAdb @('logcat', '-c')

$launch = @(
    'shell', 'am', 'start', '-W',
    '-n', "$Package/.LauncherActivity",
    '--ez', 'autorun', 'true',
    '--es', 'ipa_path', $ipaPath,
    '--ei', 'scale_hack', "$ScaleHack",
    '--ei', 'fps_cap', "$FpsCap",
    '--ez', 'stats_log', 'true',
    '--ez', 'show_fps', $($ShowFps.IsPresent.ToString().ToLowerInvariant()),
    '--ez', 'gpu_present_copy', 'true',
    '--ez', 'fast_present', 'true'
)
switch ($Mode) {
    'native' {}
    'upscale' {
        $launch += @('--ez', 'fsr3_ahb_probe', 'true')
    }
    'upscale-depth' {
        $launch += @('--ez', 'fsr3_ahb_probe', 'true', '--ez', 'fsr3_real_depth', 'true')
    }
    'upscale-depth-gpu' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_gpu_depth', 'true'
        )
    }
    'upscale-depth-gpu-async' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_gpu_depth', 'true',
            '--ez', 'fsr3_async', 'true'
        )
    }
    'upscale-depth-gpu-async-direct' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_gpu_depth', 'true',
            '--ez', 'fsr3_async', 'true',
            '--ez', 'fsr3_direct_output', 'true'
        )
    }
    'upscale-depth-diag' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_diag_readback', 'true',
            '--ez', 'fsr3_trace', 'true'
        )
    }
    'fg-safe' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_fg', 'true'
        )
    }
    'fg-opticalflow' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_fg', 'true',
            '--ez', 'fsr3_fg_probe', 'true',
            '--ez', 'fsr3_diag_readback', 'true',
            '--ez', 'fsr3_trace', 'true'
        )
    }
    'fg-synthetic' {
        $launch += @(
            '--ez', 'fsr3_ahb_probe', 'true',
            '--ez', 'fsr3_real_depth', 'true',
            '--ez', 'fsr3_fg', 'true',
            '--ez', 'fsr3_fg_synthetic', 'true',
            '--ez', 'fsr3_diag_readback', 'true',
            '--ez', 'fsr3_trace', 'true'
        )
    }
}
Invoke-DeviceAdb $launch

$logName = 'logcat.txt'
$logPath = Join-Path $runDir $logName
$logErrorPath = Join-Path $runDir 'logcat-stderr.txt'
$logcat = Start-Process -FilePath $Adb -ArgumentList @(
    '-s', $Serial, 'logcat', '-v', 'threadtime'
) -RedirectStandardOutput $logPath -RedirectStandardError $logErrorPath `
  -WindowStyle Hidden -PassThru

try {
    $videoProcess = $null
    $localVideo = $null
    Start-Sleep -Seconds $WarmupSeconds
    Save-Screenshot 'warmup.png'
    if ($NavigateChapter12) {
        # Coordinates target the 2376x1080 landscape surface used by the
        # connected OnePlus device. The save-slot screen exposes slot 1,
        # confirmation, and the gameplay hint in that order.
        # A cold start can still be on the audio-device tip. Center-tap it
        # before touching the title-screen controls.
        Tap-And-Shoot 1188 540 'nav-00-startup.png' 10000
        Tap-And-Shoot 650 970 'nav-01-start.png' 2500
        Tap-And-Shoot 680 650 'nav-02-save1.png' 2500
        Tap-And-Shoot 1810 1000 'nav-03-confirm.png' 10000
        Tap-And-Shoot 1188 540 'nav-04-gameplay.png' 12000
    }
    if ($NavigateChapter8) {
        Tap-And-Shoot 1188 540 'nav-00-startup.png' 10000
        Tap-And-Shoot 650 970 'nav-01-start.png' 2500
        Tap-And-Shoot 680 650 'nav-02-save1.png' 2500
        Tap-And-Shoot 1475 205 'nav-03-chapter-mode.png' 1200
        Tap-And-Shoot 1810 1000 'nav-04-chapter-list.png' 2500
        for ($chapterPage = 0; $chapterPage -lt 5; $chapterPage++) {
            Invoke-DeviceAdb @('shell', 'input', 'tap', '1880', '530')
            Start-Sleep -Milliseconds 700
        }
        Save-Screenshot 'nav-05-chapter8-page.png'
        Tap-And-Shoot 930 440 'nav-06-chapter8-selected.png' 800
        Tap-And-Shoot 1810 1000 'nav-07-chapter8-confirm.png' 15000
        Tap-And-Shoot 1188 540 'nav-08-gameplay.png' 12000
    }
    if ($RecordVideo) {
        $localVideo = Join-Path $runDir 'video.mp4'
        $videoErrorPath = Join-Path $runDir 'scrcpy-stderr.txt'
        $videoProcess = Start-Process -FilePath $ScrcpyExe -ArgumentList @(
            '-s', $Serial,
            '--no-window',
            '--no-audio',
            '--max-size=0',
            "--video-bit-rate=$VideoBitRate",
            "--record=$localVideo",
            '--record-format=mp4',
            "--time-limit=$VideoSeconds"
        ) -RedirectStandardOutput (Join-Path $runDir 'scrcpy-stdout.txt') `
          -RedirectStandardError $videoErrorPath -WindowStyle Hidden -PassThru
        Write-Output "Recording video: $localVideo"
    }
    Start-Sleep -Seconds $SampleSeconds
    Save-Screenshot 'sample.png'
} finally {
    if ($logcat -and -not $logcat.HasExited) {
        $logcat.Kill()
        $logcat.WaitForExit()
    }
    if ($videoProcess -and -not $videoProcess.HasExited) {
        $videoProcess.WaitForExit(30000) | Out-Null
        if (-not $videoProcess.HasExited) {
            $videoProcess.Kill()
            $videoProcess.WaitForExit()
        }
    }
}

if ($RecordVideo) {
    if (-not $localVideo -or -not (Test-Path -LiteralPath $localVideo)) {
        throw "Video recording did not produce $localVideo"
    }
    if (Get-Command ffprobe -ErrorAction SilentlyContinue) {
        & ffprobe -v error -show_entries format=duration,size,bit_rate `
            -show_entries stream=index,codec_name,width,height,r_frame_rate,avg_frame_rate `
            -of json $localVideo |
            Set-Content -LiteralPath (Join-Path $runDir 'video-ffprobe.json') -Encoding utf8
    }
}

Save-AdbText 'getprop.txt' @('shell', 'getprop')
Save-AdbText 'wm-size.txt' @('shell', 'wm', 'size')
Save-AdbText 'wm-density.txt' @('shell', 'wm', 'density')
Save-AdbText 'display.txt' @('shell', 'dumpsys', 'display')
Save-AdbText 'package.txt' @('shell', 'dumpsys', 'package', $Package)
Save-AdbText 'meminfo.txt' @('shell', 'dumpsys', 'meminfo', $Package)
Save-AdbText 'gfxinfo.txt' @('shell', 'dumpsys', 'gfxinfo', $Package)
Save-AdbText 'thermalservice.txt' @('shell', 'dumpsys', 'thermalservice')
Save-AdbText 'battery.txt' @('shell', 'dumpsys', 'battery')
Save-AdbText 'top.txt' @('shell', 'top', '-b', '-n', '1')

$config = @(
    "serial=$Serial",
    "package=$Package",
    "mode=$Mode",
    "apk=$Apk",
    "ipa_path=$ipaPath",
    "scale_hack=$ScaleHack",
    "fps_cap=$FpsCap",
    "warmup_seconds=$WarmupSeconds",
    "sample_seconds=$SampleSeconds",
    "show_fps=$($ShowFps.IsPresent)",
    "record_video=$($RecordVideo.IsPresent)",
    "video_seconds=$VideoSeconds",
    "video_bit_rate=$VideoBitRate",
    "video_backend=scrcpy",
    "scrcpy_exe=$ScrcpyExe",
    "navigate_chapter12=$($NavigateChapter12.IsPresent)",
    "navigate_chapter8=$($NavigateChapter8.IsPresent)",
    "started_at=$stamp"
)
$config | Set-Content -LiteralPath (Join-Path $runDir 'config.txt') -Encoding utf8
Write-Output $runDir
