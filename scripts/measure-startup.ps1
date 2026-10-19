param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/release/seek.exe',
    [int]$Files = 10000,
    [int]$Runs = 5,
    [int]$IdleSeconds = 20,
    [string]$Output = 'docs/benchmarks/startup-profile.json',
    [switch]$Walker,
    [switch]$ColdFileCache,
    [string]$CacheTool
)
$ErrorActionPreference = 'Stop'
if ($Runs -lt 1 -or $IdleSeconds -lt 0 -or $IdleSeconds -gt 60) { throw 'Invalid sample count or idle interval' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekStartupWindow {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    public static void PointerNeutral(IntPtr h) { Rect r; if(GetWindowRect(h,out r)) SetCursorPos(r.Right-24,Math.Max(r.Top+50,r.Bottom-42)); }
    public static uint Owner(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
}
'@
$seekExe = (Resolve-Path -LiteralPath $Executable).Path
$seekRoot = (Resolve-Path -LiteralPath ".bench-data/$Files/files").Path
$seekData = (Resolve-Path -LiteralPath ".bench-data/$Files/index").Path
$seekSamples = @()
$seekPurgeReceipt=$null
if($ColdFileCache){
 if($Runs -ne 1 -or -not $CacheTool){throw 'A cold file-cache run requires -Runs 1 and a verified -CacheTool'}
 if(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Launch this harness as your ordinary user, not administrator'}
 if(Get-Process -Name seek -ErrorAction SilentlyContinue){throw 'Close Seek before the cold file-cache run'}
 $seekCacheTool=(Resolve-Path -LiteralPath $CacheTool).Path
 $seekPurgeHelper=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot 'purge-cold-cache.ps1')).Path
 $seekPurgeReceipt=Join-Path $env:TEMP ('seek-cache-purge-'+[guid]::NewGuid().ToString('N')+'.json')
}
function Read-SeekTrace([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $seekStream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $seekReader = New-Object IO.StreamReader($seekStream)
    try { $seekText = $seekReader.ReadToEnd() } finally { $seekReader.Dispose() }
    foreach ($seekLine in $seekText.Split("`n")) {
        if ($seekLine.Trim()) { try { $seekLine | ConvertFrom-Json } catch {} }
    }
}
for ($seekRun = 0; $seekRun -lt $Runs; $seekRun++) {
    $seekTrace = [IO.Path]::GetFullPath(".bench-data/$Files/startup-$([DateTime]::UtcNow.Ticks).ndjson")
    $seekArgs = @('--root', ('"{0}"' -f $seekRoot), '--data', ('"{0}"' -f $seekData), '--metrics', ('"{0}"' -f $seekTrace), '--trace-frames')
    if ($Walker) { $seekArgs += '--walker' }
    if($ColdFileCache){
        $purgeArgs=@('-NoProfile','-ExecutionPolicy','Bypass','-File',('"'+$seekPurgeHelper+'"'),'-Tool',('"'+$seekCacheTool+'"'),'-Receipt',('"'+$seekPurgeReceipt+'"'))
        $purge=Start-Process -FilePath "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe" -Verb RunAs -WindowStyle Hidden -ArgumentList $purgeArgs -PassThru
        if(-not $purge.WaitForExit(60000)){throw 'Cache-purge diagnostic did not finish'}
        if($purge.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $seekPurgeReceipt)){throw 'Cache purge failed; do not label this sample cold'}
    }
    $seekLaunchedUnixMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $seekProcess = Start-Process -FilePath $seekExe -ArgumentList $seekArgs -WindowStyle Normal -PassThru
    $seekRetainedHandle = $seekProcess.Handle
    $seekCreatedUnixMs = ([DateTimeOffset]$seekProcess.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
    try {
        $seekWait = [Diagnostics.Stopwatch]::StartNew()
        do {
            $seekProcess.Refresh()
            if ($seekProcess.HasExited) { throw "Seek exited before results: $($seekProcess.ExitCode)" }
            $seekRecords = @(Read-SeekTrace $seekTrace)
            $seekCached = $seekRecords | Where-Object { $_.status.ready -and $_.rows.Count -gt 0 } | Select-Object -First 1
            $seekReady = $seekRecords | Where-Object { $_.status.ready -and -not $_.status.catching_up -and $_.rows.Count -gt 0 } | Select-Object -Last 1
            if (-not $seekReady) { Start-Sleep -Milliseconds 5 }
        } while (-not $seekReady -and $seekWait.Elapsed.TotalSeconds -lt 120)
        if (-not $seekReady -or -not $seekCached) { throw 'Timed out awaiting presented results' }
        $seekProcess.Refresh()
        $seekHwnd = $seekProcess.MainWindowHandle
        $seekWindowWait = [Diagnostics.Stopwatch]::StartNew()
        while ($seekHwnd -eq 0 -and $seekWindowWait.Elapsed.TotalSeconds -lt 30) {
            Start-Sleep -Milliseconds 10
            $seekProcess.Refresh()
            $seekHwnd = $seekProcess.MainWindowHandle
        }
        if (-not [SeekStartupWindow]::IsWindowVisible($seekHwnd) -or [SeekStartupWindow]::Owner($seekHwnd) -ne $seekProcess.Id) { throw 'Missing visible Seek window' }
        $seekSample = [ordered]@{
            run = $seekRun + 1
            trace = $seekTrace
            harness_start_to_cached_present_ms = $seekCached.presented_unix_ms - $seekLaunchedUnixMs
            process_launch_to_cached_present_ms = $seekCached.presented_unix_ms - $seekCreatedUnixMs
            process_entry_to_cached_present_ms = $seekCached.entry_to_present_ms
            cached_present_catching_up = $seekCached.status.catching_up
            cached_present_rows = $seekCached.rows.Count
            process_launch_to_up_to_date_present_ms = $seekReady.presented_unix_ms - $seekCreatedUnixMs
            harness_start_to_process_creation_ms = $seekCreatedUnixMs - $seekLaunchedUnixMs
            walker_catch_up_ms = $seekReady.status.catch_up_ms
            milestones = @($seekRecords | Where-Object { $_.phase })
            foreground_idle = $null
        }
        if ($IdleSeconds -gt 0 -and $seekRun -eq $Runs - 1) {
            [void][SeekStartupWindow]::SetForegroundWindow($seekHwnd)
            [SeekStartupWindow]::PointerNeutral($seekHwnd)
            $seekFocusWait = [Diagnostics.Stopwatch]::StartNew()
            while ([SeekStartupWindow]::GetForegroundWindow() -ne $seekHwnd -and $seekFocusWait.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 10 }
            if ([SeekStartupWindow]::GetForegroundWindow() -ne $seekHwnd) { throw 'Seek did not become foreground; no idle claim can be made' }
            Start-Sleep -Seconds 2 # Settle activation; benchmark harness only.
            [void][SeekStartupWindow]::SetForegroundWindow($seekHwnd)
            $seekIdleForegroundStart = [SeekStartupWindow]::GetForegroundWindow() -eq $seekHwnd
            if (-not $seekIdleForegroundStart) { throw 'Seek lost foreground before the idle sample' }
            $seekProcess.Refresh()
            $seekIdleRecordsBefore = @(Read-SeekTrace $seekTrace)
            $seekFrameStart = @($seekIdleRecordsBefore | Where-Object { $null -ne $_.frame_entry_ms }).Count
            $seekGenerationBefore = ($seekIdleRecordsBefore | Where-Object { $_.status } | Select-Object -Last 1).generation
            $seekActivityBefore = @($seekIdleRecordsBefore | Where-Object { $null -ne $_.native_activity }).Count
            if ($seekFrameStart -eq 0) { throw 'Presentation trace unavailable; rebuild with the current benchmarks feature' }
            $seekForegroundSamples = 0
            $seekForegroundLost = 0
            $seekOtherForeground = @{}
            $seekProcess.Refresh()
            $seekCpu = $seekProcess.TotalProcessorTime.TotalMilliseconds
            $seekIdleWatch = [Diagnostics.Stopwatch]::StartNew()
            while ($seekIdleWatch.Elapsed.TotalSeconds -lt $IdleSeconds) {
                $seekForegroundSamples++
                $activeWindow=[SeekStartupWindow]::GetForegroundWindow()
                if ($activeWindow -ne $seekHwnd) {
                    $seekForegroundLost++
                    $ownerId=[SeekStartupWindow]::Owner($activeWindow)
                    $ownerName=try{[Diagnostics.Process]::GetProcessById($ownerId).ProcessName}catch{'unknown'}
                    if(-not $seekOtherForeground.ContainsKey($ownerName)){$seekOtherForeground[$ownerName]=0}
                    $seekOtherForeground[$ownerName]++
                }
                Start-Sleep -Milliseconds 250 # Test harness monitors focus; app does not poll.
            }
            $seekProcess.Refresh()
            $seekWallMs = $seekIdleWatch.Elapsed.TotalMilliseconds
            $seekCpuMs = $seekProcess.TotalProcessorTime.TotalMilliseconds - $seekCpu
            $seekIdleForegroundEnd = [SeekStartupWindow]::GetForegroundWindow() -eq $seekHwnd
            $seekIdleRecordsAfter = @(Read-SeekTrace $seekTrace)
            $seekFrameEnd = @($seekIdleRecordsAfter | Where-Object { $null -ne $_.frame_entry_ms }).Count
            $seekGenerationAfter = ($seekIdleRecordsAfter | Where-Object { $_.status } | Select-Object -Last 1).generation
            $seekActivityAfter = @($seekIdleRecordsAfter | Where-Object { $null -ne $_.native_activity }).Count
            $seekMemory = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekProcess.Id)"
            $seekSample.foreground_idle = [ordered]@{
                wall_ms = $seekWallMs; cpu_ms = $seekCpuMs
                foreground_at_start = $seekIdleForegroundStart
                foreground_checks = $seekForegroundSamples; foreground_lost_checks = $seekForegroundLost
                foreground_at_end = $seekIdleForegroundEnd
                other_foreground_samples = $seekOtherForeground
                presentations_during_idle = $seekFrameEnd - $seekFrameStart
                query_generation_at_start = $seekGenerationBefore
                query_generation_at_end = $seekGenerationAfter
                foreground_and_query_unchanged = ($seekIdleForegroundStart -and $seekIdleForegroundEnd -and $seekForegroundLost -eq 0 -and $seekGenerationBefore -eq $seekGenerationAfter)
                native_activity_messages = $seekActivityAfter - $seekActivityBefore
                quiet_foreground_sample = ($seekIdleForegroundStart -and $seekIdleForegroundEnd -and $seekForegroundLost -eq 0 -and $seekGenerationBefore -eq $seekGenerationAfter -and $seekActivityBefore -eq $seekActivityAfter)
                input_attribution = 'Native keyboard, character/IME, mouse, non-client mouse, pointer and activation/focus message ids; no text payloads. Redraw count is an outcome, not a reason to discard a quiet sample.'
                private_working_set_bytes = [uint64]$seekMemory.WorkingSetPrivate
                working_set_bytes = [uint64]$seekMemory.WorkingSet
                private_commit_bytes = [uint64]$seekMemory.PrivateBytes
            }
        }
        $seekClose = [Diagnostics.Stopwatch]::StartNew()
        if (-not $seekProcess.CloseMainWindow() -or -not $seekProcess.WaitForExit(10000)) { throw 'Closing did not exit Seek' }
        $seekProcess.WaitForExit()
        $seekSample.close_to_exit_ms = $seekClose.Elapsed.TotalMilliseconds
        $seekSample.exit_code = $seekProcess.ExitCode
        $seekSamples += $seekSample
    } finally {
        $seekProcess.Refresh()
        if (-not $seekProcess.HasExited) { [void]$seekProcess.CloseMainWindow(); [void]$seekProcess.WaitForExit(10000) }
    }
}
$seekReport = [ordered]@{
    captured_at = [DateTime]::UtcNow.ToString('o')
    files = $Files; runs = $Runs; executable = $seekExe; sha256 = (Get-FileHash -LiteralPath $seekExe -Algorithm SHA256).Hash
    cold_file_cache = [bool]$ColdFileCache
    purge = $(if($seekPurgeReceipt){Get-Content -LiteralPath $seekPurgeReceipt -Raw|ConvertFrom-Json}else{$null})
    launch_kind = 'Windows process creation timestamp through successful DirectX Present return; see cold_file_cache and purge receipt for cache state; excludes PowerShell launch setup and physical DWM scanout'
    idle_method = 'Final run only; foreground checked every 250 ms by harness; app trace records every presentation; wait for up-to-date before sampling'
    samples = $seekSamples
}
$seekDirectory = Split-Path -Parent $Output
if ($seekDirectory) { New-Item -ItemType Directory -Force -Path $seekDirectory | Out-Null }
$seekReport | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Output -Encoding UTF8
$seekReport | ConvertTo-Json -Depth 10
if($IdleSeconds -gt 0 -and -not $seekSamples[-1].foreground_idle.quiet_foreground_sample){throw 'Idle sample had native activity or lost foreground focus; no quiet-idle claim is valid.'}
