param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$Output
)

$ErrorActionPreference = 'Stop'
$installer = (Resolve-Path -LiteralPath $Installer).Path
$output = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $output) { throw 'New test output required.' }
New-Item -ItemType Directory -Path $output | Out-Null

function Hash-File([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function New-FixtureIpa {
    Add-Type -AssemblyName System.IO.Compression
    $memory = New-Object IO.MemoryStream
    $archive = New-Object IO.Compression.ZipArchive($memory,[IO.Compression.ZipArchiveMode]::Create,$true)
    try {
        foreach ($name in @('Payload/Fixture.app/Info.plist','Payload/Fixture.app/Dead Space','Payload/Fixture.app/published/test')) {
            $target = $archive.CreateEntry($name).Open()
            try { $bytes = [Text.Encoding]::UTF8.GetBytes('synthetic fixture, not a game IPA'); $target.Write($bytes,0,$bytes.Length) } finally { $target.Dispose() }
        }
    } finally { $archive.Dispose() }
    try { return ,$memory.ToArray() } finally { $memory.Dispose() }
}

function New-TestMarker([string]$Path) {
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $fixtures = [ordered]@{
        'payload/DeadSpace.ipa' = (New-FixtureIpa)
        'payload/texture-packs/2/PACK.json' = [Text.Encoding]::UTF8.GetBytes('{"tier":2}')
        'payload/texture-packs/2/fixture.png' = [byte[]](1,2,3,4)
        'payload/texture-packs/4/PACK.json' = [Text.Encoding]::UTF8.GetBytes('{"tier":4}')
        'payload/texture-packs/4/fixture.png' = [byte[]](5,6,7,8)
        'payload/texture-packs/6/PACK.json' = [Text.Encoding]::UTF8.GetBytes('{"tier":6}')
        'payload/texture-packs/6/fixture.png' = [byte[]](9,10,11,12)
    }
    $entries = @()
    foreach ($name in $fixtures.Keys) {
        $bytes = $fixtures[$name]
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $digest = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-','').ToLowerInvariant() }
        finally { $sha.Dispose() }
        $entries += [ordered]@{
            path = $name
            size = $bytes.Length
            sha256 = $digest
            role = $(if ($name -eq 'payload/DeadSpace.ipa') { 'ipa' } else { 'texture' })
        }
    }
    $manifest = [ordered]@{
        schema = 1
        kind = 'deadspace-vandal-marker'
        marker_id = ('a' * 64)
        marker_version = 'installer-fixture-v1'
        game = [ordered]@{ ipa_path = 'payload/DeadSpace.ipa' }
        texture_tiers = [ordered]@{ '2' = $true; '4' = $true; '6' = $true }
        entries = $entries
    }
    $manifestBytes = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Depth 8))
    $stream = [IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    try {
        $archive = New-Object IO.Compression.ZipArchive($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
        try {
            $entry = $archive.CreateEntry('vandal.json',[IO.Compression.CompressionLevel]::Optimal)
            $target = $entry.Open()
            try { $target.Write($manifestBytes,0,$manifestBytes.Length) } finally { $target.Dispose() }
            foreach ($name in $fixtures.Keys) {
                $entry = $archive.CreateEntry($name,[IO.Compression.CompressionLevel]::Optimal)
                $target = $entry.Open()
                try { $target.Write($fixtures[$name],0,$fixtures[$name].Length) } finally { $target.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
}

function Invoke-Installer([string[]]$Arguments,[string]$Log) {
    $process = Start-Process -FilePath $installer -ArgumentList $Arguments -PassThru -Wait -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        $detail = $(if (Test-Path -LiteralPath $Log) { Get-Content -Raw -LiteralPath $Log } else { '' })
        throw "Installer failed with exit $($process.ExitCode): $detail"
    }
}

$marker = Join-Path $output 'vandal.marker'
New-TestMarker $marker
$markerHash = Hash-File $marker
Set-ItemProperty -LiteralPath $marker -Name IsReadOnly -Value $true

$program = Join-Path $output 'Program'
$gameData = Join-Path $output 'GameData'
$markerLog = Join-Path $output 'install-marker.log'
Invoke-Installer @(
    '--silent',
    '--program-root', $program,
    '--game-data-root', $gameData,
    '--resource', $marker,
    '--tier', '4',
    '--no-shortcut',
    '--no-register',
    '--log', $markerLog
) $markerLog

if (!(Test-Path -LiteralPath (Join-Path $program 'DeadSpace Launcher.exe'))) { throw 'Launcher missing.' }
if (!(Test-Path -LiteralPath (Join-Path $program 'Game/touchHLE.exe'))) { throw 'Native runtime missing.' }
if (!(Test-Path -LiteralPath (Join-Path $program 'installer.json'))) { throw 'Installer state missing.' }
if (!(Test-Path -LiteralPath (Join-Path $program 'DeadSpace-Uninstall.exe'))) { throw 'Uninstaller missing.' }
$markerInstall = Get-ChildItem -LiteralPath (Join-Path $gameData 'marker-content') -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'installation.json') } |
    Select-Object -First 1
if (!$markerInstall) { throw 'Installed marker record missing.' }
if (!(Test-Path -LiteralPath (Join-Path $markerInstall.FullName 'payload/DeadSpace.ipa'))) { throw 'Installed IPA missing.' }
if (!(Test-Path -LiteralPath (Join-Path $markerInstall.FullName 'payload/texture-packs/4/PACK.json'))) { throw 'Installed 4x pack missing.' }
if (Test-Path -LiteralPath (Join-Path $markerInstall.FullName 'payload/texture-packs/2/PACK.json')) { throw 'Unrequested 2x pack was installed.' }
if ((Hash-File $marker) -ne $markerHash) { throw 'Marker changed during install.' }

$sentinelData = Join-Path $gameData 'upgrade-sentinel.txt'
$sentinelProgram = Join-Path $program 'program-unknown-file.txt'
[IO.File]::WriteAllText($sentinelData,'keep game data')
[IO.File]::WriteAllText($sentinelProgram,'keep unknown program file')
$markerHidden = Join-Path $output 'vandal.marker.hidden-during-upgrade'
$tierAddLog = Join-Path $output 'add-tier-2.log'
Invoke-Installer @(
    '--silent',
    '--program-root', $program,
    '--game-data-root', $gameData,
    '--resource', $marker,
    '--tier', '2',
    '--no-shortcut',
    '--no-register',
    '--log', $tierAddLog
) $tierAddLog
if (!(Test-Path -LiteralPath (Join-Path $markerInstall.FullName 'payload/texture-packs/2/PACK.json'))) { throw 'Additional 2x pack was not installed.' }
if (!(Test-Path -LiteralPath (Join-Path $markerInstall.FullName 'payload/texture-packs/4/PACK.json'))) { throw 'Adding 2x removed installed 4x pack.' }
if ([IO.File]::ReadAllText($sentinelData) -ne 'keep game data') { throw 'Adding texture tier changed GameData sentinel.' }
if ((Hash-File $marker) -ne $markerHash) { throw 'Marker changed while adding texture tier.' }

$upgradeLog = Join-Path $output 'upgrade.log'
Move-Item -LiteralPath $marker -Destination $markerHidden
Invoke-Installer @(
    '--silent',
    '--program-root', $program,
    '--game-data-root', $gameData,
    '--tier', '4',
    '--no-shortcut',
    '--no-register',
    '--log', $upgradeLog
) $upgradeLog
Move-Item -LiteralPath $markerHidden -Destination $marker
if ([IO.File]::ReadAllText($sentinelData) -ne 'keep game data') { throw 'Upgrade changed GameData sentinel.' }
if ([IO.File]::ReadAllText($sentinelProgram) -ne 'keep unknown program file') { throw 'Upgrade deleted unknown program file.' }
if ((Hash-File $marker) -ne $markerHash) { throw 'Marker changed during upgrade.' }

$ipa = Join-Path $output 'fixture.ipa'
[IO.File]::WriteAllBytes($ipa,(New-FixtureIpa))
$ipaProgram = Join-Path $output 'Program-Ipa'
$ipaGameData = Join-Path $output 'GameData-Ipa'
$ipaLog = Join-Path $output 'install-ipa.log'
Invoke-Installer @(
    '--silent',
    '--program-root', $ipaProgram,
    '--game-data-root', $ipaGameData,
    '--resource', $ipa,
    '--no-shortcut',
    '--no-register',
    '--log', $ipaLog
) $ipaLog
$ipaInstall = Get-ChildItem -LiteralPath (Join-Path $ipaGameData 'marker-content') -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'installation.json') } |
    Select-Object -First 1
if (!$ipaInstall) { throw 'Direct IPA installation record missing.' }
if (!(Test-Path -LiteralPath (Join-Path $ipaInstall.FullName 'payload/DeadSpace.ipa'))) { throw 'Direct IPA payload missing.' }
if (Test-Path -LiteralPath (Join-Path $ipaInstall.FullName 'payload/texture-packs')) { throw 'Direct IPA invented texture packs.' }

$multiLog = Join-Path $output 'install-multiple.log'
Invoke-Installer @('--silent', '--program-root', $program, '--game-data-root', $gameData,
    '--resource', $marker, '--tiers', '2,4,6', '--no-shortcut', '--no-register', '--log', $multiLog) $multiLog
$record = Get-Content -Raw -LiteralPath (Join-Path $markerInstall.FullName 'installation.json') | ConvertFrom-Json
if (($record.texture_tiers -join ',') -ne '2,4,6') { throw 'Multi-selection did not install all tiers.' }

$launcher = Join-Path $program 'DeadSpace Launcher.exe'
for ($mask = 0; $mask -lt 8; $mask++) {
    $selected = @(); for ($bit = 0; $bit -lt 3; $bit++) { if ($mask -band (1 -shl $bit)) { $selected += [string](2 + $bit * 2) } }
    $selection = if ($selected.Count) { $selected -join ',' } else { 'none' }
    $destination = Join-Path $output "selection-$mask"
    $report = Join-Path $output "selection-$mask.json"
    $process = Start-Process -FilePath $launcher -ArgumentList @('--install-resource','--marker',$marker,'--install-root',$destination,'--tiers',$selection,'--result',$report) -PassThru -Wait -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "Selection $selection failed" }
    $content = Join-Path $destination ('marker-content/' + ('a' * 64))
    $record = Get-Content -Raw -LiteralPath (Join-Path $content 'installation.json') | ConvertFrom-Json
    if (($record.texture_tiers -join ',') -ne ($selected -join ',')) { throw "Selection $selection record mismatch" }
    foreach ($tier in @('2','4','6')) {
        $exists = Test-Path -LiteralPath (Join-Path $content "payload/texture-packs/$tier/PACK.json")
        if ($exists -ne ($selected -contains $tier)) { throw "Selection $selection contents mismatch" }
    }
}
$badRoot = Join-Path $output 'invalid-selection'
$process = Start-Process -FilePath $launcher -ArgumentList @('--install-resource','--marker',$marker,'--install-root',$badRoot,'--tiers','4,8') -PassThru -Wait -WindowStyle Hidden
if ($process.ExitCode -eq 0 -or (Test-Path -LiteralPath $badRoot)) { throw 'Invalid tier wrote resource data.' }
if ((Hash-File $marker) -ne $markerHash) { throw 'Marker changed in selection matrix.' }

$result = [ordered]@{
    status = 'passed'
    installer = $installer
    installer_sha256 = Hash-File $installer
    marker_fixture_sha256 = $markerHash
    marker_unchanged = ((Hash-File $marker) -eq $markerHash)
    initial_marker_install_tier_4_only = $true
    marker_tier_2_added_without_removing_4 = $true
    installer_multi_selection_2_4_6 = $true
    all_eight_selection_combinations = $true
    invalid_selection_rejected_before_data_write = $true
    program_installed = $true
    uninstaller_installed = $true
    upgrade_preserved_game_data = $true
    upgrade_preserved_unknown_program_file = $true
    exe_only_upgrade_without_marker = $true
    direct_ipa_installed = $true
    direct_ipa_has_no_texture_packs = $true
    shortcuts_skipped_for_test = $true
    registry_skipped_for_test = $true
}
$result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'RESULT.json') -Encoding utf8
$result | ConvertTo-Json -Depth 5
