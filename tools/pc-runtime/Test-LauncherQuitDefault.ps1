param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $root -Force | Out-Null
$caseId = [Guid]::NewGuid().ToString('N')
$known = Join-Path $root ('known-' + $caseId)
$unknown = Join-Path $root ('unknown-' + $caseId)
New-Item -ItemType Directory -Path $known,$unknown | Out-Null

# Exercise the real build, not only a source-text assertion. Neither the
# generated launcher nor the emulator is executed. Every artifact is private.
$name = 'DeadSpace-quit-dialog-v1.txt'
Set-Content -LiteralPath (Join-Path $known $name) -Value '1' -Encoding ASCII
& (Join-Path $PSScriptRoot 'Build-Launcher.ps1') -OutputDirectory $known | Out-Null
if (Test-Path -LiteralPath (Join-Path $known $name)) { throw 'Known experimental marker survived a rebuild.' }
if (!(Test-Path -LiteralPath (Join-Path $known 'DeadSpace Launcher.exe'))) { throw 'Launcher output missing.' }

# Unknown files must not be deleted, or replaced by a new launcher, on failure.
$existing = Join-Path $unknown 'DeadSpace Launcher.exe'
Set-Content -LiteralPath $existing -Value 'private-existing-output' -Encoding ASCII
Set-Content -LiteralPath (Join-Path $unknown $name) -Value 'custom-marker' -Encoding ASCII
$before = (Get-FileHash -LiteralPath $existing).Hash
$failure = $null
try { & (Join-Path $PSScriptRoot 'Build-Launcher.ps1') -OutputDirectory $unknown | Out-Null }
catch { $failure = $_.Exception.Message }
if (!$failure -or !$failure.Contains('Unrecognized quit-dialog marker')) { throw 'Unknown marker was not rejected before compilation.' }
if ((Get-FileHash -LiteralPath $existing).Hash -ne $before) { throw 'Existing output changed on rejected build.' }
if ((Get-Content -Raw -LiteralPath (Join-Path $unknown $name)).Trim() -ne 'custom-marker') { throw 'Unknown marker changed.' }

$result = [pscustomobject]@{passed=$true;knownMarkerRemoved=$true;unknownMarkerAndOutputPreserved=$true;launcherNotExecuted=$true;emulatorNotExecuted=$true;knownOutput=$known;unknownOutput=$unknown}
$json = $result | ConvertTo-Json
$json | Set-Content -LiteralPath (Join-Path $root 'result.json') -Encoding UTF8
$json
