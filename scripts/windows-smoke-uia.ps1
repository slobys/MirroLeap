# Windows UIAutomation assertion on the ACTUAL packaged Tauri main window.
# Unlike a DevTools port, UIAutomation does not require WebView2's optional
# remote debugger to be enabled by the browser runtime. Read headings and
# section labels, not screenshots or inaccessible internal implementation.
param([Parameter(Mandatory=$true)][int]$AppProcessId)
$ErrorActionPreference = 'Stop'
try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
} catch {
    Write-Warning "Windows UIAutomation is unavailable in this runner: $_"
    exit 2
}

$last = 'MainWindowHandle not ready'
$found = @()
for ($attempt = 1; $attempt -le 25; $attempt++) {
    try {
        $process = Get-Process -Id $AppProcessId -ErrorAction Stop
        if ($process.HasExited) {
            Write-Warning "MirroLeap exited during accessibility inspection"
            exit 1
        }
        $process.Refresh()
        if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
            $last = 'No visible main window HWND'
        } else {
            $top = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
            if ($null -ne $top) {
                $all = $top.FindAll(
                    [System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.Condition]::TrueCondition
                )
                $names = [System.Collections.Generic.List[string]]::new()
                for ($i = 0; $i -lt $all.Count; $i++) {
                    try {
                        $name = $all.Item($i).Current.Name
                        if (![string]::IsNullOrWhiteSpace($name)) {
                            $names.Add($name.Trim())
                        }
                    } catch {
                        # A WebView2 UIA element can become stale during render.
                    }
                }
                $found = @($names | Select-Object -Unique)
                $summary = $found -join ' | '
                $last = "Found $($all.Count) accessibility nodes: $($summary.Substring(0, [Math]::Min(650, $summary.Length)))"
                $hasDevices = [bool](@($found | Where-Object { $_ -match '(My devices|我的设备)' }).Count)
                $hasMirroring = [bool](@($found | Where-Object { $_ -match '(Screen mirroring|屏幕投屏)' }).Count)
                if ($hasDevices -and $hasMirroring) {
                    Write-Host "PASS: real Windows WebView2 accessibility tree contains MirroLeap device selector AND mirroring panel"
                    Write-Host $last
                    exit 0
                }
            }
        }
    } catch {
        $last = "UIAutomation read failed: $_"
    }
    Start-Sleep -Milliseconds 800
}
Write-Warning "UIAutomation did not find both real homepage panels: $last"
exit 1
