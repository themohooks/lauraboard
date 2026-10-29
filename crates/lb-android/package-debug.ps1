param([Parameter(Mandatory)][string]$Repo, [Parameter(Mandatory)][string]$Sdk, [Parameter(Mandatory)][string]$NdkBin)
$ErrorActionPreference = 'Stop'
$tools = Join-Path $Sdk 'build-tools/35.0.0'
foreach ($tool in @('aapt2.exe', 'zipalign.exe', 'apksigner.bat')) {
    if (!(Test-Path (Join-Path $tools $tool))) { throw 'Install Android SDK Build Tools 35.0.0.' }
}
$native = Join-Path $Repo 'target/x/debug/android/arm64/cargo/aarch64-linux-android/debug/liblb_android.so'
if (!(Test-Path -LiteralPath $native)) { throw 'Build the native Android library first.' }
$stage = Join-Path $Repo 'target/android-package'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
$unsigned = Join-Path $stage 'unsigned.apk'
$aligned = Join-Path $stage 'aligned.apk'
$apk = Join-Path $Repo 'target/x/debug/android/lb-android.apk'
$manifest = Join-Path $PSScriptRoot 'AndroidManifest.xml'
& (Join-Path $tools 'aapt2.exe') link -o $unsigned -I (Join-Path $Sdk 'platforms/android-35/android.jar') --manifest $manifest
if ($LASTEXITCODE) { throw 'Android manifest packaging failed' }

# Like Android Gradle debug packaging, keep full symbols beside the build
# and omit debug sections from the installed library to reduce APK size.
$stripped = Join-Path $stage 'liblb_android.so'
& (Join-Path $NdkBin 'llvm-strip.exe') --strip-debug -o $stripped $native
if ($LASTEXITCODE) { throw 'Stripping packaged debug sections failed' }
$dependencies = & (Join-Path $NdkBin 'llvm-readelf.exe') -d $native
if ($LASTEXITCODE) { throw 'Reading native dependencies failed' }
$needed = @($dependencies | Select-String '\(NEEDED\)' | ForEach-Object {
    if ($_.Line -match '\[([^\]]+)\]') { $Matches[1] }
})
$systemLibraries = @('libandroid.so', 'libdl.so', 'liblog.so', 'libEGL.so', 'libGLESv2.so', 'libm.so', 'libc.so')
$unknown = @($needed | Where-Object { $_ -notin $systemLibraries })
if ($unknown.Count) { throw "Additional runtime libraries need packaging: $($unknown -join ', ')" }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::Open($unsigned, [IO.Compression.ZipArchiveMode]::Update)
try {
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $stripped, 'lib/arm64-v8a/liblb_android.so', [IO.Compression.CompressionLevel]::Optimal) | Out-Null
} finally { $zip.Dispose() }
& (Join-Path $tools 'zipalign.exe') -f 4 $unsigned $aligned
if ($LASTEXITCODE) { throw 'APK alignment failed' }
$keystore = Join-Path $stage 'debug.keystore'
if (!(Test-Path -LiteralPath $keystore)) {
    & (Join-Path $env:JAVA_HOME 'bin/keytool.exe') -genkeypair -keystore $keystore -storepass android -keypass android `
        -alias androiddebugkey -dname 'CN=Android Debug,O=Android,C=US' -keyalg RSA -keysize 2048 -validity 10000
    if ($LASTEXITCODE) { throw 'Debug keystore generation failed' }
}
& (Join-Path $tools 'apksigner.bat') sign --ks $keystore --ks-key-alias androiddebugkey --ks-pass pass:android --key-pass pass:android --out $apk $aligned
if ($LASTEXITCODE) { throw 'APK debug signing failed' }
& (Join-Path $tools 'apksigner.bat') verify --verbose $apk
if ($LASTEXITCODE) { throw 'APK signature verification failed' }
