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
    $hostSession = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
    Write-Host "Starting packaged MirroLeap WebView2 smoke test: $resolved"
    Write-Host "CI runner session id: $hostSession"
    $appProcess = Start-Process -FilePath $resolved -WorkingDirectory (Split-Path -Parent $resolved) -PassThru
    Start-Sleep -Seconds 4
    $appProcess.Refresh()
    Write-Host "MirroLeap process PID=$($appProcess.Id) Session=$($appProcess.SessionId) Exited=$($appProcess.HasExited)"
    if ($appProcess.HasExited) {
        Write-Warning "MirroLeap exited before WebView2 DOM inspection: $($appProcess.ExitCode)"
    }
    $webviewProcesses = @(Get-Process msedgewebview2 -ErrorAction SilentlyContinue)
    Write-Host "Running WebView2 processes: $($webviewProcesses.Count)"
    $webviewProcesses | Select-Object -First 4 Id, SessionId, ProcessName | Format-Table -AutoSize
    $listening = @(Get-NetTCPConnection -LocalPort 19222 -State Listen -ErrorAction SilentlyContinue)
    Write-Host "WebView2 remote debugging port listeners: $($listening.Count)"
    # Prefer the normal Windows accessibility tree; it works even when the
    # WebView2 runtime refuses Chrome's optional remote debugging flags.
    & pwsh -NoProfile -File scripts/windows-smoke-uia.ps1 -AppProcessId $appProcess.Id
    $uiaExitCode = $LASTEXITCODE
    if ($uiaExitCode -eq 0) {
        Write-Host 'PASS: Installed Windows application visibly exposed its actual controls.'
    } else {
        Write-Warning "UIAutomation result=$uiaExitCode. Checking the optional WebView2 CDP channel."
        $env:MIRROLEAP_SMOKE_TIMEOUT_MS = '25000'
        node scripts/windows-smoke-cdp.mjs
        $cdpExitCode = $LASTEXITCODE
        Remove-Item Env:MIRROLEAP_SMOKE_TIMEOUT_MS -ErrorAction SilentlyContinue
        if ($cdpExitCode -eq 0) {
            Write-Host 'PASS: real WebView2 DOM inspection found the homepage.'
        } else {
        $appProcess.Refresh()
        Write-Host "MirroLeap process at test failure: Exited=$($appProcess.HasExited)"
        $processesAfter = @(Get-Process msedgewebview2 -ErrorAction SilentlyContinue)
        Write-Host "WebView2 processes after failure: $($processesAfter.Count)"
        throw 'Neither Windows accessibility nor WebView2 DOM inspection proved that the MirroLeap homepage rendered.'
        }
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
