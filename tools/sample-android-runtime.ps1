param(
    [Parameter(Mandatory = $true)]
    [string]$Adb,
    [Parameter(Mandatory = $true)]
    [string]$Serial,
    [string]$Package = 'org.touchhle.android',
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [int]$DurationSeconds = 600,
    [int]$IntervalSeconds = 30
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Adb)) {
    throw "adb not found: $Adb"
}
if ($DurationSeconds -le 0 -or $IntervalSeconds -le 0) {
    throw "DurationSeconds and IntervalSeconds must be positive"
}

$outputDir = Split-Path -Parent ([IO.Path]::GetFullPath($OutputPath))
if ($outputDir) {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}
'timestamp,pid,total_pss_kb,total_rss_kb,swap_pss_kb,native_heap_kb,graphics_kb,battery_temp_tenths_c,thermal_zone0_mc' |
    Set-Content -LiteralPath $OutputPath -Encoding utf8

$deadline = (Get-Date).AddSeconds($DurationSeconds)
while ((Get-Date) -lt $deadline) {
    $pidValue = (& $Adb -s $Serial shell pidof $Package 2>$null | Out-String).Trim()
    if (-not $pidValue) {
        "process-exited,$((Get-Date).ToString('o'))" |
            Add-Content -LiteralPath $OutputPath -Encoding utf8
        break
    }

    $meminfo = & $Adb -s $Serial shell dumpsys meminfo $Package 2>$null
    function Read-Metric([string]$pattern) {
        $text = $meminfo -join "`n"
        if ($text -match $pattern) { return $Matches[1] }
        return ''
    }

    $totalPss = Read-Metric 'TOTAL PSS:\s*(\d+)'
    $totalRss = Read-Metric 'TOTAL RSS:\s*(\d+)'
    $swapPss = Read-Metric 'TOTAL SWAP PSS:\s*(\d+)'
    $nativeHeap = Read-Metric 'Native Heap:\s*(\d+)'
    $graphics = Read-Metric '(?m)^\s*Graphics:\s*(\d+)'
    $batteryTemp = (& $Adb -s $Serial shell dumpsys battery |
        Select-String -Pattern '^\s*temperature:' |
        Select-Object -First 1).Line -replace '^\s*temperature:\s*', ''
    $zoneTemp = (& $Adb -s $Serial shell cat /sys/class/thermal/thermal_zone0/temp 2>$null |
        Out-String).Trim()

    @(
        (Get-Date).ToString('o'),
        $pidValue,
        $totalPss,
        $totalRss,
        $swapPss,
        $nativeHeap,
        $graphics,
        $batteryTemp,
        $zoneTemp
    ) -join ',' | Add-Content -LiteralPath $OutputPath -Encoding utf8

    Start-Sleep -Seconds $IntervalSeconds
}

Write-Output $OutputPath
