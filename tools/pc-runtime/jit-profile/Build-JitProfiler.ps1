param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
$taskRepo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$taskOut=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path -LiteralPath $taskOut){throw 'Output directory must be new'}
$taskVsWhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$taskVs=(& $taskVsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
if($LASTEXITCODE -ne 0 -or !$taskVs){throw 'MSVC x64 tools not found'}
$taskVc=Join-Path $taskVs 'VC/Auxiliary/Build/vcvars64.bat'
if(!(Test-Path -LiteralPath $taskVc)){throw 'vcvars64.bat not found'}
New-Item -ItemType Directory -Path $taskOut | Out-Null
$taskCommands=@(
  '@echo off',
  "call `"$taskVc`" >nul",
  'if errorlevel 1 exit /b 1',
  "cl /nologo /std:c++17 /O2 /EHsc /MD /W4 /Fe:`"$taskOut/PcJitSampler.exe`" /Fo:`"$taskOut/`" `"$PSScriptRoot/PcJitSampler.cpp`"",
  'if errorlevel 1 exit /b 1',
  "cl /nologo /std:c++17 /O2 /EHsc /MD /W4 /I `"$taskRepo/src/cpu/dynarmic_wrapper`" /I `"$taskRepo/vendor/dynarmic/src`" /I `"$taskRepo/vendor/dynarmic/externals/mcl/include`" /Fe:`"$taskOut/TestPcJitMap.exe`" /Fo:`"$taskOut/`" `"$taskRepo/tools/tests/TestPcJitMap.cpp`" `"$taskRepo/vendor/dynarmic/src/dynarmic/backend/x64/perf_map.cpp`"",
  'exit /b %errorlevel%'
)
$taskCmd=Join-Path $taskOut 'build.cmd'
[IO.File]::WriteAllLines($taskCmd,$taskCommands,[Text.UTF8Encoding]::new($false))
$taskProcess=Start-Process -FilePath $env:ComSpec -ArgumentList @('/d','/c',"`"$taskCmd`"") -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskOut 'build.log') -RedirectStandardError (Join-Path $taskOut 'build.err')
if(!$taskProcess.WaitForExit(120000)){throw 'Owned compiler wrapper did not finish within120seconds'}
$taskProcess.Refresh()
Get-Content -LiteralPath (Join-Path $taskOut 'build.log') -Tail 8
Get-Content -LiteralPath (Join-Path $taskOut 'build.err') -Tail 8
if($taskProcess.ExitCode -ne 0){throw "Profiler tools compile failed: $($taskProcess.ExitCode)"}
Write-Output 'Profiler tools compiled'
