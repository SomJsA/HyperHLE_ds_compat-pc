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
$trigger = Join-Path $stateRoot 'skin-candidate.trigger'
Set-Content -LiteralPath $trigger -Value 'candidate' -Encoding ASCII

# Opt-in only: the launcher and its child inherit these values. The runtime
# keeps the original ARM function during the 64-call warmup and every 256-call
# recheck, and restores it permanently on unsupported input or any mismatch.
$env:TOUCHHLE_DS_PC_SKIN_SHADOW = '1'
$env:TOUCHHLE_DS_PC_SKIN_MATH = 'host'
$env:TOUCHHLE_DS_PC_SKIN_INPUT = 'strided'
$env:TOUCHHLE_DS_PC_SKIN_TRIGGER = $trigger

Write-Host 'Starting the opt-in CPU skin candidate.'
Write-Host "ProgramRoot: $root"
Write-Host 'Mode: host skin + strided input + original-output rechecks'
Write-Host "Trigger: $trigger"
Write-Host 'Remove the trigger file after the game exits if you want to clear the candidate marker.'
Start-Process -FilePath $launcher -WorkingDirectory $root
