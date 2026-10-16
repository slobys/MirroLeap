# The NSIS package may have changed bytes when Authenticode signing ran.
# Sign that final exact package with the pre-existing Tauri minisign key.
# An unsigned or differently-signed package is NEVER eligible for auto-update.
$ErrorActionPreference = 'Stop'
$version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
$installer = "src-tauri/target/release/bundle/nsis/MirroLeap_${version}_x64-setup.exe"
$signature = "$installer.sig"
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -or -not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    throw 'Tauri signing secrets missing: updater release must fail closed'
}
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw 'Final signed NSIS installer is missing'
}
if (Test-Path -LiteralPath $signature) { Remove-Item -LiteralPath $signature -Force }
npm run tauri -- signer sign "$installer"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $signature -PathType Leaf)) {
    throw 'Could not sign final NSIS updater executable'
}
$sig = (Get-Content -LiteralPath $signature -Raw).Trim()
if ($sig.Length -lt 80 -or $sig.Length -gt 2000) {
    throw 'Updater signature has an unexpected format'
}
node scripts/verify-updater-signature.mjs "$installer" "$signature"
if ($LASTEXITCODE -ne 0) {
    throw 'The final NSIS installer failed pinned-key signature verification'
}
Write-Host "Verified updater signature artifact exists for MirroLeap v$version"
