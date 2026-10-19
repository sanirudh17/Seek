param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/release/seek.exe',
    [int]$Files = 10000,
    [int]$Seconds = 30,
    [string]$Output = 'docs/benchmarks/stage4-quiet-idle.json'
)
$ErrorActionPreference = 'Stop'
if ($Seconds -lt 20 -or $Seconds -gt 60) { throw 'Use a bounded 20-60 second sample.' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekQuietWindow {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
    public static uint Owner(IntPtr h) { uint p; GetWindowThreadProcessId(h,out p);return p; }
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
    public static void NeutralPointer(IntPtr h) { Rect r; if(GetWindowRect(h,out r)) SetCursorPos(r.Right-24,r.Bottom-42); }
}
'@
$exe = (Resolve-Path -LiteralPath $Executable).Path
$root = (Resolve-Path -LiteralPath ".bench-data/$Files/files").Path
$data = (Resolve-Path -LiteralPath ".bench-data/$Files/index").Path
# Outside the watched files directory; this trace cannot generate its own events.
$trace = [IO.Path]::GetFullPath(".bench-data/$Files/quiet-$([DateTime]::UtcNow.Ticks).ndjson")
function Records {
    if (-not (Test-Path -LiteralPath $trace)) { return }
    $stream = [IO.File]::Open($trace, 'Open', 'Read', 'ReadWrite')
    $reader = [IO.StreamReader]::new($stream)
    try { $content = $reader.ReadToEnd() } finally { $reader.Dispose() }
    foreach ($line in $content.Split("`n")) { if ($line.Trim()) { try { $line | ConvertFrom-Json } catch {} } }
}
function Counters {
    $rows = @(Records)
    $app.Refresh()
    if ($app.HasExited) { throw 'Seek exited during sampling.' }
    $latest = $rows | Where-Object { $null -ne $_.status } | Select-Object -Last 1
    $foregroundWindow=[SeekQuietWindow]::GetForegroundWindow()
    $foregroundOwner=[SeekQuietWindow]::Owner($foregroundWindow)
    $foregroundName=try{[Diagnostics.Process]::GetProcessById($foregroundOwner).ProcessName}catch{'unknown'}
    @{
        elapsed_ms = $watch.Elapsed.TotalMilliseconds
        cpu_ms = $app.TotalProcessorTime.TotalMilliseconds
        foreground = $foregroundWindow -eq $hwnd
        foreground_owner_pid = $foregroundOwner
        foreground_owner_name = $foregroundName
        frames = @($rows | Where-Object { $null -ne $_.frame_entry_ms }).Count
        activity = @($rows | Where-Object { $null -ne $_.native_activity }).Count
        applied = @($rows | Where-Object { $null -ne $_.status }).Count
        generation = $latest.generation
    }
}
$intervals = [Collections.Generic.List[object]]::new()
$segments = [Collections.Generic.List[object]]::new()
$app = Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"'),'--trace-frames') -WindowStyle Normal -PassThru
$retainedHandle = $app.Handle
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(120)
    do {
        $app.Refresh()
        if ($app.HasExited) { throw 'Seek exited before ready.' }
        $ready = @(Records) | Where-Object { $_.status.ready -and -not $_.status.catching_up } | Select-Object -Last 1
        if (-not $ready) { Start-Sleep -Milliseconds 20 }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Seek did not become ready.' }
    } while (-not $ready)
    $hwnd = $app.MainWindowHandle
    if ($hwnd -eq 0) { throw 'No visible Seek window.' }
    # Let one-time asynchronous shell artwork finish before the fixed sample.
    Start-Sleep -Seconds 12
    [void][SeekQuietWindow]::SetForegroundWindow($hwnd)
    [SeekQuietWindow]::NeutralPointer($hwnd)
    Start-Sleep -Milliseconds 500
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $previous = Counters
    $segment = $null
    while ($watch.Elapsed.TotalSeconds -lt $Seconds) {
        Start-Sleep -Milliseconds 250 # Harness sampling only; the app has no polling timer.
        $current = Counters
        $quiet = $previous.activity -eq $current.activity -and $previous.applied -eq $current.applied -and $previous.generation -eq $current.generation -and $previous.foreground -eq $current.foreground
        $kind = if (-not $quiet) { 'activity-or-focus-change' } elseif ($current.foreground) { 'quiet-foreground' } else { 'quiet-background' }
        $item = [ordered]@{
            start_ms=$previous.elapsed_ms; wall_ms=$current.elapsed_ms-$previous.elapsed_ms
            kind=$kind; cpu_ms=$current.cpu_ms-$previous.cpu_ms
            frames=$current.frames-$previous.frames; native_activity=$current.activity-$previous.activity
            applied_updates=$current.applied-$previous.applied
            foreground_owner_pid=$current.foreground_owner_pid;foreground_owner_name=$current.foreground_owner_name
        }
        $intervals.Add($item)
        if ($null -eq $segment -or $segment.kind -ne $kind) {
            $segment = [ordered]@{kind=$kind;start_ms=$item.start_ms;wall_ms=0.0;cpu_ms=0.0;frames=0}
            $segments.Add($segment)
        }
        $segment.wall_ms += $item.wall_ms; $segment.cpu_ms += $item.cpu_ms; $segment.frames += $item.frames
        $previous = $current
    }
    $memory = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($app.Id)"
    $close = [Diagnostics.Stopwatch]::StartNew()
    if (-not $app.CloseMainWindow() -or -not $app.WaitForExit(10000)) { throw 'Close did not exit.' }
    $app.WaitForExit()
    $foreground = $segments | Where-Object { $_.kind -eq 'quiet-foreground' } | Sort-Object {[double]$_.wall_ms} -Descending | Select-Object -First 1
    $background = $segments | Where-Object { $_.kind -eq 'quiet-background' } | Sort-Object {[double]$_.wall_ms} -Descending | Select-Object -First 1
    $passed = $foreground -and $foreground.wall_ms -ge 20000 -and $foreground.cpu_ms -eq 0 -and $foreground.frames -eq 0
    $report = [ordered]@{
        utc=[DateTime]::UtcNow.ToString('o');executable=$exe;sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        files=$Files;seconds=$Seconds;settle_seconds=12;trace=$trace
        method='250 ms harness intervals qualified only by unchanged native activity, applied query/status updates and foreground identity. CPU and presentations are outcomes, never exclusion criteria. Focus is requested once, never locked; raw intervals are retained. Windows process CPU counter resolution applies; this is a saved fixture with no writes.'
        longest_quiet_foreground=$foreground;longest_quiet_background=$background
        sustained_foreground_idle_passed=[bool]$passed
        private_working_set_bytes=[uint64]$memory.WorkingSetPrivate;private_commit_bytes=[uint64]$memory.PrivateBytes
        close_ms=$close.Elapsed.TotalMilliseconds;exit_code=$app.ExitCode;segments=$segments;intervals=$intervals
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Output -Encoding UTF8
    [PSCustomObject]$report | Select-Object sustained_foreground_idle_passed,longest_quiet_foreground,longest_quiet_background,exit_code | ConvertTo-Json -Depth 5
    if ($app.ExitCode -ne 0) { throw 'Seek close returned failure.' }
    if (-not $passed) { throw 'No qualifying 20-second quiet foreground segment; inspect the retained raw intervals.' }
} finally {
    $app.Refresh()
    if (-not $app.HasExited) { [void]$app.CloseMainWindow(); [void]$app.WaitForExit(10000) }
}
