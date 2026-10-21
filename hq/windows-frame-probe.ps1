# MirroLeap experimental scrcpy HQ IPC probe (manual Windows diagnostic).
# Copyright (c) 2026 MirroLeap contributors (MIT).
# Requires the isolated, v4.0-pinned patched scrcpy.exe built by HQ CI.
# NEVER enables HQ in the normal MirroLeap app or writes video to disk.
param(
    [string]$PatchedScrcpy = "",
    [string]$Serial = "",
    [int]$Seconds = 15,
    [switch]$SelfTest
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$capacity = [long]24883424  # 64 + 2*(80 + 3840*2160*3/2)
$slotBytes = [long]12441600
$slotHeader = [long]80
$globalHeader = [long]64
$noFrame = [uint32]::MaxValue

function Set-U32($access, [long]$offset, [uint32]$value) {
    $access.Write($offset, $value)
}
function Get-LatestValidFrame($access) {
    # All live fields are read twice. If the producer overwrites a slot,
    # the sample is skipped instead of accepting a partially written frame.
    $publication = $access.ReadUInt32(24)
    $slot = $access.ReadUInt32(20)
    if ($slot -eq [uint32]::MaxValue -or $publication -eq 0) {return $null}
    if ($slot -ge 2) {return $null}
    $offset = $globalHeader + [long]$slot * ($slotHeader + $slotBytes)
    $before = $access.ReadUInt32($offset)
    if ($before -eq 0 -or ($before -band 1) -ne 0) {return $null}
    $format = $access.ReadUInt32($offset + 4)
    $width = $access.ReadUInt32($offset + 8)
    $height = $access.ReadUInt32($offset + 12)
    $pixels = [long]$width * [long]$height
    if (($format -ne 1 -and $format -ne 2) -or
        $width -lt 2 -or $height -lt 2 -or $width -gt 3840 -or
        $height -gt 3840 -or ($width % 2) -ne 0 -or
        ($height % 2) -ne 0 -or $pixels -gt 8294400) {return $null}
    $payload = $access.ReadUInt32($offset + 60)
    if ($payload -ne ($pixels * 3 / 2) -or $payload -gt $slotBytes) {return $null}
    $pts = $access.ReadInt64($offset + 16)
    if ($before -ne $access.ReadUInt32($offset) -or
        $slot -ne $access.ReadUInt32(20) -or
        $publication -ne $access.ReadUInt32(24)) {return $null}
    return [pscustomobject]@{
        Generation = $publication; Slot = $slot; Width = $width;
        Height = $height; Format = $format; Pts = $pts;
    }
}

$map = $null
$access = $null
$process = $null
$generator = [Security.Cryptography.RandomNumberGenerator]::Create()
try {
    [byte[]]$nonceBytes = New-Object byte[] 16
    $generator.GetBytes($nonceBytes)
    $nonce = -join ($nonceBytes | ForEach-Object { $_.ToString('x2') })
    # The secret name is never sent over the network or echoed in output.
    $map = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateNew(
        "Local\MirroLeapHQ_$nonce", $capacity,
        [IO.MemoryMappedFiles.MemoryMappedFileAccess]::ReadWrite
    )
    $access = $map.CreateViewAccessor(0, $capacity,
        [IO.MemoryMappedFiles.MemoryMappedFileAccess]::ReadWrite)

    [byte[]]$magic = 0x4d, 0x4c, 0x48, 0x51
    $access.WriteArray([long]0, $magic, 0, 4)
    Set-U32 $access 4 ([uint32]1)
    Set-U32 $access 8 ([uint32]$capacity)
    Set-U32 $access 12 ([uint32]$slotBytes)
    Set-U32 $access 16 ([uint32]2)
    Set-U32 $access 20 $noFrame

    if ($SelfTest) {
        # Simulate two producer publications with the SAME slot-local
        # sequence. Reading only the slot sequence would miss frame 2.
        for ($slot=0;$slot -lt 2;$slot++) {
            $off=$globalHeader + [long]$slot * ($slotHeader + $slotBytes)
            Set-U32 $access $off ([uint32]2)
            Set-U32 $access ($off + 4) ([uint32]2)  # NV12
            Set-U32 $access ($off + 8) ([uint32]4)
            Set-U32 $access ($off + 12) ([uint32]4)
            $access.Write([long]($off + 16), [long]($slot+123))
            Set-U32 $access ($off + 60) ([uint32]24)
            Set-U32 $access 20 ([uint32]$slot)
            Set-U32 $access 24 ([uint32]($slot+1))
            $frame = Get-LatestValidFrame $access
            if (-not $frame -or $frame.Generation -ne $slot+1 -or
                $frame.Width -ne 4 -or $frame.Pts -ne $slot+123) {
                throw "HQ Windows IPC probe self-test failed on slot $slot"
            }
        }
        Write-Output "PASS HQ Windows probe: isolated mapping, two-slot publication, PTS, frame geometry"
        return
    }

    if ([string]::IsNullOrWhiteSpace($PatchedScrcpy)) {
        throw "Specify -PatchedScrcpy with the TEST-ONLY scrcpy v4.0 executable."
    }
    if ($Seconds -lt 3 -or $Seconds -gt 120) {
        throw "Probe interval must be 3..120 seconds."
    }
    if ($Serial -ne '' -and $Serial -notmatch '^[a-zA-Z0-9_.:-]{1,80}$') {
        throw "Unsupported Android serial input."
    }
    $resolved = (Resolve-Path -LiteralPath $PatchedScrcpy -ErrorAction Stop).Path
    if (-not [IO.File]::Exists($resolved) -or
        [IO.Path]::GetFileName($resolved) -ne 'scrcpy.exe') {
        throw "This diagnostic requires an explicit patched scrcpy.exe file."
    }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $resolved
    $info.WorkingDirectory = [IO.Path]::GetDirectoryName($resolved)
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $false
    $info.Arguments = if ($Serial -ne '') {
        '--serial ' + $Serial
    } else { '' }
    $info.EnvironmentVariables['MIRROLEAP_HQ_TOKEN'] = $nonce
    $process = [Diagnostics.Process]::Start($info)
    if (-not $process) {throw "Patched scrcpy could not be launched."}
    # Scrcpy may print ordinary ADB device logs; no secret names are logged.
    Write-Output "HQ session started; reading only frame metadata for $Seconds seconds."
    $start = [DateTime]::UtcNow
    $count=0
    $lastPublication=[uint32]0
    $firstUpdate=$null
    $lastUpdate=$null
    $lastSize=''
    while (([DateTime]::UtcNow-$start).TotalSeconds -lt $Seconds -and
           -not $process.HasExited) {
        Start-Sleep -Milliseconds 90
        $frame=Get-LatestValidFrame $access
        if ($null -eq $frame -or $frame.Generation -eq $lastPublication) {continue}
        $count++
        $lastPublication=$frame.Generation
        $lastSize="$($frame.Width)x$($frame.Height)"
        $lastUpdate=[DateTime]::UtcNow
        if (-not $firstUpdate) {$firstUpdate=$lastUpdate}
    }
    if ($count -eq 0) {
        Write-Warning "No decoded HQ frames: patched scrcpy, mapping or Android connection may be unavailable. Standard MirroLeap remains unchanged."
        exit 2
    }
    $elapsed=[Math]::Max(0.001,($lastUpdate-$firstUpdate).TotalSeconds)
    $fps=if ($count -gt 1) {($count-1)/$elapsed} else {0}
    Write-Output ("HQ producer verified: {0} samples, last source {1}, observed update rate {2:N1}fps. No pixels stored." -f
                  $count, $lastSize, $fps)
} finally {
    if ($process) {
        try {
            if (-not $process.HasExited) {
                [void]$process.CloseMainWindow()
                if (-not $process.WaitForExit(3500)) {$process.Kill()}
            }
        } catch {}
        $process.Dispose()
    }
    if ($access) {$access.Dispose()}
    if ($map) {$map.Dispose()}
    $generator.Dispose()
}
