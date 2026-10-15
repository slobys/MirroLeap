# ScrcpyGUI Windows installer signing (optional; never bypass SmartScreen).
# The workflow always produces SHA256SUMS.txt. It signs with a genuine CA-issued
# code signing PFX only when BOTH GitHub Actions secrets are configured:
#   SCRCPY_SIGNING_PFX_BASE64 / SCRCPY_SIGNING_PFX_PASSWORD
# Most new OV keys are hardware/cloud-backed and cannot be exported as PFX:
# use Tauri's Windows signCommand/Azure Artifact Signing for those providers.
#
# A signature identifies the verified publisher, but cannot guarantee immediate
# SmartScreen reputation for a new release (see Microsoft's developer docs).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$workspace = (Get-Location).Path
$bundle = Join-Path $workspace 'src-tauri\target\release\bundle'
$installers = @(
    Get-ChildItem -Path (Join-Path $bundle 'nsis') -Filter '*.exe' -File -ErrorAction SilentlyContinue
    Get-ChildItem -Path (Join-Path $bundle 'msi') -Filter '*.msi' -File -ErrorAction SilentlyContinue
)
if ($installers.Count -ne 2) { throw "Expected EXE and MSI installers, got $($installers.Count)" }
$portableStage = Join-Path $workspace 'release-portable\ScrcpyGUI-Portable'
$portableExe = Join-Path $portableStage 'scrcpy-gui-v3.exe'
if (-not (Test-Path -LiteralPath $portableExe -PathType Leaf)) {
    throw "Missing standalone portable binary: $portableExe"
}
$signTargets = @($installers) + @(Get-Item -LiteralPath $portableExe)

$pfxBase64 = $env:SCRCPY_SIGNING_PFX_BASE64
$pfxPassword = $env:SCRCPY_SIGNING_PFX_PASSWORD
if ([bool]$pfxBase64 -ne [bool]$pfxPassword) {
    throw 'Signing secrets are incomplete; both PFX base64 and password are required'
}

if ($pfxBase64 -and $pfxPassword) {
    $signers = @(
        Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue |
          Sort-Object -Property FullName -Descending
    )
    if ($signers.Count -eq 0) { throw 'Windows SDK SignTool not found on runner' }
    $signtool = $signers[0].FullName
    $tempCert = Join-Path $env:RUNNER_TEMP 'scrcpygui-release-signing.pfx'
    try {
        [IO.File]::WriteAllBytes($tempCert, [Convert]::FromBase64String($pfxBase64))
        foreach ($asset in $signTargets) {
            # Args are passed as an argument array, not through cmd/string
            # interpolation. GitHub automatically masks matching secrets.
            & $signtool sign /fd SHA256 /f $tempCert /p $pfxPassword /tr 'http://timestamp.digicert.com' /td SHA256 $asset.FullName
            if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed: $($asset.Name)" }
            & $signtool verify /pa /v $asset.FullName
            if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed: $($asset.Name)" }
        }
        Write-Host 'Authenticode: signing and post-signature verification complete'
    } finally {
        if (Test-Path $tempCert) { Remove-Item -Force $tempCert }
    }
} else {
    Write-Host '::warning::Installers and portable EXE are unsigned: no CA code-signing identity configured. Windows SmartScreen can warn. Do not disable protection.'
}

# Compress *after* optional Authenticode signing. The ZIP therefore contains
# the exact binary (signed or unsigned) that was verified by SignTool.
$portableBundle = Join-Path $bundle 'portable'
New-Item -ItemType Directory -Force -Path $portableBundle | Out-Null
$appVersion = (Get-Content -LiteralPath (Join-Path $workspace 'package.json') -Raw | ConvertFrom-Json).version
if ($appVersion -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid package.json version' }
$portableZip = Join-Path $portableBundle "ScrcpyGUI_${appVersion}_x64_Portable.zip"
if (Test-Path -LiteralPath $portableZip) { Remove-Item $portableZip -Force }
Compress-Archive -Path $portableStage -DestinationPath $portableZip -CompressionLevel Optimal
if (-not (Test-Path -LiteralPath $portableZip)) {
    throw 'Standalone portable archive was not created'
}

# Confirm the ZIP has a real EXE, batch launcher, and instructions.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($portableZip)
try {
    $entryNames = @($zip.Entries | ForEach-Object { $_.FullName -replace '\\', '/' })
    foreach ($required in @(
        'ScrcpyGUI-Portable/scrcpy-gui-v3.exe',
        'ScrcpyGUI-Portable/Run-ScrcpyGUI.cmd',
        'ScrcpyGUI-Portable/README-PORTABLE.txt'
    )) {
        if ($entryNames -notcontains $required) {
            throw "Portable archive missing $required"
        }
    }
} finally { $zip.Dispose() }

$checksumPath = Join-Path $workspace 'release-checksums'
New-Item -ItemType Directory -Path $checksumPath -Force | Out-Null
$manifest = Join-Path $checksumPath 'SHA256SUMS.txt'
$releaseFiles = @($installers) + @(Get-Item -LiteralPath $portableZip)
$entries = @(
    foreach ($file in $releaseFiles) {
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($file.Name)"
    }
)
if ($entries.Count -ne 3) { throw 'Could not hash the EXE, MSI and portable ZIP' }
[IO.File]::WriteAllLines($manifest, [string[]]$entries, [Text.UTF8Encoding]::new($false))
Write-Host "SHA256 manifest: $manifest"
Get-Content $manifest
