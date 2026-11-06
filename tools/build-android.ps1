param(
    [ValidateSet('arm64-v8a', 'x86_64')]
    [string]$Abi = 'arm64-v8a',

    [ValidateSet('debug', 'release')]
    [string]$BuildType = 'debug',

    [string]$BundledIpaPath = '',

    [string]$ToolchainRoot = 'C:\dsh\android-toolchain',
    [string]$GradleUserHome = 'C:\dsh\gradle-home',

    [int]$TargetSdk = 24,

    [bool]$FastPresent = $false,

    [int]$ScaleHack = 2,

    [string]$ApplicationIdSuffix = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$androidRoot = Join-Path $repoRoot 'android'
if (-not (Test-Path -LiteralPath (Join-Path $androidRoot 'app\build.gradle'))) {
    throw "Android project not found under $repoRoot"
}

$jdk = Join-Path $ToolchainRoot 'jdk'
$sdk = Join-Path $ToolchainRoot 'sdk'
$ndk = Join-Path $sdk 'ndk\25.2.9519653'
$cmakeBin = Join-Path $ToolchainRoot 'cmake\bin'
$ninjaDir = Join-Path $ToolchainRoot 'ninja'
$gradleBat = Get-ChildItem (Join-Path $ToolchainRoot 'gradle') -Recurse -Filter gradle.bat |
    Where-Object { $_.FullName -match '\\bin\\gradle\.bat$' } |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $gradleBat) {
    throw "gradle.bat not found under $ToolchainRoot\gradle"
}

$env:JAVA_HOME = $jdk
$env:ANDROID_SDK_ROOT = $sdk
$env:ANDROID_HOME = $sdk
$env:ANDROID_NDK_ROOT = $ndk
$env:ANDROID_NDK_HOME = $ndk
$env:GRADLE_USER_HOME = $GradleUserHome
$env:CMAKE = Join-Path $cmakeBin 'cmake.exe'
$env:Path = "$jdk\bin;$sdk\cmdline-tools\latest\cmdline-tools\bin;$sdk\platform-tools;$env:USERPROFILE\.cargo\bin;$cmakeBin;$ninjaDir;$env:Path"

$rustTarget = switch ($Abi) {
    'arm64-v8a' { 'aarch64-linux-android' }
    'x86_64' { 'x86_64-linux-android' }
}
rustup target add $rustTarget

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

$gradleTask = if ($BuildType -eq 'release') { 'assembleRelease' } else { 'assembleDebug' }
Push-Location $androidRoot
try {
    & $gradleBat --no-daemon --console=plain "-PtouchHleAndroidAbi=$Abi" "-PtouchHleTargetSdk=$TargetSdk" "-PtouchHleFastPresent=$FastPresent" "-PtouchHleScaleHack=$ScaleHack" "-PtouchHleApplicationIdSuffix=$ApplicationIdSuffix" $gradleTask
    if ($LASTEXITCODE -ne 0) {
        throw "Gradle failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$outputDirectory = Join-Path $androidRoot "app\build\outputs\apk\$BuildType"
$apk = Join-Path $outputDirectory "app-$BuildType.apk"
if (-not (Test-Path -LiteralPath $apk -PathType Leaf)) {
    throw "Expected APK not found: $apk"
}
$item = Get-Item -LiteralPath $apk
$hash = (Get-FileHash -LiteralPath $apk -Algorithm SHA256).Hash
Write-Output ("APK={0}" -f $item.FullName)
Write-Output ("SIZE={0}" -f $item.Length)
Write-Output ("SHA256={0}" -f $hash)
