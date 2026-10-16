# Create GitHub Releases/latest.json from the actual, post-Authenticode Tauri
# signed Windows NSIS installer. All fields are derived from this build; none
# can direct the application to arbitrary URLs or untrusted release assets.
$ErrorActionPreference = 'Stop'
$version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
if ($version -cnotmatch '^\d+\.\d+\.\d+$') { throw 'Invalid stable version' }
$filename = "MirroLeap_${version}_x64-setup.exe"
$installer = Join-Path "src-tauri/target/release/bundle/nsis" $filename
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw "Cannot generate signed updater metadata: $filename missing"
}
$signaturePath = "$installer.sig"
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
    throw 'Tauri updater signature for NSIS is missing'
}
$signature = (Get-Content -LiteralPath $signaturePath -Raw).Trim()
if ($signature.Length -lt 60 -or $signature.Length -gt 1024) {
    throw 'Tauri updater signature is empty or malformed'
}
# No checksum replaces an Ed25519 signature. Its public key is embedded in the
# installed application and cannot be overridden from latest.json.
$release = [ordered]@{
    version = $version
    notes = 'MirroLeap 更新：投屏快捷控制与签名验证的应用内更新。'
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = @{
        'windows-x86_64' = @{
            signature = $signature
            url = "https://github.com/slobys/MirroLeap/releases/download/v$version/$filename"
        }
    }
}
$folder = 'release-checksums'
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$path = Join-Path $folder 'latest.json'
[IO.File]::WriteAllText(
    (Join-Path (Get-Location).Path $path),
    ($release | ConvertTo-Json -Depth 10),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "Signed updater feed ready for Windows x64: $path"
