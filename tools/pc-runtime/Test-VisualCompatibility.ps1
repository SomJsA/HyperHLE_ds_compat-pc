param([Parameter(Mandatory=$true)][string]$LauncherPath, [Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$launcher = (Resolve-Path -LiteralPath $LauncherPath).Path
$assembly = [Reflection.Assembly]::LoadFile($launcher)
$type = $assembly.GetType('DeadSpaceBootstrap.VisualCompatibility')
$flags = [Reflection.BindingFlags]'Static,NonPublic'
$expected = @{}
foreach ($line in (Get-Content -LiteralPath (Join-Path $repoRoot 'tools\pc-runtime\DeadSpace-VisualCompat.ini'))) {
    if ($line.Trim() -and !$line.Trim().StartsWith('#')) {
        $pair = $line -split '=',2; $expected[$pair[0]] = $pair[1]
    }
}
$embedded = $type.GetMethod('ReadDefaults',$flags).Invoke($null,@())
if ($embedded.Count -ne $expected.Count) { throw 'Embedded/default profile key count differs.' }
foreach ($key in $expected.Keys) { if ($embedded[$key] -ne $expected[$key]) { throw "Embedded mismatch: $key" } }
foreach ($key in @('TOUCHHLE_DS_PC_DEPTH24','TOUCHHLE_DS_OPENING_FOG_FIX','TOUCHHLE_DS_BLOOD_DEPTH_FIX','TOUCHHLE_DS_AUTO_MIPS_HASH','TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH','TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT','TOUCHHLE_DS_DEPTH_LEQUAL_HASH','TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE','TOUCHHLE_DS_NO_FOG_HASH')) {
    if (!$embedded.ContainsKey($key)) { throw "Missing known visual repair: $key" }
}
# Independent regression contract for the five accepted alpha-aware mip fixes.
# Comparing only the embedded file to its source would miss deleting a key from both.
foreach ($name in @('TOUCHHLE_DS_AUTO_MIPS_HASH','TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH')) {
    $keys = $embedded[$name] -split ','
    foreach ($key in @('0712153f','24bec7ad','60bb1a47','e0549cfb','02d3486c')) {
        if ($keys -notcontains $key) { throw "Missing accepted alpha mip repair: $name / $key" }
    }
    # Shared content keys must not be added only to Android or only to PC.
    $rustDefaults = [IO.File]::ReadAllText((Join-Path $repoRoot 'src\lib.rs'))
    $match = [regex]::Match($rustDefaults, '"' + $name + '"\s*,\s*"([^"]+)"')
    if (!$match.Success -or (Compare-Object ($match.Groups[1].Value -split ',' | Sort-Object) ($keys | Sort-Object))) {
        throw "PC/Android alpha mip repair tables differ: $name"
    }
}
$diagnosticName = 'TOUCHHLE_DS_DIAG_PRESERVE_DEPTH_HASH'
$savedDiagnostic = [Environment]::GetEnvironmentVariable($diagnosticName)
try {
    [Environment]::SetEnvironmentVariable($diagnosticName,$null)
    foreach ($initial in @($null,'stale-parent-value')) {
        foreach ($key in $expected.Keys) { [Environment]::SetEnvironmentVariable($key,$initial) }
        $type.GetMethod('Apply',$flags).Invoke($null,@())
        foreach ($key in $expected.Keys) { if ([Environment]::GetEnvironmentVariable($key) -ne $expected[$key]) { throw "Effective mismatch: $key" } }
    }
    # This explicit diagnostic is separate from normal quality defaults.
    [Environment]::SetEnvironmentVariable($diagnosticName,'1')
    [Environment]::SetEnvironmentVariable('TOUCHHLE_DS_DEPTH_LEQUAL_HASH','diagnostic-sentinel')
    $type.GetMethod('Apply',$flags).Invoke($null,@())
    if ([Environment]::GetEnvironmentVariable('TOUCHHLE_DS_DEPTH_LEQUAL_HASH') -ne 'diagnostic-sentinel') { throw 'Explicit depth-hash diagnostic was lost.' }
}
finally {
    [Environment]::SetEnvironmentVariable($diagnosticName,$savedDiagnostic)
    [Environment]::SetEnvironmentVariable('TOUCHHLE_DS_DEPTH_LEQUAL_HASH',$expected['TOUCHHLE_DS_DEPTH_LEQUAL_HASH'])
}
$testDir = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $testDir -Force | Out-Null
$info = New-Object Diagnostics.ProcessStartInfo
$info.FileName = $launcher
$info.UseShellExecute = $false
$info.EnvironmentVariables['TOUCHHLE_DS_MSAA'] = '0'
$type.GetMethod('WriteEffective',$flags).Invoke($null,[object[]]@([string]$testDir,[Diagnostics.ProcessStartInfo]$info))
$effective = Get-Content -LiteralPath (Join-Path $testDir 'DeadSpace-visual-effective.ini') -Raw
$profileHash = (Get-FileHash -LiteralPath (Join-Path $repoRoot 'tools\pc-runtime\DeadSpace-VisualCompat.ini')).Hash
if (!$effective.Contains('visual_profile_sha256=' + $profileHash)) { throw 'Embedded profile SHA-256 differs from source.' }
if (!$effective.Contains('TOUCHHLE_DS_PC_DEPTH24=1') -or !$effective.Contains('TOUCHHLE_DS_MSAA=0')) { throw 'Depth repair was coupled to MSAA.' }
# A preset can erase a repair after Apply. Verify the final child boundary
# rejects each such regression, rather than only comparing two default tables.
$negativeCases = 0
function Test-RejectedRepair([string]$name, [string]$value) {
    $previous = $info.EnvironmentVariables[$name]
    $failure = $null
    try {
        $info.EnvironmentVariables[$name] = $value
        try { $type.GetMethod('WriteEffective',$flags).Invoke($null,[object[]]@([string](Join-Path $testDir 'must-not-write'),[Diagnostics.ProcessStartInfo]$info)) }
        catch { $failure = $_.Exception; while ($failure.InnerException) { $failure = $failure.InnerException } }
        if (!($failure -is [IO.InvalidDataException])) { throw "Final child repair gate did not reject: $name / $value" }
    }
    finally { $info.EnvironmentVariables[$name] = $previous }
}
Test-RejectedRepair 'TOUCHHLE_DS_PC_DEPTH24' '0'; $negativeCases++
foreach ($name in @('TOUCHHLE_DS_AUTO_MIPS_HASH','TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH')) {
    foreach ($missing in @('0712153f','24bec7ad','60bb1a47','e0549cfb','02d3486c')) {
        $remaining = (($embedded[$name] -split ',') | Where-Object { $_ -ne $missing }) -join ','
        Test-RejectedRepair $name $remaining; $negativeCases++
    }
}
if (Test-Path -LiteralPath (Join-Path $testDir 'must-not-write')) { throw 'Rejected repair settings wrote output.' }
# Custom texture keys are allowed in addition to the permanent repairs.
foreach ($name in @('TOUCHHLE_DS_AUTO_MIPS_HASH','TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH')) { $info.EnvironmentVariables[$name] += ',deadbeef' }
$type.GetMethod('WriteEffective',$flags).Invoke($null,[object[]]@([string]$testDir,[Diagnostics.ProcessStartInfo]$info))
[pscustomobject]@{passed=$true;embeddedKeys=$embedded.Count;parentCases=2;msaaOffKeepsDepth24=$true;rejectedChildRegressions=$negativeCases;extraTextureKeysAllowed=$true;explicitDepthHashDiagnosticPreserved=$true} | ConvertTo-Json | Tee-Object -FilePath (Join-Path $testDir 'result.json')
