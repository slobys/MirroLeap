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
if ($installers.Count -eq 0) { throw 'Windows installers missing; cannot create verified release' }

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
        foreach ($asset in $installers) {
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
    Write-Host '::warning::Installers are unsigned: no CA code-signing identity configured. Windows SmartScreen can warn. Do not disable protection.'
}

$checksumPath = Join-Path $workspace 'release-checksums'
New-Item -ItemType Directory -Path $checksumPath -Force | Out-Null
$manifest = Join-Path $checksumPath 'SHA256SUMS.txt'
$entries = @(
    foreach ($file in $installers) {
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($file.Name)"
    }
)
if ($entries.Count -ne $installers.Count) { throw 'Could not hash every installer' }
[IO.File]::WriteAllLines($manifest, [string[]]$entries, [Text.UTF8Encoding]::new($false))
Write-Host "SHA256 manifest: $manifest"
Get-Content $manifest
