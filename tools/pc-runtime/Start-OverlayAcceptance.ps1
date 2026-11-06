$ErrorActionPreference = 'Stop'
$runtime = $PSScriptRoot
$previous = 'E:\CodexProjects\DeadSpaceLegacy\builds\pc-overlay-mouse-20261028'
if (Get-Process touchHLE -ErrorAction SilentlyContinue) { throw '游戏仍在运行，请先从游戏正常退出，再启动此候选。' }
foreach ($dir in @('touchHLE_dylibs','touchHLE_fonts','touchHLE_sandbox')) {
    $dest = Join-Path $runtime $dir
    if (-not (Test-Path -LiteralPath $dest)) { Copy-Item -LiteralPath (Join-Path $previous $dir) -Destination $dest -Recurse }
}
foreach ($file in @('touchHLE_default_options.txt','ds-macros.txt','DeadSpace-Keys.ini','DeadSpace-Gamepad.ini')) {
    $dest = Join-Path $runtime $file
    if (-not (Test-Path -LiteralPath $dest)) { Copy-Item -LiteralPath (Join-Path $previous $file) -Destination $dest }
}
Get-ChildItem Env: | Where-Object Name -like 'TOUCHHLE*' | ForEach-Object { Remove-Item -LiteralPath ('Env:\' + $_.Name) }
$env:TOUCHHLE_DS_DEPTH_SIZE = '24'
$env:TOUCHHLE_DS_MSAA = '4'
$env:TOUCHHLE_DS_MSAA_CLEAR_DEPTH = '1'
$env:TOUCHHLE_DS_NO_CULL_HASH_COUNT = '02d3486c:405'
$env:TOUCHHLE_DS_DEPTH_LEQUAL_HASH = '25a08a82,cee43546,36497762,1d5cccb8,8c12a704,87127d0f,e70efc97,2131afd5,18c949fb,f4d178fa'
$env:TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE = '1'
$env:TOUCHHLE_DS_NO_FOG = '0'
$env:TOUCHHLE_DS_NO_FOG_ORTHO = '1'
$env:TOUCHHLE_DS_NO_FOG_SCREEN = '0'
$env:TOUCHHLE_DS_NO_FOG_HASH = '7027023c,56c62e8a'
$env:TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT = '56c62e8a:6,56c62e8a:36,7027023c:6,7027023c:36'
$env:TOUCHHLE_DS_TEX_UPLOAD_FINISH = '1'
$env:TOUCHHLE_DS_TEXFILTER_LINEAR_HASH = '25a08a82'
$env:TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT = '6'
$env:TOUCHHLE_DS_BLOOD_DEPTH_FIX = '1'
$env:TOUCHHLE_DS_BLOOD_SIZE = '0.5'
$env:TOUCHHLE_DS_AUTO_OPENING_MIPS = '1'
$env:TOUCHHLE_DS_AUTO_MIPS_ALL = '0'
$env:TOUCHHLE_DS_CG_GLYPH_SCALE = '0.68'
$env:TOUCHHLE_DS_CG_GLYPH_DY = '0'
$env:TOUCHHLE_DS_ALPHA_AS_WHITE = '0'
$env:TOUCHHLE_DS_AUTO_DISMISS_ALERTS = '1'
$env:TOUCHHLE_DS_NATIVE_INPUTS = '1'
$env:TOUCHHLE_DS_NATIVE_HUD = '1'
$env:TOUCHHLE_DS_SCREEN_SCALE = '2'
$env:TOUCHHLE_DS_WINDOW_MODE = 'window'
$env:TOUCHHLE_DS_MOUSE_LOOK = 'all'
$env:TOUCHHLE_DS_MOUSE_LOOK_RMB = '0'
$env:TOUCHHLE_DS_AIM_XY = '360,200'
$env:TOUCHHLE_DS_MOUSE_LOOK_INVERT = 'none'
$env:TOUCHHLE_DS_MOUSE_LOOK_GAIN = '1.0'
$env:TOUCHHLE_DS_INPUT_MODE = 'keyboard'
$env:TOUCHHLE_DS_MOUSE_AIM_MODE = 'hold'
$env:TOUCHHLE_DS_CONTROLLER_AIM_MODE = 'hold'
$env:TOUCHHLE_DS_KEY_STICK_GAIN = '2.0'
$env:TOUCHHLE_DS_AIM_WALK = '1'
$env:TOUCHHLE_DS_AIM_FORWARD_LIMIT = '1.0'
$env:TOUCHHLE_DS_RELEASE_F8 = '1'
$env:TOUCHHLE_DS_RELEASE_ALT = '1'
$env:TOUCHHLE_DS_CONFIRM_QUIT = '1'
$env:TOUCHHLE_DS_LOG_LEVEL = 'normal'
$env:TOUCHHLE_DS_INPUT_TRACE = '1'
$env:TOUCHHLE_DS_DUMP_AT_PRESENT = '0'
$env:TOUCHHLE_DS_DUMP_FRAME_EVERY = '0'
$env:TOUCHHLE_USER_DATA_DIR = $runtime
$env:TOUCHHLE_DS_KEY_BINDINGS = Join-Path $runtime 'DeadSpace-Keys.ini'
$env:TOUCHHLE_DS_GAMEPAD_BINDINGS = Join-Path $runtime 'DeadSpace-Gamepad.ini'
$env:TOUCHHLE_DS_MACROS = Join-Path $runtime 'ds-macros.txt'
$env:TOUCHHLE_DS_DUMP_FRAMES = Join-Path $runtime 'frames'
New-Item -ItemType Directory -Path $env:TOUCHHLE_DS_DUMP_FRAMES -Force | Out-Null
$env:TOUCHHLE_DS_CHAPTER_OVERLAY = '1'
$env:TOUCHHLE_DS_SUBTITLE_OVERLAY = '1'
$env:TOUCHHLE_DS_OBJECTIVE_OVERLAY = '1'
$env:TOUCHHLE_DS_TUTORIAL_OVERLAY = '1'
$env:TOUCHHLE_DS_OVERLAY_STYLE = 'E:\CodexProjects\DeadSpaceLegacy\tools\overlay-studio\pc-style.json'
$ipa = 'E:\CodexProjects\DeadSpaceLegacy/run-host/Runtime-PC/touchHLE_apps/DeadSpace-ipad2.ipa'
$gameArgs = @(('"' + $ipa + '"'), '--force-composition', '--landscape-right', '--scale-hack=2', '--preferred-languages=zh-Hans,zh', '--stick-to-touch=30,190,120,110', '--fps-limit=60', '--no-error-popup')
Start-Process -FilePath (Join-Path $runtime 'touchHLE.exe') -WorkingDirectory $runtime -ArgumentList $gameArgs -RedirectStandardOutput (Join-Path $runtime 'play-stdout.log') -RedirectStandardError (Join-Path $runtime 'play-stderr.log') -WindowStyle Normal


