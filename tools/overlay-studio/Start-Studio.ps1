$ErrorActionPreference='Stop'
$listener=Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort 8768 -State Listen -ErrorAction SilentlyContinue
if (-not $listener) {
    $studioPython=(Get-Command python).Source
    Start-Process -FilePath $studioPython -ArgumentList @(('"'+(Join-Path $PSScriptRoot 'server.py')+'"')) -WorkingDirectory $PSScriptRoot -WindowStyle Hidden
}
Start-Process 'http://127.0.0.1:8768'
