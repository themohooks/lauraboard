param([string]$Sdk = $env:ANDROID_HOME, [string]$Ndk = $env:ANDROID_NDK_ROOT)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (!$Sdk) { $Sdk = Join-Path $env:LOCALAPPDATA 'Android/Sdk' }
if (!(Test-Path -LiteralPath $Sdk)) { throw "Android SDK not found: $Sdk" }
if (!(Test-Path (Join-Path $Sdk 'platforms/android-35/android.jar'))) { throw 'Install Android SDK platform 35.' }
if (!$Ndk) {
    $Ndk = Get-ChildItem (Join-Path $Sdk 'ndk') -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' -and [int]($_.Name.Split('.')[0]) -le 28 } |
        Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (!$Ndk) { throw 'Install a stable Android NDK (r27/r28), or pass -Ndk.' }
$bin = Join-Path $Ndk 'toolchains/llvm/prebuilt/windows-x86_64/bin'
if (!(Test-Path (Join-Path $bin 'clang.exe'))) { throw "NDK clang not found: $bin" }
$names = @('ANDROID_HOME', 'ANDROID_NDK_ROOT', 'ANDROID_NDK_HOME', 'ANDROID_NDK', 'ANDROID_PLATFORM', 'JAVA_HOME', 'PATH')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $env:ANDROID_HOME = $Sdk
    $env:ANDROID_NDK_ROOT = $Ndk
    $env:ANDROID_NDK_HOME = $Ndk
    $env:ANDROID_NDK = $Ndk
    $env:ANDROID_PLATFORM = 'android-35'
    if (!$env:JAVA_HOME -or !(Test-Path (Join-Path $env:JAVA_HOME 'bin/javac.exe'))) {
        $javac = (Get-Command javac.exe -ErrorAction Stop).Source
        $env:JAVA_HOME = Split-Path (Split-Path $javac -Parent) -Parent
    }
    $env:PATH = "$bin;$(Join-Path $Sdk 'platform-tools');$env:PATH"
    $toolRoot = Join-Path $repo 'target/android-tools'
    $x = Join-Path $toolRoot 'bin/x.exe'
    if (!(Test-Path -LiteralPath $x)) {
        cargo install --git https://github.com/rust-mobile/xbuild.git --rev e67b501c --root $toolRoot
        if ($LASTEXITCODE) { throw 'xbuild installation failed' }
    }
    Push-Location $repo
    try {
        & $x build --manifest-path crates/lb-android/Cargo.toml --platform android --arch arm64 --format apk --debug 2>&1 |
            Tee-Object -Variable buildOutput
        if ($LASTEXITCODE -and (($buildOutput -join "`n") -notmatch 'failed to locate key id compileSdkVersion')) {
            throw 'Android debug build failed'
        }
        # xbuild's internal resource packer cannot serialize compileSdkVersion
        # against its API 27 resource table. Package with the installed SDK so
        # the final manifest can target 35 and explicitly permit LAN HTTP.
        & (Join-Path $PSScriptRoot 'package-debug.ps1') -Repo $repo -Sdk $Sdk -NdkBin $bin
        $apk = Join-Path $repo 'target/x/debug/android/lb-android.apk'
        if (!(Test-Path -LiteralPath $apk)) { throw "APK not produced: $apk" }
        Write-Host "APK: $apk"
    } finally { Pop-Location }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
