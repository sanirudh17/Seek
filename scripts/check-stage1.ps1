param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/debug/seek.exe',
    [string]$Cli = 'target/x86_64-pc-windows-msvc/debug/seek-cli.exe',
    [switch]$ShellActions,
    [string]$Output = 'docs/benchmarks/stage1-focused-check.json'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekStageCheck {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, UIntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public static Rect Bounds(IntPtr h) { Rect r; if (!GetWindowRect(h, out r)) throw new Exception("GetWindowRect failed"); return r; }
    public static void Key(IntPtr h, uint code) {
        if (!PostMessageW(h, 0x100, new UIntPtr(code), new IntPtr(1)) || !PostMessageW(h, 0x101, new UIntPtr(code), new IntPtr(unchecked((int)0xc0000001)))) throw new Exception("Key delivery failed");
    }
}
'@
$seekExe = (Resolve-Path -LiteralPath $Executable).Path
$seekCli = (Resolve-Path -LiteralPath $Cli).Path
$seekFixture = [IO.Path]::GetFullPath(".bench-data/stage1-check-$([DateTime]::UtcNow.Ticks)")
$seekRoot = Join-Path $seekFixture 'files'
$seekData = Join-Path $seekFixture 'index'
New-Item -ItemType Directory -Path (Join-Path $seekRoot 'subfolder') -Force | Out-Null
foreach ($seekName in @('invoice-alpha.txt', 'invoice-beta.txt', 'invoice-picture.png')) {
    [IO.File]::WriteAllText((Join-Path $seekRoot $seekName), 'Seek test fixture; filename search only.')
}
[IO.File]::WriteAllText((Join-Path $seekRoot 'subfolder/closed-before.txt'), 'Closed lifecycle fixture.')
$seekChecks = @()
$seekVerify = @()
$seekExits = @()
$seekShellChecks = @()
$seekProcess = $null
$seekTrace = ''
function Read-SeekCheckTrace {
    if (-not (Test-Path -LiteralPath $script:seekTrace)) { return }
    $seekStream = [IO.File]::Open($script:seekTrace, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $seekReader = New-Object IO.StreamReader($seekStream)
    try { $seekText = $seekReader.ReadToEnd() } finally { $seekReader.Dispose() }
    foreach ($seekLine in $seekText.Split("`n")) { try { $seekLine | ConvertFrom-Json } catch {} }
}
function Wait-SeekCheck([scriptblock]$Predicate, [double]$After = -1) {
    $seekWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $script:seekProcess.Refresh()
        if ($script:seekProcess.HasExited) { throw 'Seek exited during check' }
        $seekMatch = Read-SeekCheckTrace | Where-Object { $_.entry_ms -gt $After -or $_.entry_to_present_ms -gt $After } | Where-Object $Predicate | Select-Object -Last 1
        if ($seekMatch) { return $seekMatch }
        Start-Sleep -Milliseconds 20 # Harness only; app remains event-driven.
    } while ($seekWait.Elapsed.TotalSeconds -lt 20)
    throw 'Timed out waiting for a Stage 1 assertion'
}
function Start-SeekCheck([int]$Run) {
    $script:seekTrace = Join-Path $seekFixture "trace-$Run.ndjson"
    $script:seekProcess = Start-Process -FilePath $seekExe -ArgumentList @('--root', ('"{0}"' -f $seekRoot), '--data', ('"{0}"' -f $seekData), '--metrics', ('"{0}"' -f $script:seekTrace)) -WindowStyle Normal -PassThru
    $script:seekRetainedHandle = $script:seekProcess.Handle
    $seekReady = Wait-SeekCheck { $_.status.ready -and -not $_.status.catching_up }
    $script:seekProcess.Refresh()
    $script:seekHwnd = $script:seekProcess.MainWindowHandle
    if ($script:seekHwnd -eq 0) { throw 'Missing window handle' }
    return $seekReady
}
function Set-SeekCheckQuery([string]$Query) {
    $seekCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
    $seekProviderWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $seekWindow = [System.Windows.Automation.AutomationElement]::FromHandle($script:seekHwnd)
        $seekEdit = $seekWindow.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $seekCondition)
        if (-not $seekEdit) { Start-Sleep -Milliseconds 50 }
    } while (-not $seekEdit -and $seekProviderWait.Elapsed.TotalSeconds -lt 5)
    if (-not $seekEdit) { throw 'Search edit missing' }
    $seekValue = $seekEdit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $seekValue.SetValue($Query)
    $seekReadback = [Diagnostics.Stopwatch]::StartNew()
    while ($seekValue.Current.Value -ne $Query -and $seekReadback.Elapsed.TotalSeconds -lt 5) { Start-Sleep -Milliseconds 20 }
    if ($seekValue.Current.Value -ne $Query) { throw 'Query readback mismatch' }
}
function Invoke-SeekCheckButton([string]$Name) {
    $seekWindow = [System.Windows.Automation.AutomationElement]::FromHandle($script:seekHwnd)
    $seekCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Name)
    $seekButton = $seekWindow.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $seekCondition)
    if (-not $seekButton) { throw "Missing button: $Name" }
    $seekButton.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Close-SeekCheck {
    $seekWatch = [Diagnostics.Stopwatch]::StartNew()
    if (-not $script:seekProcess.CloseMainWindow() -or -not $script:seekProcess.WaitForExit(10000)) { throw 'Window close did not exit process' }
    $script:seekProcess.WaitForExit()
    return [ordered]@{ close_ms=$seekWatch.Elapsed.TotalMilliseconds; exit_code=$script:seekProcess.ExitCode }
}
function Verify-SeekCheck([string]$Phase) {
    $seekOutput = Join-Path $seekFixture "verify-$Phase.json"
    & $seekCli verify --root $seekRoot --data $seekData --output $seekOutput | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Independent verify failed: $Phase" }
    $seekResult = Get-Content -LiteralPath $seekOutput -Raw -Encoding UTF8 | ConvertFrom-Json
    return [ordered]@{ phase=$Phase; report=$seekResult.report; catch_up_ms=$seekResult.catch_up_ms }
}
function Find-SeekExplorer([string]$Folder, [string]$Selected = '') {
    $seekShell = New-Object -ComObject Shell.Application
    $seekWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        foreach ($seekWindow in @($seekShell.Windows())) {
            try {
                $seekPath = ([Uri]$seekWindow.LocationURL).LocalPath.TrimEnd('\')
                if ($seekPath -eq $Folder.TrimEnd('\')) {
                    if (-not $Selected -or @($seekWindow.Document.SelectedItems() | Where-Object { $_.Path -eq $Selected }).Count -eq 1) { return $seekWindow }
                }
            } catch {} # Ignore non-Explorer shell windows.
        }
        Start-Sleep -Milliseconds 50
    } while ($seekWait.Elapsed.TotalSeconds -lt 10)
    throw 'Explorer did not show the expected fixture location/selection'
}
try {
    $seekInitial = Start-SeekCheck 1
    $seekVerify += Verify-SeekCheck 'initial'
    Set-SeekCheckQuery 'invoice ext:txt '
    $seekFiltered = Wait-SeekCheck { $_.query -eq 'invoice ext:txt ' -and $_.rows.Count -eq 2 }
    $seekChecks += 'filter chip created; query read back; two filtered rows painted'
    Invoke-SeekCheckButton 'ext: txt'
    $seekRemoved = Wait-SeekCheck { $null -ne $_.query -and $_.query.Trim() -eq 'invoice' -and $_.rows.Count -eq 3 }
    $seekChecks += 'chip removed; three matching rows painted'
    Invoke-SeekCheckButton 'Sort by Name'
    $seekSorted = Wait-SeekCheck { $_.interaction -eq 'sort' -and $_.detail.column -eq 0 }
    $seekExpected = @('invoice-alpha.txt', 'invoice-beta.txt', 'invoice-picture.png')
    if ($seekSorted.detail.direction -eq 'Descending') { [array]::Reverse($seekExpected) }
    if (($seekSorted.detail.rows -join '|') -ne ($seekExpected -join '|')) { throw 'Sort order mismatch' }
    $seekChecks += 'Name sort produced the requested order'
    [SeekStageCheck]::Key($seekHwnd, 40)
    $seekSelected = Wait-SeekCheck { $_.interaction -eq 'select' -and $_.detail.row -eq 0 } $seekSorted.entry_ms
    [SeekStageCheck]::Key($seekHwnd, 40)
    $seekSelected = Wait-SeekCheck { $_.interaction -eq 'select' -and $_.detail.row -eq 1 } $seekSelected.entry_ms
    [SeekStageCheck]::Key($seekHwnd, 38)
    $seekSelected = Wait-SeekCheck { $_.interaction -eq 'select' -and $_.detail.row -eq 0 } $seekSelected.entry_ms
    [SeekStageCheck]::Key($seekHwnd, 27)
    $seekCleared = Wait-SeekCheck { $_.query -eq '' -and $_.rows.Count -eq 4 } $seekSelected.entry_ms
    [SeekStageCheck]::Key($seekHwnd, 27)
    $null = Wait-SeekCheck { $_.interaction -eq 'selection_cleared' } $seekCleared.entry_to_present_ms
    $seekChecks += 'Down/Down/Up changed selection; Escape cleared query then selection'

    [IO.File]::WriteAllText((Join-Path $seekRoot 'invoice-live.txt'), 'Created while GUI is running.')
    Move-Item -LiteralPath (Join-Path $seekRoot 'invoice-alpha.txt') -Destination (Join-Path $seekRoot 'invoice-renamed.txt')
    Move-Item -LiteralPath (Join-Path $seekRoot 'invoice-beta.txt') -Destination (Join-Path $seekRoot 'subfolder/beta-moved.txt')
    Remove-Item -LiteralPath (Join-Path $seekRoot 'invoice-picture.png')
    $seekLive = Wait-SeekCheck { $_.query -eq '' -and (($_.rows | Sort-Object) -join '|') -eq 'beta-moved.txt|closed-before.txt|invoice-live.txt|invoice-renamed.txt' }
    $seekChecks += 'GUI live results reflect create, rename, move and delete'
    $seekVerify += Verify-SeekCheck 'live'
    if (-not [SeekStageCheck]::SetWindowPos($seekHwnd, [IntPtr]::Zero, 140, 110, 1200, 780, 4)) { throw 'Resize failed' }
    Start-Sleep -Milliseconds 200
    $seekBoundsBefore = [SeekStageCheck]::Bounds($seekHwnd)
    $seekExits += Close-SeekCheck
    $seekSaved = Get-Content (Join-Path $seekData 'settings.json') -Raw -Encoding UTF8 | ConvertFrom-Json

    [IO.File]::WriteAllText((Join-Path $seekRoot 'closed-created.txt'), 'Created while GUI is closed.')
    Move-Item -LiteralPath (Join-Path $seekRoot 'invoice-renamed.txt') -Destination (Join-Path $seekRoot 'closed-renamed.txt')
    Move-Item -LiteralPath (Join-Path $seekRoot 'invoice-live.txt') -Destination (Join-Path $seekRoot 'subfolder/closed-moved.txt')
    Remove-Item -LiteralPath (Join-Path $seekRoot 'subfolder/closed-before.txt')
    $seekRelaunch = Start-SeekCheck 2
    $seekCaught = Wait-SeekCheck { $_.query -eq '' -and -not $_.status.catching_up -and (($_.rows | Sort-Object) -join '|') -eq 'beta-moved.txt|closed-created.txt|closed-moved.txt|closed-renamed.txt' }
    $seekChecks += 'GUI relaunch results reflect closed-process create, rename, move and delete'
    $seekBoundsAfter = [SeekStageCheck]::Bounds($seekHwnd)
    foreach ($seekEdge in @('Left','Top','Right','Bottom')) {
        if ([Math]::Abs($seekBoundsBefore.$seekEdge - $seekBoundsAfter.$seekEdge) -gt 3) { throw "Window geometry not restored: $seekEdge" }
    }
    $seekChecks += 'window position and size restored within three physical pixels'
    $seekVerify += Verify-SeekCheck 'relaunch'
    if ($ShellActions) {
        Set-SeekCheckQuery 'subfolder kind:folder'
        $seekFolderResult = Wait-SeekCheck { $_.query -eq 'subfolder kind:folder' -and $_.rows.Count -eq 1 }
        [SeekStageCheck]::Key($seekHwnd, 13)
        $null = Wait-SeekCheck { $_.interaction -eq 'shell_dispatch' -and $_.detail.accepted -and -not $_.detail.reveal } $seekFolderResult.entry_to_present_ms
        $seekExplorer = Find-SeekExplorer (Join-Path $seekRoot 'subfolder')
        $seekShellChecks += 'Enter opened the selected folder in its Windows default handler (Explorer); exact fixture location read back'
        $seekExplorer.Quit() # Only this run's unique test folder window.
        Set-SeekCheckQuery "'closed-created.txt"
        $seekFileResult = Wait-SeekCheck { $_.query -eq "'closed-created.txt" -and $_.rows.Count -eq 1 }
        [void][SeekStageCheck]::SetForegroundWindow($seekHwnd)
        $seekFocusWait = [Diagnostics.Stopwatch]::StartNew()
        while ([SeekStageCheck]::GetForegroundWindow() -ne $seekHwnd -and $seekFocusWait.Elapsed.TotalSeconds -lt 3) { Start-Sleep -Milliseconds 20 }
        if ([SeekStageCheck]::GetForegroundWindow() -ne $seekHwnd) { throw 'Seek unavailable for the in-window reveal shortcut' }
        $seekOrca = (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
        $seekObservation = & $seekOrca computer get-app-state --app "pid:$($seekProcess.Id)" --no-screenshot --json | ConvertFrom-Json
        if (-not $seekObservation.ok) { throw 'Cannot observe the reveal test window' }
        $seekHotkey = & $seekOrca computer hotkey --app "pid:$($seekProcess.Id)" --key Ctrl+Return --no-screenshot --json | ConvertFrom-Json
        if (-not $seekHotkey.ok) { throw 'Reveal shortcut delivery failed' }
        $null = Wait-SeekCheck { $_.interaction -eq 'shell_dispatch' -and $_.detail.accepted -and $_.detail.reveal } $seekFileResult.entry_to_present_ms
        $seekExplorer = Find-SeekExplorer $seekRoot (Join-Path $seekRoot 'closed-created.txt')
        $seekShellChecks += 'Ctrl+Enter revealed the file in Explorer; full location and selected file path read back'
        $seekExplorer.Quit()
    }
    $seekExits += Close-SeekCheck
    $seekChecks += 'both window closes exit the complete GUI process'
    $seekReport = [ordered]@{
        captured_at=[DateTime]::UtcNow.ToString('o'); fixture=$seekFixture; executable=$seekExe
        checks=$seekChecks; verification=$seekVerify; close=$seekExits
        shell_actions=$seekShellChecks
        gui_walker_relaunch_catch_up_ms=$seekRelaunch.status.catch_up_ms
        saved_geometry=$seekSaved.window; geometry_before=$seekBoundsBefore; geometry_after=$seekBoundsAfter
        method='Real GUI processes, UI Automation Value/Invoke patterns with readback, native in-window key messages, painted-result trace and independent CLI ground-truth walk. No benchmark reruns.'
        limitations=$(if ($ShellActions) { 'No assertion of physical DWM scanout, non-folder default-handler contents, column drag resizing or true cold OS cache. Explorer location/selection was read back.' } else { 'No assertion of physical DWM scanout, default-handler contents, Explorer selection, column drag resizing or true cold OS cache.' })
    }
    $seekReport | ConvertTo-Json -Depth 9 | Set-Content -LiteralPath $Output -Encoding UTF8
    $seekReport | ConvertTo-Json -Depth 9
} finally {
    if ($seekProcess) { $seekProcess.Refresh(); if (-not $seekProcess.HasExited) { [void]$seekProcess.CloseMainWindow(); [void]$seekProcess.WaitForExit(10000) } }
}
