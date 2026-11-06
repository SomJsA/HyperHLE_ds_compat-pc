param(
    [Parameter(Mandatory = $true)]
    [string]$JdkHome,

    [Parameter(Mandatory = $true)]
    [string]$AndroidSdk,

    [Parameter(Mandatory = $true)]
    [string]$GradleBat,

    [string]$RustRoot = '',
    [string]$GradleUserHome = '',
    [ValidateSet('debug', 'release')]
    [string]$BuildType = 'release',
    [ValidateSet('arm64-v8a', 'x86_64')]
    [string]$Abi = 'arm64-v8a',
    [int]$TargetSdk = 24,
    [int]$ScaleHack = 2,
    [bool]$FastPresent = $false,
    [string]$ApplicationIdSuffix = '',
    [string]$BundledIpaPath = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$androidRoot = Join-Path $repoRoot 'android'
$ndk = Join-Path $AndroidSdk 'ndk\25.2.9519653'
$cmakeBin = Join-Path $AndroidSdk 'cmake\3.31.6\bin'
$ninja = Join-Path $cmakeBin 'ninja.exe'

foreach ($required in @(
    (Join-Path $JdkHome 'bin\java.exe'),
    (Join-Path $AndroidSdk 'platforms\android-35\android.jar'),
    (Join-Path $ndk 'build\cmake\android.toolchain.cmake'),
    (Join-Path $cmakeBin 'cmake.exe'),
    $ninja,
    $GradleBat
)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required build tool not found: $required"
    }
}

$env:JAVA_HOME = $JdkHome
$env:ANDROID_SDK_ROOT = $AndroidSdk
$env:ANDROID_HOME = $AndroidSdk
$env:ANDROID_NDK_ROOT = $ndk
$env:ANDROID_NDK_HOME = $ndk
$env:CMAKE = Join-Path $cmakeBin 'cmake.exe'
if ($GradleUserHome) {
    $env:GRADLE_USER_HOME = $GradleUserHome
}
if ($RustRoot) {
    $env:RUSTUP_HOME = Join-Path $RustRoot '.rustup'
    $env:CARGO_HOME = Join-Path $RustRoot '.cargo'
    $env:Path = "$(Join-Path $RustRoot '.cargo\bin');$env:Path"
}
$env:Path = "$(Join-Path $JdkHome 'bin');$cmakeBin;$env:Path"

$bundledAssetDirectory = Join-Path $androidRoot 'app\src\main\assets\touchHLE_bundled'
$bundledAssetIpa = Join-Path $bundledAssetDirectory 'DeadSpace.ipa'
$bundledAssetId = Join-Path $bundledAssetDirectory 'bundle.id'
if ($BundledIpaPath) {
    if (-not (Test-Path -LiteralPath $BundledIpaPath -PathType Leaf)) {
        throw "Bundled IPA not found: $BundledIpaPath"
    }
    New-Item -ItemType Directory -Path $bundledAssetDirectory -Force | Out-Null
    Copy-Item -LiteralPath $BundledIpaPath -Destination $bundledAssetIpa -Force
    $bundleHash = (Get-FileHash -LiteralPath $bundledAssetIpa -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath $bundledAssetId -Value $bundleHash -Encoding ASCII -NoNewline
} elseif (Test-Path -LiteralPath $bundledAssetIpa) {
    throw "A staged IPA already exists at $bundledAssetIpa; pass -BundledIpaPath to confirm it"
}

$task = if ($BuildType -eq 'release') { 'assembleRelease' } else { 'assembleDebug' }
Push-Location $androidRoot
try {
    & $GradleBat --no-daemon --console=plain `
        "-PtouchHleAndroidAbi=$Abi" `
        "-PtouchHleTargetSdk=$TargetSdk" `
        "-PtouchHleFastPresent=$FastPresent" `
        "-PtouchHleScaleHack=$ScaleHack" `
        "-PtouchHleApplicationIdSuffix=$ApplicationIdSuffix" `
        $task
    if ($LASTEXITCODE -ne 0) {
        throw "Gradle failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$apk = Join-Path $androidRoot "app\build\outputs\apk\$BuildType\app-$BuildType.apk"
if (-not (Test-Path -LiteralPath $apk)) {
    throw "APK not found: $apk"
}
$item = Get-Item -LiteralPath $apk
$hash = (Get-FileHash -LiteralPath $apk -Algorithm SHA256).Hash
Write-Output "APK=$($item.FullName)"
Write-Output "SIZE=$($item.Length)"
Write-Output "SHA256=$hash"
