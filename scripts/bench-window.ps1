param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [int]$Files = 10000,
    [int]$Runs = 30,
    [string]$Output = 'docs/benchmarks/ui.json',
    [switch]$Walker,
    [switch]$Extended,
    [switch]$BroadOnly,
    [switch]$ProfileUi,
    [switch]$NoUiAutomation
)
$ErrorActionPreference = 'Stop'
if ($Runs -lt 30) { throw 'At least 30 measured runs are required' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekBenchWindow {
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
    public static string ForegroundOwner() { uint p;GetWindowThreadProcessId(GetForegroundWindow(),out p);try{return System.Diagnostics.Process.GetProcessById((int)p).ProcessName+" ("+p+")";}catch{return "unknown ("+p+")";} }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
    public static void FocusSearch(IntPtr h) { Rect r;GetClientRect(h,out r);int x=(r.Right-r.Left)/2;int y=(int)(24*GetDpiForWindow(h)/96);var point=new IntPtr((y<<16)|(x&65535));SendMessageW(h,0x201,new UIntPtr(1),point);SendMessageW(h,0x202,UIntPtr.Zero,point); }

    public static void Text(IntPtr h, int c) { if (!PostMessageW(h, 0x102, new UIntPtr((uint)c), IntPtr.Zero)) throw new Exception("WM_CHAR failed"); }
    public static void Backspace(IntPtr h) {
        if (!PostMessageW(h, 0x100, new UIntPtr(8u), new IntPtr(1)) || !PostMessageW(h, 0x101, new UIntPtr(8u), IntPtr.Zero)) throw new Exception("Backspace failed");
    }
}
'@
$seekExe = (Resolve-Path -LiteralPath $Executable).Path
$seekRoot = (Resolve-Path -LiteralPath ".bench-data/$Files/files").Path
$seekData = (Resolve-Path -LiteralPath ".bench-data/$Files/index").Path
$seekTrace = [IO.Path]::GetFullPath(".bench-data/$Files/ui-metrics-$([DateTime]::UtcNow.Ticks).ndjson")
$seekArgs = @('--root', ('"{0}"' -f $seekRoot), '--data', ('"{0}"' -f $seekData), '--metrics', ('"{0}"' -f $seekTrace))
if ($Walker) { $seekArgs += '--walker' }
if ($ProfileUi) { $seekArgs += '--profile-ui' }
$seekLaunchedUnixMs = ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
$seekProcess = Start-Process -FilePath $seekExe -ArgumentList $seekArgs -WindowStyle Normal -PassThru
$seekCreatedUnixMs = ([DateTimeOffset]$seekProcess.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
function Get-SeekRecords {
    if (Test-Path -LiteralPath $seekTrace) {
        $seekStream = [IO.File]::Open($seekTrace, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
        $seekReader = New-Object IO.StreamReader($seekStream)
        try { $seekText = $seekReader.ReadToEnd() } finally { $seekReader.Dispose() }
        foreach ($seekLine in $seekText.Split("`n")) {
            try { $seekLine | ConvertFrom-Json } catch { } # Writer may be completing its final line.
        }
    }
}
function Wait-SeekPaint([string]$Query, [long]$AfterGeneration, [string]$Target) {
    $seekDeadline = [Diagnostics.Stopwatch]::StartNew()
    while ($seekDeadline.Elapsed.TotalSeconds -lt 120) {
        $seekProcess.Refresh()
        if ($seekProcess.HasExited) { throw "Seek exited: $($seekProcess.ExitCode)" }
        $seekMatch = Get-SeekRecords | Where-Object {
            $_.query -eq $Query -and $_.generation -gt $AfterGeneration -and $_.status.ready -and -not $_.status.catching_up -and ($AfterGeneration -le 0 -or ($_.native_input -and $null -ne $_.query_to_present_ms))
        } | Select-Object -Last 1
        $seekValid=$seekMatch -and (-not $Target -or @($seekMatch.rows|Select-Object -First 5) -contains $Target)
        if($seekValid -and $Query.StartsWith("'")){$seekValid=$seekMatch.rows.Count -eq 1}
        if($seekValid -and $Query -eq 'atlas limit:200'){$seekValid=$seekMatch.rows.Count -eq 200 -and @($seekMatch.rows|Where-Object{-not $_.StartsWith('atlas')}).Count -eq 0}
        if($seekValid -and $BroadOnly){$prefix=$Query.Split(' ')[0];$seekValid=$seekMatch.rows.Count -eq 200 -and @($seekMatch.rows|Where-Object{-not $_.StartsWith($prefix)}).Count -eq 0}
        if ($seekValid) { return $seekMatch }
        Start-Sleep -Milliseconds 5 # Benchmark harness only; the app has no polling.
    }
    throw "Timed out awaiting painted result: $Query"
}
try {
    $seekInitial = Wait-SeekPaint '' -1 ''
    $seekCached = Get-SeekRecords | Where-Object { $_.query -eq '' -and $_.status.ready -and $_.rows.Count -gt 0 } | Select-Object -First 1
    $seekProcess.Refresh()
    $seekHwnd = $seekProcess.MainWindowHandle
    $seekWindowWait = [Diagnostics.Stopwatch]::StartNew()
    while ($seekHwnd -eq 0 -and $seekWindowWait.Elapsed.TotalSeconds -lt 30) {
        Start-Sleep -Milliseconds 10
        $seekProcess.Refresh()
        $seekHwnd = $seekProcess.MainWindowHandle
    }
    if ($seekHwnd -eq 0) { throw 'Missing native window' }
    [void][SeekBenchWindow]::SetForegroundWindow($seekHwnd)
    $seekFocusWait = [Diagnostics.Stopwatch]::StartNew()
    while ([SeekBenchWindow]::GetForegroundWindow() -ne $seekHwnd -and $seekFocusWait.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 10 }
    $seekForeground = [SeekBenchWindow]::GetForegroundWindow() -eq $seekHwnd
    if (-not $seekForeground) { throw 'Benchmark window did not become foreground' }
    Start-Sleep -Milliseconds 200 # Allow native activation/focus handlers to finish; outside samples.
    if($NoUiAutomation){[SeekBenchWindow]::FocusSearch($seekHwnd)}else{
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $seekNativeRoot=[Windows.Automation.AutomationElement]::FromHandle($seekHwnd)
    $seekEdit=$seekNativeRoot.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
    if(-not $seekEdit){throw 'Missing search input'}
    $seekEdit.SetFocus()
    }
    Start-Sleep -Milliseconds 100
    Write-Verbose "Benchmark HWND=$seekHwnd PID=$($seekProcess.Id)"
    $seekRows = @()
    $seekPrevious = ''
    $seekCases=@('atlas-notes-00000.txt','orbit-budget-00001.txt','pixel-design-00002.txt')|ForEach-Object{@{query="'$_ limit:500";target=$_}}
    if($Extended){$seekCases+=@(
        @{query='atlas limit:200';target=''},
        @{query='altas-notes-00000 limit:200';target='atlas-notes-00000.txt'},
        @{query='^orbit-budget-00001 limit:200';target='orbit-budget-00001.txt'},
        @{query='pixel-design-00002 ext:txt limit:200';target='pixel-design-00002.txt'}
    )}
    if($BroadOnly){$seekCases=@('atlas','orbit','pixel')|ForEach-Object{@{query=($_+' limit:200');target=''}}}
    foreach ($seekCase in $seekCases) {
        $seekTarget=$seekCase.target
        foreach ($seekChar in $seekPrevious.ToCharArray()) {
            [SeekBenchWindow]::Backspace($seekHwnd)
        }
        $seekQuery = $seekCase.query
        foreach ($seekChar in $seekQuery.ToCharArray()) { [SeekBenchWindow]::Text($seekHwnd, [int]$seekChar) }
        $seekPaint = Wait-SeekPaint $seekQuery 0 $seekTarget
        $seekSamples = @()
        $seekWorkerSamples = @()
        for ($seekRun=0; $seekRun -lt ($Runs+3); $seekRun++) {
            if([SeekBenchWindow]::GetForegroundWindow() -ne $seekHwnd){throw ('Search sample lost foreground focus to '+[SeekBenchWindow]::ForegroundOwner())}
            $seekGeneration = $seekPaint.generation
            [SeekBenchWindow]::Backspace($seekHwnd)
            [SeekBenchWindow]::Text($seekHwnd, 48)
            $seekPaint = Wait-SeekPaint $seekQuery $seekGeneration $seekTarget
            if([SeekBenchWindow]::GetForegroundWindow() -ne $seekHwnd){throw ('Search sample finished without foreground focus: '+[SeekBenchWindow]::ForegroundOwner())}
            if (-not $seekPaint.native_input) { throw 'Sample did not begin at native WM_CHAR receipt' }
            if($NoUiAutomation -and $seekPaint.accessibility_active){throw 'Assistive technology became active; this is not an accessibility-inactive sample'}
            if ($seekRun -ge 3) {
                $seekSamples += $seekPaint.query_to_present_ms
                $seekWork=Get-SeekRecords|Where-Object{$_.interaction -eq 'search_work' -and $_.detail.generation -eq $seekPaint.generation}|Select-Object -Last 1
                if($seekWork){$seekWorkerSamples+=$seekWork.detail.elapsed_ms}
            }
        }
        $seekSorted = @($seekSamples | Sort-Object)
        if ($Runs % 2 -eq 0) { $seekMedian = ($seekSorted[$Runs/2-1]+$seekSorted[$Runs/2])/2 } else { $seekMedian = $seekSorted[[int][Math]::Floor($Runs/2)] }
        $seekRows += [ordered]@{ query=$seekQuery; median_ms=$seekMedian; p95_ms=$seekSorted[[int][Math]::Ceiling($Runs*0.95)-1]; samples_ms=$seekSamples; worker_samples_ms=$seekWorkerSamples }
        $seekPrevious = $seekQuery
    }
    $seekMemory = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekProcess.Id)"
    $seekCloseWatch = [Diagnostics.Stopwatch]::StartNew()
    if (-not $seekProcess.CloseMainWindow() -or -not $seekProcess.WaitForExit(10000)) { throw 'Window close failed to exit process' }
    $seekResult = [ordered]@{
        captured_at=[DateTime]::UtcNow.ToString('o'); executable=$seekExe; sha256=(Get-FileHash -LiteralPath $seekExe -Algorithm SHA256).Hash; files=$Files; no_ui_automation=[bool]$NoUiAutomation; accessibility_active=$seekPaint.accessibility_active; warmups=3; runs=$Runs; foreground_at_start=$seekForeground
        measurement='Native text input receipt (final WM_CHAR) through result application and DirectX Present return; excludes DWM scanout and harness wait'
        process_entry_to_correct_present_ms=$seekInitial.entry_to_present_ms
        process_launch_to_correct_present_ms=$seekInitial.presented_unix_ms-$seekCreatedUnixMs
        process_entry_to_cached_present_ms=$seekCached.entry_to_present_ms
        process_launch_to_cached_present_ms=$seekCached.presented_unix_ms-$seekCreatedUnixMs
        harness_start_to_cached_present_ms=$seekCached.presented_unix_ms-$seekLaunchedUnixMs
        harness_start_to_process_creation_ms=$seekCreatedUnixMs-$seekLaunchedUnixMs
        cached_present_catching_up=$seekCached.status.catching_up
        cached_present_rows=$seekCached.rows.Count
        launch_kind='Windows process creation timestamp; first launch in this measurement; OS file cache not flushed; PowerShell launch setup excluded'
        walker_catch_up_ms=$seekInitial.status.catch_up_ms
        private_working_set_bytes=[uint64]$seekMemory.WorkingSetPrivate; working_set_bytes=[uint64]$seekMemory.WorkingSet; private_commit_bytes=[uint64]$seekMemory.PrivateBytes
        close_to_exit_ms=$seekCloseWatch.Elapsed.TotalMilliseconds; exit_code=$seekProcess.ExitCode; queries=$seekRows
    }
    $seekResult | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Output -Encoding UTF8
    $seekResult | ConvertTo-Json -Depth 8
} finally {
    $seekProcess.Refresh()
    if (-not $seekProcess.HasExited) { [void]$seekProcess.CloseMainWindow(); [void]$seekProcess.WaitForExit(10000) }
}
