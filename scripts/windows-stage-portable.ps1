# Stage a standalone MirroLeap Windows executable, with NO NSIS or MSI
# registration. The archive is produced after optional Authenticode signing.
$ErrorActionPreference = 'Stop'
$workspace = (Get-Location).Path
$exePath = Join-Path $workspace 'src-tauri\target\release\mirroleap.exe'
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "Standalone Rust application missing: $exePath"
}
if ((Get-Item $exePath).Length -lt 1024 * 1024) {
    throw 'Standalone MirroLeap executable unexpectedly small'
}
$staging = Join-Path $workspace 'release-portable\MirroLeap-Portable'
if (Test-Path -LiteralPath $staging) { Remove-Item $staging -Recurse -Force }
New-Item -Path $staging -ItemType Directory -Force | Out-Null
Copy-Item -LiteralPath $exePath -Destination (Join-Path $staging 'mirroleap.exe')

# Batch launcher: no elevation, registry updates, system writes or auto-uninstall.
$launcher = '@echo off' + [Environment]::NewLine +
    'start "" "%~dp0mirroleap.exe"' + [Environment]::NewLine
[IO.File]::WriteAllText((Join-Path $staging 'Run-MirroLeap.cmd'),
    $launcher, [Text.ASCIIEncoding]::new())

$guide = @'
MirroLeap Portable for Windows x64
==================================

This is an EXTRACT-AND-RUN edition. It does NOT use NSIS/MSI and does not
invoke the previous ScrcpyGUI / MirroLeap uninstaller, which may be damaged on some PCs.

1. Exit all previous MirroLeap and scrcpy projection windows.
2. Extract the *whole* ZIP to a user-writable folder, e.g.
   D:\Apps\MirroLeap-Portable\  or  %USERPROFILE%\Apps\MirroLeap-Portable\
   Do not run files directly from inside the ZIP.
3. Double-click Run-MirroLeap.cmd or mirroleap.exe.
4. If scrcpy is not yet found, use the app's core downloader, or place a
   trusted scrcpy-win64 package in a sibling folder named "scrcpy-bin":
      MirroLeap-Portable\
        mirroleap.exe
        scrcpy-bin\
          scrcpy.exe
          adb.exe
          scrcpy-server
          *.dll
   The GUI needs both "adb.exe" and "scrcpy.exe" to mirror Android devices.

Your old installation is NOT removed or overwritten. Keep its scrcpy-bin and
settings backed up before making changes. If a broken old uninstall entry is
left in Windows Settings, consult Microsoft's Program Install and Uninstall
troubleshooting guidance rather than manually deleting registry keys:
https://support.microsoft.com/en-us/windows/uninstall-or-remove-apps-and-programs-in-windows-4b55f974-2cc6-2d2b-d092-5905080eaf98

Check SHA256SUMS.txt from the GitHub Release before opening the portable ZIP.
The ZIP is not code-signed simply by including hashes; Windows may still show
a SmartScreen warning until a trusted publisher certificate is configured.
Do not bypass Windows security controls for a file from an untrusted source.

Requires Windows 10/11 x64 and the Microsoft WebView2 Runtime.
'@
[IO.File]::WriteAllText((Join-Path $staging 'README-PORTABLE.txt'),
    $guide, [Text.UTF8Encoding]::new($false))

$files = @(Get-ChildItem -LiteralPath $staging -File)
if ($files.Count -ne 3) {
    throw "Unexpected portable package contents: $($files.Count) files"
}
Write-Host "Prepared portable standalone EXE: $staging"
Get-ChildItem -LiteralPath $staging | Format-Table -AutoSize
