param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/release/seek.exe',
    [string]$Output = 'docs/benchmarks/spike.json',
    [string]$Root,
    [string]$Data,
    [int]$IdleSeconds = 20,
    [switch]$Mica,
    [switch]$Foreground,
    [switch]$CloseDuringIndex,
    [switch]$Close
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekIdleWindow {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
$seekPath = (Resolve-Path -LiteralPath $Executable).Path
$seekArgs = @('--spike')
if ($Root) {
    if (-not $Data) { throw 'Shell measurement requires an isolated --Data directory' }
    $seekArgs = @('--root', ('"{0}"' -f (Resolve-Path -LiteralPath $Root).Path), '--data', ('"{0}"' -f [IO.Path]::GetFullPath($Data)))
}
if ($Mica) { $seekArgs += '--mica' }
$seekStart = [Diagnostics.Stopwatch]::StartNew()
$seekBackdropLog = [IO.Path]::GetFullPath($Output + '.glass.log')
$seekOutputDirectory = Split-Path -Parent $Output
if ($seekOutputDirectory) { New-Item -ItemType Directory -Force $seekOutputDirectory | Out-Null }
$seekProcess = Start-Process -FilePath $seekPath -ArgumentList $seekArgs -WindowStyle Normal -RedirectStandardError $seekBackdropLog -PassThru
$seekNativeProcessHandle = $seekProcess.Handle
while ($seekProcess.MainWindowHandle -eq 0 -and $seekStart.Elapsed.TotalSeconds -lt 30) {
    Start-Sleep -Milliseconds 50
    $seekProcess.Refresh()
    if ($seekProcess.HasExited) { throw "Seek exited before opening a window: $($seekProcess.ExitCode)" }
}
if ($seekProcess.MainWindowHandle -eq 0) { throw 'Seek did not open a window within 30 seconds' }
if ($Foreground) { [void][SeekIdleWindow]::SetForegroundWindow($seekProcess.MainWindowHandle) }
$seekInputIdleMs = $seekStart.Elapsed.TotalMilliseconds
if (-not $CloseDuringIndex) { Start-Sleep -Seconds 5 }
$seekProcess.Refresh()
if ($seekProcess.HasExited) { throw "Seek exited early: $($seekProcess.ExitCode)" }
$seekCpuBefore = $seekProcess.TotalProcessorTime.TotalMilliseconds
$seekForegroundBefore = [SeekIdleWindow]::GetForegroundWindow() -eq $seekProcess.MainWindowHandle
$seekWatch = [Diagnostics.Stopwatch]::StartNew()
if (-not $CloseDuringIndex) { Start-Sleep -Seconds $IdleSeconds }
$seekProcess.Refresh()
$seekIdleCpuMs = $seekProcess.TotalProcessorTime.TotalMilliseconds - $seekCpuBefore
$seekIdleWallMs = $seekWatch.Elapsed.TotalMilliseconds
$seekForegroundAfter = [SeekIdleWindow]::GetForegroundWindow() -eq $seekProcess.MainWindowHandle
$seekMemory = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekProcess.Id)"
if (-not $seekMemory) { throw 'Private working set performance counter unavailable' }
$seekRecord = [ordered]@{
    captured_at = (Get-Date).ToUniversalTime().ToString('o')
    executable = $seekPath
    mode = $(if ($Mica) { 'Mica' } else { 'Blurred/Acrylic' })
    pid = $seekProcess.Id
    foreground_at_sample_start = $seekForegroundBefore
    foreground_at_sample_end = $seekForegroundAfter
    window_handle_ms = $seekInputIdleMs
    private_working_set_bytes = [uint64]$seekMemory.WorkingSetPrivate
    working_set_bytes = [uint64]$seekMemory.WorkingSet
    private_commit_bytes = [uint64]$seekMemory.PrivateBytes
    dataset_root = $Root
    data = $Data
    close_during_index_requested = [bool]$CloseDuringIndex
    idle_sample_wall_ms = $seekIdleWallMs
    idle_cpu_ms = $seekIdleCpuMs
    idle_cpu_percent_one_core = 100 * $seekIdleCpuMs / $seekIdleWallMs
    os = (Get-CimInstance Win32_OperatingSystem).Caption
    cpu = (Get-CimInstance Win32_Processor).Name
    logical_processors = [Environment]::ProcessorCount
    close_exited = $null
}
if ($Close -or $CloseDuringIndex) {
    $seekCloseWatch = [Diagnostics.Stopwatch]::StartNew()
    if (-not $seekProcess.CloseMainWindow()) { throw 'Seek has no closeable main window' }
    if (-not $seekProcess.WaitForExit(10000)) { throw 'Seek remained alive 10 seconds after WM_CLOSE' }
    $seekProcess.WaitForExit()
    $seekRecord.close_exited = $true
    $seekRecord.exit_code = $seekProcess.ExitCode
    $seekRecord.close_to_exit_ms = $seekCloseWatch.Elapsed.TotalMilliseconds
}
$seekOutputDirectory = Split-Path -Parent $Output
if ($seekOutputDirectory) { New-Item -ItemType Directory -Force $seekOutputDirectory | Out-Null }
$seekRecord | ConvertTo-Json | Set-Content -LiteralPath $Output -Encoding UTF8
$seekRecord | ConvertTo-Json
