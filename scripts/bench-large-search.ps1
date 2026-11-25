# 4.6 large-index search benchmark.
# Types scripted queries into the All locations view of a benchmark build and
# records, per query: keystroke-to-present times, time to the first page, the
# longest UI-thread stall (an external watchdog pings the window with
# SendMessageTimeout(WM_NULL) every 4 ms; a reply that takes N ms means the UI
# message loop was blocked for N ms), and present-to-present frame gaps while
# results stream in.
#
# Usage (from the repository root, after
#   cargo build -p seek --release --features benchmarks):
#   powershell -NoProfile -File scripts/bench-large-search.ps1 -Data <index dir> -Output docs/benchmarks/large-search-<label>.json
# -Data must hold index.bin and roots.json; use a COPY, never a live profile.
param(
    [Parameter(Mandatory = $true)][string]$Data,
    [string]$Executable = 'target\x86_64-pc-windows-msvc\release\seek.exe',
    [string]$Output = 'docs/benchmarks/large-search.json',
    [int]$KeyDelayMs = 110,
    [int]$Repeats = 3,
    [switch]$ScrollOnly
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public static class LargeBench {
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, UIntPtr w, IntPtr l, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public static void Char(IntPtr h, int c) { PostMessageW(h, 0x102, new UIntPtr((uint)c), IntPtr.Zero); }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    // One wheel notch (or several) at the centre of the window's left half,
    // where the file list is.
    public static void Wheel(IntPtr h, int notches) {
        RECT r; GetWindowRect(h, out r);
        int x = r.L + (r.R - r.L) * 2 / 5, y = r.T + (r.B - r.T) / 2;
        PostMessageW(h, 0x020A, new UIntPtr((uint)((-120 * notches) << 16)), new IntPtr((y << 16) | (x & 0xffff)));
    }
    public static void Key(IntPtr h, uint vk) { PostMessageW(h, 0x100, new UIntPtr(vk), new IntPtr(1)); PostMessageW(h, 0x101, new UIntPtr(vk), IntPtr.Zero); }
    // Watchdog: (start ms on the stopwatch, reply latency ms) for every ping.
    static Thread worker; static volatile bool running; static IntPtr target;
    public static List<double[]> Pings = new List<double[]>();
    public static Stopwatch Clock = Stopwatch.StartNew();
    public static void Start(IntPtr h) {
        target = h; running = true; lock (Pings) Pings.Clear();
        worker = new Thread(() => {
            while (running) {
                double at = Clock.Elapsed.TotalMilliseconds;
                UIntPtr r; SendMessageTimeoutW(target, 0, UIntPtr.Zero, IntPtr.Zero, 0x2, 5000, out r);
                double took = Clock.Elapsed.TotalMilliseconds - at;
                lock (Pings) Pings.Add(new double[] { at, took });
                Thread.Sleep(4);
            }
        });
        worker.IsBackground = true; worker.Priority = ThreadPriority.AboveNormal; worker.Start();
    }
    public static double[][] Stop() { running = false; worker.Join(); lock (Pings) return Pings.ToArray(); }
    // The trace arrives over a named pipe, so measuring writes nothing to the
    // indexed drive (a file there would itself cause index updates and redraws).
    public static List<string> Lines = new List<string>();
    public static void Listen(string name) {
        var server = new System.IO.Pipes.NamedPipeServerStream(name, System.IO.Pipes.PipeDirection.In);
        var t = new Thread(() => {
            server.WaitForConnection();
            var reader = new System.IO.StreamReader(server);
            string line;
            while ((line = reader.ReadLine()) != null) lock (Lines) Lines.Add(line);
        });
        t.IsBackground = true; t.Start();
    }
    public static string[] Snapshot(int from) { lock (Lines) { return Lines.GetRange(Math.Min(from, Lines.Count), Lines.Count - Math.Min(from, Lines.Count)).ToArray(); } }
    public static int Count() { lock (Lines) return Lines.Count; }
}
'@
$exe = (Resolve-Path -LiteralPath $Executable).Path
$dataDir = (Resolve-Path -LiteralPath $Data).Path
if ((Resolve-Path "$env:LOCALAPPDATA\Seek" -ErrorAction SilentlyContinue).Path -eq $dataDir) { throw 'Use a copy of the index, not the live profile' }
$pipe = "seek-large-bench-{0}" -f [DateTime]::UtcNow.Ticks
[LargeBench]::Listen($pipe)
$trace = "\\.\pipe\$pipe"
$proc = Start-Process -FilePath $exe -ArgumentList @('--walker', '--data', ('"{0}"' -f $dataDir), '--metrics', ('"{0}"' -f $trace), '--trace-frames') -PassThru
$launchClock = [Diagnostics.Stopwatch]::StartNew()
function Get-Records([int]$From = 0) {
    foreach ($l in [LargeBench]::Snapshot($From)) { if ($l.Trim()) { try { $l | ConvertFrom-Json } catch {} } }
}
try {
    # Wait for the saved index to be searchable. The walker's launch re-check
    # of every folder keeps running (about 30 minutes on a 3.6M-entry C:
    # drive); searching during it is the realistic case, so it is measured.
    while ($true) {
        if ($launchClock.Elapsed.TotalMinutes -gt 5) { throw 'Index never became ready' }
        $ready = Get-Records | Where-Object { $_.status -and $_.status.ready } | Select-Object -Last 1
        if ($ready) { break }
        Start-Sleep -Milliseconds 500
    }
    Start-Sleep -Seconds 10
    $entries = $ready.status.entries
    $proc.Refresh(); $hwnd = $proc.MainWindowHandle
    [void][LargeBench]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 500
    Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes
    $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $edit = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Edit))
    $edit.SetFocus(); Start-Sleep -Milliseconds 300
    # Broad to selective; one that matches nothing; filters; a long one.
    $queries = @('a', 're', 'report', 'program files', 'zqxjv', 'ext:dll', 'windows system32 drivers', 'screenshot 2026')
    if ($ScrollOnly) { $queries = @() }
    $results = @()
    foreach ($q in $queries) {
        for ($rep = 0; $rep -lt $Repeats; $rep++) {
            # Clear the field: as many backspaces as the previous text had.
            for ($i = 0; $i -lt 40; $i++) { [LargeBench]::Key($hwnd, 8) }
            Start-Sleep -Milliseconds 1500
            $before = [LargeBench]::Count()
            [LargeBench]::Start($hwnd)
            $typeStart = [LargeBench]::Clock.Elapsed.TotalMilliseconds
            foreach ($c in $q.ToCharArray()) { [LargeBench]::Char($hwnd, [int]$c); Start-Sleep -Milliseconds $KeyDelayMs }
            $typed = [LargeBench]::Clock.Elapsed.TotalMilliseconds
            # Wait for the final query's first page to be presented.
            $deadline = [Diagnostics.Stopwatch]::StartNew(); $final = $null
            while (-not $final -and $deadline.Elapsed.TotalSeconds -lt 60) {
                $final = Get-Records $before | Where-Object { $_.query -eq $q -and $null -ne $_.query_to_present_ms } | Select-Object -First 1
                if (-not $final) { Start-Sleep -Milliseconds 20 }
            }
            Start-Sleep -Milliseconds 1500  # let streaming and settle finish inside the window
            $pings = [LargeBench]::Stop()
            $records = @(Get-Records $before)
            $presents = @($records | Where-Object { $null -ne $_.frame_entry_ms } | ForEach-Object { [double]$_.frame_entry_ms })
            $gaps = @(); for ($i = 1; $i -lt $presents.Count; $i++) { $gaps += $presents[$i] - $presents[$i - 1] }
            $applied = @($records | Where-Object { $null -ne $_.query_to_present_ms })
            $stalls = @($pings | ForEach-Object { $_[1] })
            $results += [ordered]@{
                query = $q; repeat = $rep
                final_query_to_first_page_ms = if ($final) { [double]$final.query_to_present_ms } else { $null }
                keystroke_to_present_ms = @($applied | ForEach-Object { [math]::Round([double]$_.query_to_present_ms, 2) })
                longest_ui_stall_ms = [math]::Round(($stalls | Measure-Object -Maximum).Maximum, 2)
                ui_stalls_over_16ms = @($stalls | Where-Object { $_ -gt 16.7 }).Count
                ui_stalls_over_50ms = @($stalls | Where-Object { $_ -gt 50 }).Count
                pings = $stalls.Count
                presents = $presents.Count
                longest_present_gap_ms = if ($gaps.Count) { [math]::Round(($gaps | Measure-Object -Maximum).Maximum, 2) } else { $null }
                result_rows = if ($final) { @($final.rows).Count } else { $null }
            }
            Write-Host ("{0,-26} rep {1}: first page {2,8:N1} ms  longest stall {3,7:N1} ms  >50ms {4}" -f $q, $rep, $results[-1].final_query_to_first_page_ms, $results[-1].longest_ui_stall_ms, $results[-1].ui_stalls_over_50ms)
        }
    }
    # Scroll the All locations page quickly, as a person flicking the wheel.
    for ($i = 0; $i -lt 40; $i++) { [LargeBench]::Key($hwnd, 8) }
    Start-Sleep -Milliseconds 2500
    $scrolls = @()
    foreach ($round in 0..2) {
        $before = [LargeBench]::Count()
        [LargeBench]::Start($hwnd)
        for ($i = 0; $i -lt 60; $i++) { [LargeBench]::Wheel($hwnd, 3); Start-Sleep -Milliseconds 16 }
        Start-Sleep -Milliseconds 1500
        $stalls = @([LargeBench]::Stop() | ForEach-Object { $_[1] })
        $presents = @(Get-Records $before | Where-Object { $null -ne $_.frame_entry_ms } | ForEach-Object { [double]$_.frame_entry_ms })
        $gaps = @(); for ($i = 1; $i -lt $presents.Count; $i++) { $gaps += $presents[$i] - $presents[$i - 1] }
        $scrolls += [ordered]@{
            round = $round
            longest_ui_stall_ms = [math]::Round(($stalls | Measure-Object -Maximum).Maximum, 2)
            ui_stalls_over_16ms = @($stalls | Where-Object { $_ -gt 16.7 }).Count
            ui_stalls_over_50ms = @($stalls | Where-Object { $_ -gt 50 }).Count
            presents = $presents.Count
            p95_present_gap_ms = if ($gaps.Count) { [math]::Round((@($gaps | Sort-Object))[[int][math]::Floor($gaps.Count * 0.95)], 2) } else { $null }
        }
        Write-Host ("scroll round {0}: longest stall {1,7:N1} ms  >16ms {2}  >50ms {3}  presents {4}" -f $round, $scrolls[-1].longest_ui_stall_ms, $scrolls[-1].ui_stalls_over_16ms, $scrolls[-1].ui_stalls_over_50ms, $scrolls[-1].presents)
    }
    # Idle after the last search: the window must stop presenting.
    Start-Sleep -Milliseconds 2000
    $idleBefore = @(Get-Records | Where-Object { $null -ne $_.frame_entry_ms }).Count
    Start-Sleep -Milliseconds 5000
    $idleAfter = @(Get-Records | Where-Object { $null -ne $_.frame_entry_ms }).Count
    $cpu = (Get-Process -Id $proc.Id).TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Milliseconds 5000
    $cpuIdle = (Get-Process -Id $proc.Id).TotalProcessorTime.TotalMilliseconds - $cpu
    $mem = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($proc.Id)"
    $summary = [ordered]@{
        captured_at = [DateTime]::UtcNow.ToString('o')
        executable_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        hardware = "$((Get-CimInstance Win32_Processor).Name.Trim()), $([math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)) GB RAM"
        index_entries = $entries
        catching_up_during_run = [bool]$ready.status.catching_up
        key_delay_ms = $KeyDelayMs
        presents_during_5s_idle = $idleAfter - $idleBefore
        cpu_ms_during_5s_idle = [math]::Round($cpuIdle, 1)
        private_working_set_bytes = [uint64]$mem.WorkingSetPrivate
        worst_ui_stall_ms = ($results | ForEach-Object { $_.longest_ui_stall_ms } | Measure-Object -Maximum).Maximum
        worst_scroll_stall_ms = ($scrolls | ForEach-Object { $_.longest_ui_stall_ms } | Measure-Object -Maximum).Maximum
        runs = $results
        scrolling = $scrolls
    }
    $summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Output -Encoding UTF8
    Write-Host ("index {0} entries; worst stall {1} ms; idle presents {2}; idle CPU {3} ms/5s" -f $entries, $summary.worst_ui_stall_ms, $summary.presents_during_5s_idle, $summary.cpu_ms_during_5s_idle)
} finally {
    $proc.Refresh()
    if (-not $proc.HasExited) { [void]$proc.CloseMainWindow(); [void]$proc.WaitForExit(15000) }
}
