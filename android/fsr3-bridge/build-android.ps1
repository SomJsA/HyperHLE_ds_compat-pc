param(
    [Parameter(Mandatory = $true)]
    [string]$Fsr3SdkRoot,
    [Parameter(Mandatory = $true)]
    [string]$NdkRoot,
    [string]$VulkanHeaders = "",
    [string]$Fsr3ShaderCompiler = "",
    [switch]$UsePrebuiltFfxLibraries,
    [switch]$SkipCopy
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$sourceDir = (Resolve-Path $PSScriptRoot).Path
$buildDir = Join-Path $repoRoot "target\fsr3-bridge-arm64"
$toolchain = Join-Path $NdkRoot "build\cmake\android.toolchain.cmake"

if (-not (Test-Path -LiteralPath $toolchain)) {
    throw "Android NDK toolchain not found: $toolchain"
}
if (-not (Test-Path -LiteralPath $Fsr3SdkRoot)) {
    throw "FSR3 SDK root not found: $Fsr3SdkRoot"
}
if (-not $UsePrebuiltFfxLibraries -and
    ($Fsr3ShaderCompiler -eq "" -or -not (Test-Path -LiteralPath $Fsr3ShaderCompiler))) {
    throw "Fsr3ShaderCompiler is required when rebuilding FidelityFX static libraries"
}

$cmakeArgs = @(
    "-S", $sourceDir,
    "-B", $buildDir,
    "-G", "Ninja",
    "-DCMAKE_TOOLCHAIN_FILE=$toolchain",
    "-DANDROID_ABI=arm64-v8a",
    "-DANDROID_PLATFORM=31",
    "-DCMAKE_BUILD_TYPE=Release",
    "-DFSR3_SDK_ROOT=$Fsr3SdkRoot",
    "-DFSR3_USE_PREBUILT=$($UsePrebuiltFfxLibraries.IsPresent.ToString().ToUpperInvariant())"
)
if ($VulkanHeaders -ne "") {
    $cmakeArgs += "-DFSR3_VULKAN_HEADERS=$VulkanHeaders"
}
if ($Fsr3ShaderCompiler -ne "") {
    $cmakeArgs += "-DFSR3_SC_EXECUTABLE=$Fsr3ShaderCompiler"
}

& cmake @cmakeArgs
if ($LASTEXITCODE -ne 0) {
    throw "CMake configure failed with exit code $LASTEXITCODE"
}
& cmake --build $buildDir --config Release --target dsfsr3bridge
if ($LASTEXITCODE -ne 0) {
    throw "CMake build failed with exit code $LASTEXITCODE"
}

$builtLibrary = Get-ChildItem -LiteralPath $buildDir -Recurse -File -Filter "libdsfsr3bridge.so" |
    Select-Object -First 1
if ($null -eq $builtLibrary) {
    throw "libdsfsr3bridge.so was not produced"
}

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $builtLibrary.FullName).Hash
Write-Host "Built $($builtLibrary.FullName)"
Write-Host "SHA-256 $hash"

if (-not $SkipCopy) {
    $jniDir = Join-Path $repoRoot "android\app\src\main\jniLibs\arm64-v8a"
    New-Item -ItemType Directory -Force -Path $jniDir | Out-Null
    Copy-Item -LiteralPath $builtLibrary.FullName `
        -Destination (Join-Path $jniDir "libdsfsr3bridge.so") -Force
    Write-Host "Copied to $jniDir"
}
