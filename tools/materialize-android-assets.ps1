param(
    [string]$Repo = 'C:\dsh\dsbuild2'
)
$ErrorActionPreference = 'Stop'
$repoFull = [IO.Path]::GetFullPath($Repo)
$app = Join-Path $repoFull 'android\app\src\main'
if (-not (Test-Path -LiteralPath $app)) {
    throw "Android app source missing under $repoFull"
}

$backup = Join-Path $env:TEMP ('android-symlink-placeholders-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $backup | Out-Null

function Replace-WithCopy([string]$target, [string]$source, [switch]$Directory) {
    if (Test-Path -LiteralPath $target) {
        $rel = $target.Substring($repoFull.Length + 1)
        $bak = Join-Path $backup ($rel -replace '[\\/:*?"<>|]', '_')
        Move-Item -LiteralPath $target -Destination $bak -Force
    }
    if ($Directory) {
        Copy-Item -LiteralPath $source -Destination $target -Recurse -Force
    } else {
        Copy-Item -LiteralPath $source -Destination $target -Force
    }
}

Replace-WithCopy `
    (Join-Path $app 'java\org\libsdl\app') `
    (Join-Path $repoFull 'vendor\SDL\android-project\app\src\main\java\org\libsdl\app') `
    -Directory

Replace-WithCopy `
    (Join-Path $app 'assets\touchHLE_default_options.txt') `
    (Join-Path $repoFull 'touchHLE_default_options.txt')

Replace-WithCopy `
    (Join-Path $app 'assets\touchHLE_dylibs') `
    (Join-Path $repoFull 'touchHLE_dylibs') `
    -Directory

Replace-WithCopy `
    (Join-Path $app 'assets\touchHLE_fonts') `
    (Join-Path $repoFull 'touchHLE_fonts') `
    -Directory

foreach ($name in 'icon.png','icon_preview.png','icon_unofficial.png') {
    Replace-WithCopy `
        (Join-Path $app "res\drawable-nodpi\$name") `
        (Join-Path $repoFull "res\$name")
}

"materialized Android assets from $repoFull"
"placeholder backup: $backup"
