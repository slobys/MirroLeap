param(
    [Parameter(Mandatory)][string]$UsbMirrorSource,
    [Parameter(Mandatory)][string]$UsbMirrorExe,
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $UsbMirrorSource).Path
$upstreamExe = (Resolve-Path -LiteralPath $UsbMirrorExe).Path
if ((git -C $source rev-parse HEAD) -ne 'dccb2bcd29f672d0c7b6af9997bde7b60c9ab025' -or
    (git -C $source status --porcelain --untracked-files=no)) { throw 'USB Mirror source revision mismatch.' }
if ((Get-FileHash -LiteralPath $upstreamExe).Hash -ne '82DBDBF17FB0BBBCFEC1598E19B1DD758D7475596E3385E7D44B7E24FB90EEDB') {
    throw 'USB Mirror 0.4.1 executable checksum mismatch.'
}
$stage = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ('MirroLeap-USB-' + [guid]::NewGuid().ToString('N')))
$report = Join-Path $stage.FullName 'bundle.json'
$extractor = Start-Process -FilePath $upstreamExe -ArgumentList @('--bundle-check', ('"{0}"' -f $report)) -WindowStyle Hidden -PassThru -Wait
if ($extractor.ExitCode -ne 0) { throw 'USB Mirror runtime extraction failed.' }
$bundle = Get-Content -Raw -LiteralPath $report | ConvertFrom-Json
if (!$bundle.passed -or $bundle.applicationStarted -or $bundle.driverChangesPerformed) { throw 'Unexpected extraction report.' }
$payload = New-Item -ItemType Directory -Path (Join-Path $stage.FullName 'payload')
foreach ($name in @('iPhoneMirror.Core.dll','iPhoneMirror.UsbConfigurationSwitch.exe','libusb-1.0.dll','libusb0.dll','USBMirror.Driver.exe','LICENSE','licenses','SOURCE.md')) {
    Copy-Item -LiteralPath (Join-Path $bundle.cacheDirectory $name) -Destination $payload.FullName -Recurse
}
$coreHeaders = Join-Path $source 'third_party/capture-engine/src/Core/include/iPhoneMirror'
$usbHeaders = Join-Path $source 'src'
$preview = Join-Path $payload.FullName 'MirroLeapUsbPreview.exe'
$object = Join-Path $stage.FullName 'preview.obj'
$compiler = if (Get-Command clang-cl.exe -ErrorAction SilentlyContinue) { 'clang-cl.exe' } else { 'cl.exe' }
$captureSource = Join-Path $source 'third_party/capture-engine'
& python (Join-Path $PSScriptRoot 'build-ios-capture-core.py') $captureSource (Join-Path $payload.FullName 'iPhoneMirror.Core.dll') --compiler $compiler
if ($LASTEXITCODE -ne 0) { throw 'USB capture core did not compile or pass recovery tests.' }
$libusbSource = Join-Path $source 'third_party/capture-engine/third_party/libusb-win32/source/src'
& python (Join-Path $PSScriptRoot 'build-ios-libusb-read.py') $payload.FullName $libusbSource --compiler $compiler
if ($LASTEXITCODE -ne 0) { throw 'USB bulk read adapter did not compile.' }
$compilerArgs = @('/nologo','/std:c++17','/O2','/MT','/EHsc','/utf-8','/DUNICODE','/D_UNICODE','/DNOMINMAX','/DWIN32_LEAN_AND_MEAN',
    "/I$coreHeaders", "/I$usbHeaders", "/Fo$object", "/Fe$preview", (Join-Path $PSScriptRoot 'ios-usb-preview.cpp'),
    '/link','/SUBSYSTEM:WINDOWS','user32.lib','shell32.lib','gdi32.lib','ole32.lib','setupapi.lib','cfgmgr32.lib','advapi32.lib')
if ($compiler -eq 'clang-cl.exe') { $compilerArgs = @('-fuse-ld=lld') + $compilerArgs }
& $compiler @compilerArgs
if ($LASTEXITCODE -ne 0) { throw 'USB preview did not compile.' }
$test = Start-Process -FilePath $preview -ArgumentList '--self-test-usb' -WindowStyle Hidden -PassThru -Wait `
    -RedirectStandardOutput (Join-Path $stage.FullName 'self-test.txt')
if ($test.ExitCode -ne 0) { throw 'USB readiness tests failed.' }
Get-Content -LiteralPath (Join-Path $stage.FullName 'self-test.txt')
$probe = Start-Process -FilePath $preview -ArgumentList '--probe' -WindowStyle Hidden -PassThru -Wait `
    -RedirectStandardOutput (Join-Path $stage.FullName 'probe.txt')
if ($probe.ExitCode -ne 0) { throw 'USB preview could not load the capture runtime.' }
Get-Content -LiteralPath (Join-Path $stage.FullName 'probe.txt')
$sourceFile = Join-Path $payload.FullName 'SOURCE.md'
$upstreamNotice = Get-Content -Raw -LiteralPath $sourceFile
$localNotice = @'
# MirroLeap USB runtime

This package rebuilds iPhoneMirror.Core.dll from the iPhoneMirror source
vendored in USBMirror commit dccb2bcd29f672d0c7b6af9997bde7b60c9ab025.
The accompanying licenses/ios-quicktime-video-recovery.patch records the
changes. The packaged libusb0.dll is MirroLeap's bulk-read adapter, whose
source is in licenses/ios-libusb-read.c. The original user-mode libusb
library is packaged as libusb0_original.dll.

The following source notice describes the upstream USBMirror baseline.
'@
Set-Content -LiteralPath $sourceFile -Value ($localNotice + "`r`n`r`n" + $upstreamNotice) -Encoding utf8
foreach ($name in @('ios-quicktime-video-recovery.patch','ios-libusb-read.c')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $payload.FullName 'licenses')
}
Compress-Archive -Path (Join-Path $payload.FullName '*') -DestinationPath $Output -Force
Get-Item -LiteralPath $Output | Select-Object FullName,Length
