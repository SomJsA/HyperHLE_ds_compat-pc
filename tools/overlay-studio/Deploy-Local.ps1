param([switch]$Open)
$ErrorActionPreference = 'Stop'
$repository = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$project = Split-Path (Split-Path $repository -Parent) -Parent
$destination = Join-Path $project 'tools/overlay-studio'
New-Item -ItemType Directory -Path (Join-Path $destination 'assets') -Force | Out-Null
foreach ($name in @('index.html','studio.css','studio.mjs','layout.mjs','server.py','Start-Studio.ps1','default-style.json','samples.json','brand-spec.md','README.md')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $destination $name) -Force
}
foreach ($name in @('corridor.jpg','deck.jpg','NotoSansSC-Regular.otf','LICENSE.noto')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('assets/' + $name)) -Destination (Join-Path $destination ('assets/' + $name)) -Force
}
if ($Open) { & (Join-Path $destination 'Start-Studio.ps1') }
