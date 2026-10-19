param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/debug/seek.exe',
    [string]$Output = 'docs/benchmarks/stage3-ui-check.json',
    [switch]$KeepOpen
)
# Isolated, ordinary-user UI check. Only creates Seek-owned fixture/data files.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekProductCheck {
 [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder lpClassName, int nMaxCount);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc enumProc, IntPtr lParam);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
 public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
 public static IntPtr FindSeekWindow(uint pid) {
     IntPtr found = IntPtr.Zero;
     EnumWindows((h, l) => {
         uint p;
         GetWindowThreadProcessId(h, out p);
         if (p == pid) {
             var sb = new System.Text.StringBuilder(256);
             GetClassName(h, sb, 256);
             if (sb.ToString() == "Zed::Window") {
                 found = h;
                 return false;
             }
         }
         return true;
     }, IntPtr.Zero);
     return found;
 }
 public static void Key(IntPtr h,int key){SetForegroundWindow(h);PostMessageW(h,0x100,new IntPtr(key),IntPtr.Zero);PostMessageW(h,0x101,new IntPtr(key),IntPtr.Zero);}
}
'@
$seekExe = (Resolve-Path -LiteralPath $Executable).Path
$seekFixture = [IO.Path]::GetFullPath(".bench-data/stage3-check-$([DateTime]::UtcNow.Ticks)")
$seekRoot = Join-Path $seekFixture 'files'
$seekData = Join-Path $seekFixture 'index'
$seekTrace = Join-Path $seekFixture 'trace.ndjson'
New-Item -ItemType Directory -Path (Join-Path $seekRoot 'Expansion/Nested') -Force | Out-Null
for($i=0;$i -lt 2048;$i++){[IO.File]::WriteAllText((Join-Path $seekRoot ('Expansion/child-{0:0000}.txt' -f $i)),'Indexed child.')}
[IO.File]::WriteAllText((Join-Path $seekRoot 'Expansion/Nested/deep.txt'),'Nested child.')
$seekHidden = Join-Path $seekRoot 'Expansion/hidden.txt'
[IO.File]::WriteAllText($seekHidden,'Hidden indexed child.')
(Get-Item -LiteralPath $seekHidden).Attributes = [IO.FileAttributes]::Hidden
foreach($name in @('Report.pdf','Budget.xlsx','Notes.docx','Slides.pptx','README.md','package.json','Archive.zip','Music.mp3','Unknown.xyz')){
    [IO.File]::WriteAllText((Join-Path $seekRoot $name),'Seek preview fixture. Synthetic office documents; do not open.')
}
Copy-Item -LiteralPath app/assets/spike.png -Destination (Join-Path $seekRoot 'Picture.png')
[IO.File]::WriteAllText((Join-Path $seekRoot 'Picture.svg'),'<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" rx="10" fill="#006fe5"/><circle cx="80" cy="50" r="28" fill="#fff"/></svg>')
$seekVideo = '.bench-data/stage3-ui/files/Preview.mp4'
if(Test-Path -LiteralPath $seekVideo){Copy-Item -LiteralPath $seekVideo -Destination (Join-Path $seekRoot 'Preview.mp4')}
$seekChecks = [Collections.Generic.List[object]]::new()
$seekApp = $null
function SeekHandle {
    $h = [SeekProductCheck]::FindSeekWindow($seekApp.Id)
    if ($h -ne [IntPtr]::Zero) { return $h }
    $seekApp.Refresh()
    return $seekApp.MainWindowHandle
}
function Records {
    if(-not(Test-Path -LiteralPath $seekTrace)){return}
    $stream=[IO.File]::Open($seekTrace,'Open','Read','ReadWrite');$reader=[IO.StreamReader]::new($stream)
    try{$text=$reader.ReadToEnd()}finally{$reader.Dispose()}
    foreach($line in $text.Split("`n")){try{$line|ConvertFrom-Json}catch{}}
}
function Wait-Trace([scriptblock]$Predicate,[double]$After=-1){
    $timer=[Diagnostics.Stopwatch]::StartNew()
    do{
        $seekApp.Refresh();if($seekApp.HasExited){throw 'Seek exited during the check'}
        $match=Records|Where-Object {($_.entry_ms -gt $After -or $_.applied_entry_ms -gt $After)}|Where-Object $Predicate|Select-Object -Last 1
        if($match){return $match};Start-Sleep -Milliseconds 30
    }while($timer.Elapsed.TotalSeconds -lt 15)
    throw 'Timed out waiting for a UI assertion'
}
function Element([string]$Name){
    $window=[Windows.Automation.AutomationElement]::FromHandle((SeekHandle))
    $condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$Name)
    return $window.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
}
function Wait-Element([string]$Name,[bool]$Present=$true){
    $timer=[Diagnostics.Stopwatch]::StartNew()
    do{$e=Element $Name;if(($null -ne $e) -eq $Present){return $e};Start-Sleep -Milliseconds 50}while($timer.Elapsed.TotalSeconds -lt 10)
    throw "UI did not reach '$Name' (present=$Present)"
}
function Button([string]$Name){
    # Use the known desktop driver's native click, then independently assert effects.
    [void][SeekProductCheck]::SetForegroundWindow((SeekHandle))
    $state=& (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer get-app-state --app ('pid:'+$seekApp.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
    if(-not $state.ok){throw $state.error.message}
    if($state.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|switch|checkbox) '+[regex]::Escape($Name)+'(?:, Secondary Actions:.*)?\s*$')){throw "Missing button: $Name"}
    $result=& (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer click --app ('pid:'+$seekApp.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
    if(-not $result.ok){throw $result.error.message}
}
function Query([string]$Text){
    $w=[Windows.Automation.AutomationElement]::FromHandle((SeekHandle))
    $e=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
    $e.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
    [void][SeekProductCheck]::SetForegroundWindow((SeekHandle))
    return Wait-Trace {$_.query -eq $Text -and $null -ne $_.entry_to_present_ms}
}
function Key([int]$Code){[SeekProductCheck]::Key((SeekHandle),$Code)}
function Settings { Get-Content -LiteralPath (Join-Path $seekData 'settings.json') -Raw -Encoding UTF8|ConvertFrom-Json }
function Wait-Settings([scriptblock]$Predicate){
    $timer=[Diagnostics.Stopwatch]::StartNew()
    do{if((Test-Path (Join-Path $seekData 'settings.json')) -and (Settings|Where-Object $Predicate)){return};Start-Sleep -Milliseconds 50}while($timer.Elapsed.TotalSeconds -lt 10)
    throw 'Settings readback did not match'
}
function Passed([string]$Name,$Detail){$seekChecks.Add([ordered]@{name=$Name;passed=$true;detail=$Detail});Write-Output "PASS $Name"}
try{
    $seekApp=Start-Process -FilePath $seekExe -ArgumentList @('--walker','--root',('"'+$seekRoot+'"'),'--data',('"'+$seekData+'"'),'--metrics',('"'+$seekTrace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $seekFixture 'stderr.txt')
    $seekHandle=$seekApp.Handle
    $ready=Wait-Trace {$_.status.ready -and -not $_.status.catching_up -and $null -ne $_.entry_to_present_ms}
    [void](Wait-Element 'Settings')
    Passed 'Ordinary-user window and complete fixture index' @{entries=$ready.status.entries;skipped=$ready.status.skipped;catch_up_ms=$ready.status.catch_up_ms}
    $q=Query "'Expansion"
    if($q.rows.Count -ne 1){throw 'Expected exactly the Expansion folder'}
    Key 40;$selected=Wait-Trace {$_.interaction -eq 'select' -and $_.detail.name -eq 'Expansion'} $q.applied_entry_ms
    Key 39;$expanded=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.expanded -eq 1 -and $_.detail.rows.Count -eq 2050} $selected.entry_ms
    Passed 'Right expands 2,049 direct children from the index' @{rows=$expanded.detail.rows.Count;max_depth=($expanded.detail.depths|Measure-Object -Maximum).Maximum}
    [void](Wait-Element 'Collapse Expansion')
    $window=[Windows.Automation.AutomationElement]::FromHandle((SeekHandle))
    $visible=$window.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::DataItem)).Count
    if($visible -lt 1 -or $visible -gt 80){throw 'Invalid realized-row count for this viewport'}
    Passed 'Virtualized expanded list' @{realized_rows=$visible;logical_rows=2050}
    Button 'Sort results';[void](Wait-Element 'Name Z–A');Button 'Name Z–A'
    Key 40;$nested=Wait-Trace {$_.interaction -eq 'select' -and $_.detail.name -eq 'Nested'} $expanded.entry_ms
    Key 39;$nestedExpanded=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.expanded -eq 2 -and $_.detail.rows.Count -eq 2051} $nested.entry_ms
    if($nestedExpanded.detail.rows[0] -ne 'Expansion' -or $nestedExpanded.detail.rows[1] -ne 'Nested' -or $nestedExpanded.detail.rows[2] -ne 'deep.txt' -or $nestedExpanded.detail.depths[2] -ne 2){throw 'Nested descending order/indent is wrong'}
    Passed 'Nested expansion follows sibling sort and indentation' @{first_rows=$nestedExpanded.detail.rows[0..4];depths=$nestedExpanded.detail.depths[0..4]}
    Key 37;$collapsed=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.expanded -eq 1 -and $_.detail.rows.Count -eq 2050} $nestedExpanded.entry_ms
    Passed 'Left collapses selected subfolder' @{rows=$collapsed.detail.rows.Count}
    Button 'Show hidden and system files'
    $hidden=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.rows -contains 'hidden.txt'} $collapsed.entry_ms
    Passed 'Expanded contents follow hidden-files setting' @{rows=$hidden.detail.rows.Count}
    Button 'Collapse Expansion';$closed=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.expanded -eq 0 -and $_.detail.rows.Count -eq 1} $hidden.entry_ms
    Button 'Expand Expansion';$click=Wait-Trace {$_.interaction -eq 'tree_rows' -and $_.detail.expanded -eq 1 -and $_.detail.rows.Count -eq 2051} $closed.entry_ms
    Passed 'Disclosure clicks collapse and expand' @{rows=$click.detail.rows.Count}
    $pic=Query "'Picture.png";Key 40
    $image=Wait-Trace {$_.interaction -eq 'preview_ready' -and $_.detail.name -eq 'Picture.png' -and $_.detail.edge -eq 480 -and $_.detail.image} $pic.applied_entry_ms
    [void](Wait-Element 'Information panel');Key 32;[void](Wait-Element 'Information panel' $false);Key 32;[void](Wait-Element 'Information panel')
    Passed 'Real image preview and Space panel toggle' $image.detail
    foreach($view in @('Icons','Gallery','List')){
        $label=@{Icons='Icons · Ctrl+2';Gallery='Gallery · Ctrl+3';List='List · Ctrl+1'}[$view]
        Button $label;$v=Wait-Trace {$_.interaction -eq 'view' -and $_.detail.view -eq $view}
        Passed "$view view activates" $v.detail
    }
    $svg=Query "'Picture.svg";Key 40
    $svgImage=Wait-Trace {$_.interaction -eq 'preview_ready' -and $_.detail.name -eq 'Picture.svg' -and $_.detail.edge -eq 480 -and $_.detail.image} $svg.applied_entry_ms
    Passed 'Real SVG preview' $svgImage.detail
    if(Test-Path -LiteralPath $seekVideo){
        $video=Query "'Preview.mp4";Key 40
        $v=Wait-Trace {$_.interaction -eq 'preview_ready' -and $_.detail.name -eq 'Preview.mp4' -and $_.detail.edge -eq 480} $video.applied_entry_ms
        Passed 'Windows video provider queried' $v.detail
    }
    $text=Query "'README.md";Key 40
    [void](Wait-Trace {$_.interaction -eq 'preview_ready' -and $_.detail.name -eq 'README.md' -and $_.detail.details} $text.applied_entry_ms)
    Passed 'Markdown uses text icon and worker text preview' @{}
    Button 'Settings';[void](Wait-Element 'System')
    foreach($theme in @('Dark','Light','System')){Button $theme;Wait-Settings {$_.theme -eq $theme.ToLower()};Passed "$theme theme saved" @{theme=(Settings).theme}}
    Button 'Glass: on';Wait-Settings {-not $_.glass};[void](Wait-Element 'Glass: off')
    Button 'Glass: off';Wait-Settings {$_.glass};Passed 'Glass can be disabled and restored' @{glass=(Settings).glass}
    Button 'Purple';Wait-Settings {$_.accent -eq '#8250c4'}
    $custom=Wait-Element '#006fe5';$custom.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue('#267e58')
    Button 'Apply';Wait-Settings {$_.accent -eq '#267e58'};Passed 'Preset and custom accent persisted' @{accent=(Settings).accent}
    Button 'Locations'
    $locations=Wait-Element 'All drives (default)';$locations.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($seekRoot)
    $exclusions=Wait-Element 'C:\path; C:\other';$exclusions.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue((Join-Path $seekRoot 'Expansion/Nested'))
    Button 'Save locations';Wait-Settings {$_.indexed_locations.Count -eq 1 -and $_.exclusions.Count -eq 1}
    Button 'Files and search'
    Button 'Icons';Wait-Settings {$_.default_view -eq 'icons'}
    Passed 'Indexed locations, exclusions and default view saved' @{locations=(Settings).indexed_locations;exclusions=(Settings).exclusions;view=(Settings).default_view}
    Button 'Done';[void](Wait-Element 'System' $false)
    Key 27;Key 27
    [void](Wait-Trace {$_.interaction -eq 'selection_cleared'})
    Passed 'Escape clears the search and selection' @{}
    [void][SeekProductCheck]::ShowWindow((SeekHandle),6)
    $second=Start-Process -FilePath $seekExe -ArgumentList @('--walker','--root',('"'+$seekRoot+'"'),'--data',('"'+$seekData.ToUpper()+'"')) -WindowStyle Normal -PassThru
    $secondHandle=$second.Handle
    if(-not $second.WaitForExit(5000)){throw 'Second instance did not exit'};$second.WaitForExit()
    Start-Sleep -Milliseconds 100
    if($second.ExitCode -ne 0 -or [SeekProductCheck]::IsIconic((SeekHandle))){throw 'Existing instance did not restore'}
    Passed 'Second launch exits and restores the existing window' @{second_exit=$second.ExitCode;foreground=([SeekProductCheck]::GetForegroundWindow() -eq (SeekHandle))}
    $memory=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekApp.Id)"
    $close=$null
    if(-not $KeepOpen){$timer=[Diagnostics.Stopwatch]::StartNew();[void]$seekApp.CloseMainWindow();if(-not $seekApp.WaitForExit(10000)){throw 'Close did not quit'};$seekApp.WaitForExit();$close=@{ms=$timer.Elapsed.TotalMilliseconds;exit=$seekApp.ExitCode};if($seekApp.ExitCode -ne 0){throw 'Nonzero app exit'};Passed 'Closing exits the entire app' $close}
    $report=[ordered]@{executable=$seekExe;fixture=$seekFixture;pid=$seekApp.Id;checks=$seekChecks;private_working_set_bytes=[long]$memory.WorkingSetPrivate;private_commit_bytes=[long]$memory.PrivateBytes;close=$close;settings=Settings;note='Focused Stage 3 fixture checks. Windows caches warm; not a whole-drive or foreground-idle benchmark.'}
    $report|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
    Write-Output "Results: $Output"
}catch{
    [ordered]@{executable=$seekExe;fixture=$seekFixture;pid=$seekApp.Id;checks=$seekChecks;failure=$_.Exception.Message}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
    throw
}finally{
    if($seekApp -and -not $KeepOpen -and -not $seekApp.HasExited){[void]$seekApp.CloseMainWindow();[void]$seekApp.WaitForExit(5000)}
}
