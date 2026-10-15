# Test ACTUAL Windows WebView2 rendering after compiling a standalone Tauri
# executable. A successful Tauri compile is insufficient to catch a blank UI.
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS works only for this child process;
# release binaries do not expose a permanent debugging port.
param(
    [string]$Executable = 'src-tauri/target/release/mirroleap.exe'
)
$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$previousArgs = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=19222 --remote-allow-origins=*'
$appProcess = $null
try {
    Write-Host "Starting packaged MirroLeap WebView2 smoke test: $resolved"
    $appProcess = Start-Process -FilePath $resolved -WorkingDirectory (Split-Path -Parent $resolved) -PassThru
    node scripts/windows-smoke-cdp.mjs
    if ($LASTEXITCODE -ne 0) {
        throw 'Packaged MirroLeap did not render its real homepage (WebView2 startup regression).'
    }
} finally {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArgs
    if ($null -ne $appProcess) {
        try {
            if (!$appProcess.HasExited) { Stop-Process -Id $appProcess.Id -Force -ErrorAction Stop }
        } catch {
            Write-Warning "The temporary app process may already have exited: $_"
        }
    }
}
