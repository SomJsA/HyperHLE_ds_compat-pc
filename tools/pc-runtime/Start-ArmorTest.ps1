param([string]$RuntimeDirectory = $PSScriptRoot, [switch]$PrepareOnly)
$ErrorActionPreference = 'Stop'
$runtime = [IO.Path]::GetFullPath($RuntimeDirectory)
$fixture = Join-Path $runtime 'armor-test-fixture\touchHLE_sandbox'
if (!(Test-Path -LiteralPath (Join-Path $fixture 'com.ea.deadspace.bv\Documents\var\00_CheckpointSave.sb'))) {
    throw '缺少商店前测试档。请从本机验收目录运行本工具。'
}
if (!$PrepareOnly -and (Get-Process -Name touchHLE -ErrorAction SilentlyContinue)) { throw '请先退出当前游戏，再打开独立换甲测试。' }
$runParent = Join-Path $runtime 'armor-test-runs'
foreach ($path in @($runtime, $fixture, $runParent)) {
    if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw '测试目录不能是链接或联接。' }
}
foreach ($item in Get-ChildItem -LiteralPath $fixture -Recurse -Force) {
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw '测试档含有链接，已保留原文件并取消。' }
}
$runRoot = Join-Path $runParent ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N'))
if (Test-Path -LiteralPath $runRoot) { throw '测试目录已存在，取消创建。' }
New-Item -ItemType Directory -Path $runRoot | Out-Null
Copy-Item -LiteralPath $fixture -Destination (Join-Path $runRoot 'touchHLE_sandbox') -Recurse
foreach ($file in Get-ChildItem -LiteralPath $fixture -Recurse -File) {
    $relative = $file.FullName.Substring($fixture.Length).TrimStart('\')
    $copy = Join-Path (Join-Path $runRoot 'touchHLE_sandbox') $relative
    if ((Get-FileHash -LiteralPath $file.FullName).Hash -ne (Get-FileHash -LiteralPath $copy).Hash) { throw '测试档复制校验失败。' }
}
Write-Output ('独立测试目录：' + $runRoot)
if ($PrepareOnly) { return }
foreach ($variable in Get-ChildItem Env: | Where-Object Name -Like 'TOUCHHLE*') { [Environment]::SetEnvironmentVariable($variable.Name, $null, 'Process') }
foreach ($line in [IO.File]::ReadAllLines((Join-Path $runtime 'DeadSpace-VisualCompat.ini'))) {
    if ($line.Trim() -and !$line.TrimStart().StartsWith('#')) {
        $pair = $line.Split('=', 2); [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
    }
}
$settings = @{
    TOUCHHLE_USER_DATA_DIR=$runRoot; TOUCHHLE_DS_NATIVE_INPUTS='1'; TOUCHHLE_DS_NATIVE_HUD='1';
    TOUCHHLE_DS_FRONTEND_GUARD='1'; TOUCHHLE_DS_FRONTEND_NAV='1'; TOUCHHLE_DS_INPUT_MODE='auto';
    TOUCHHLE_DS_MOUSE_LOOK='all'; TOUCHHLE_DS_MOUSE_LOOK_GAIN='1'; TOUCHHLE_DS_MOUSE_LOOK_INVERT='none';
    TOUCHHLE_DS_AIM_XY='360,200'; TOUCHHLE_DS_MOUSE_AIM_MODE='hold'; TOUCHHLE_DS_KEYBOARD_ALT_MODE='hold';
    TOUCHHLE_DS_CONTROLLER_FIRE_MODE='toggle'; TOUCHHLE_DS_CONTROLLER_AIM_MODE='hold';
    TOUCHHLE_DS_KEY_BINDINGS=(Join-Path $runtime 'DeadSpace-Keys.ini'); TOUCHHLE_DS_GAMEPAD_BINDINGS=(Join-Path $runtime 'DeadSpace-Gamepad.ini');
    TOUCHHLE_DS_WINDOW_MODE='window'; TOUCHHLE_DS_OUTPUT_WIDTH='1920'; TOUCHHLE_DS_OUTPUT_HEIGHT='1080';
    TOUCHHLE_DS_MSAA='0'; TOUCHHLE_DS_ANISO='4'; TOUCHHLE_DS_TEX_UPLOAD_FINISH='1';
    TOUCHHLE_DS_OVERLAY_STYLE=(Join-Path $runtime 'overlay-style.json'); TOUCHHLE_DS_CONFIRM_QUIT='1';
    TOUCHHLE_DS_LAUNCHER_PATH=(Join-Path $runtime 'DeadSpace Launcher.exe')
}
foreach ($layer in @('CHAPTER','OBJECTIVE','SUBTITLE','TUTORIAL','QTE')) { $settings['TOUCHHLE_DS_' + $layer + '_OVERLAY']='1' }
foreach ($key in $settings.Keys) { [Environment]::SetEnvironmentVariable($key, $settings[$key], 'Process') }
$start = New-Object Diagnostics.ProcessStartInfo
$start.FileName=Join-Path $runtime 'touchHLE.exe'; $start.WorkingDirectory=$runtime
$start.UseShellExecute=$false; $start.CreateNoWindow=$true
$start.Arguments='"' + (Join-Path $runtime 'DeadSpace.ipa') + '" --gles1=gles1_on_gl2 --force-composition --fast-present --landscape-right --scale-hack=4 --fps-limit=60 --preferred-languages=zh-Hans,zh --no-error-popup'
$process=[Diagnostics.Process]::Start($start)
Write-Output '测试档位于商店前，信用点充足。E 打开商店，购买护甲；每次运行创建新副本。'
$process.WaitForExit()
