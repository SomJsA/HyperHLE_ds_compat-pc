[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ProgramRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $ProgramRoot).Path
$launcher = Join-Path $root 'DeadSpace Launcher.exe'
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    throw "找不到安装后的 DeadSpace Launcher.exe：$launcher"
}

$stateRoot = Join-Path $env:LOCALAPPDATA 'DeadSpaceLegacy'
New-Item -ItemType Directory -Force -Path $stateRoot | Out-Null
$skinTrigger = Join-Path $stateRoot 'optimized-skin.trigger'
$transformTrigger = Join-Path $stateRoot 'optimized-transform.trigger'
Set-Content -LiteralPath $skinTrigger -Value 'candidate' -Encoding ASCII
Set-Content -LiteralPath $transformTrigger -Value 'candidate' -Encoding ASCII

# Candidate-only child-process environment. The stable launcher defaults remain
# unchanged. Both native candidates independently retain original ARM execution
# during warmup/rechecks and disable themselves on unsupported input or mismatch.
$env:TOUCHHLE_DS_PC_JIT_PREWARM = '1'
$env:TOUCHHLE_DS_PC_COMBAT_LATENCY = '1'
$env:TOUCHHLE_DS_PC_SKIN_SHADOW = '1'
$env:TOUCHHLE_DS_PC_SKIN_MATH = 'host'
$env:TOUCHHLE_DS_PC_SKIN_INPUT = 'strided'
$env:TOUCHHLE_DS_PC_SKIN_TRIGGER = $skinTrigger
$env:TOUCHHLE_DS_PC_TRANSFORM_MATH = 'host'
$env:TOUCHHLE_DS_PC_TRANSFORM_TRIGGER = $transformTrigger

Write-Host 'Starting the comprehensive PC optimization candidate.'
Write-Host "ProgramRoot: $root"
Write-Host 'Modes: JIT prewarm + combat polling + host skin/strided + host transform'
Write-Host "Skin trigger: $skinTrigger"
Write-Host "Transform trigger: $transformTrigger"
Write-Host 'Remove both trigger files after the game exits if you want to clear the candidate markers.'
Start-Process -FilePath $launcher -WorkingDirectory $root
