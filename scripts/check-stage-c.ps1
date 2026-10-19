param(
 [string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',
 [int]$IdleSeconds=0,
 [switch]$MemorySnapshots,
 [string]$Output='docs/benchmarks/stage-c-interactions.json',
 [string]$Driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$driver=$Driver
$base=Join-Path $env:TEMP ('seek-stage-c-check-'+[guid]::NewGuid().ToString('N'))
$root=Join-Path $base 'files';$data=Join-Path $base 'data';$trace=Join-Path $base 'metrics.ndjson'
New-Item -ItemType Directory -Path $root,$data | Out-Null
foreach($folder in @('Alpha','Beta')){
 $dir=Join-Path $root $folder;New-Item -ItemType Directory -Path $dir|Out-Null
 for($i=0;$i -lt 120;$i++){[IO.File]::WriteAllText((Join-Path $dir ('{0}-{1:d3}.txt' -f $folder,$i)),('Owned '+$folder+' fixture'))}
}
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),(@{theme='dark';glass=$true;content_locations=@($root);default_view='list'}|ConvertTo-Json -Compress))
$exe=(Resolve-Path -LiteralPath $Executable).Path
$app=Start-Process -FilePath $exe -ArgumentList @('--trace-frames','--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -PassThru
$checks=@()
$memory=@()
function State([switch]$Capture){
 $a=@('computer','get-app-state','--app',('pid:'+$app.Id),'--restore-window','--json');if(-not $Capture){$a+='--no-screenshot'}
 $s=& $driver @a|ConvertFrom-Json;if(-not $s.ok){throw ($s.error|ConvertTo-Json -Compress)};$s
}
function Click([string]$name){
 $s=State
 if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|menu item|row|edit|switch) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*|, Value:.*)?\s*$')){throw ('Missing '+$name+"`n"+$s.result.snapshot.treeText)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw ($r.error|ConvertTo-Json -Compress)};$r.result.snapshot.treeText
}
function Key([string]$key,[switch]$Chord){
 $null=State;$op=if($Chord){'hotkey'}else{'press-key'}
 $s=& $driver computer $op --app ('pid:'+$app.Id) --key $key --no-screenshot --json|ConvertFrom-Json
 if(-not $s.ok){throw ($s.error|ConvertTo-Json -Compress)};$s.result.snapshot.treeText
}
function SetQuery([string]$value){
 $s=State
 if($s.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) edit Search files and folders'){throw 'Missing search'}
 $r=& $driver computer set-value --app ('pid:'+$app.Id) --element-index $Matches[1] --value $value --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw ($r.error|ConvertTo-Json -Compress)};$r.result.snapshot.treeText
}
function Capture([string]$name){$s=State -Capture;Copy-Item -LiteralPath $s.result.screenshot.path -Destination (Join-Path $base ($name+'.png'));$s.result.snapshot.treeText|Set-Content (Join-Path $base ($name+'.txt'));$s.result.snapshot.treeText}
function PointClick([string]$name,[string]$modifiers='') {
 $null=State
 $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $e=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
 if(-not $e){throw ('Missing '+$name)}
 $r=$e.Current.BoundingRectangle;$bounds=$w.Current.BoundingRectangle
 $args=@('computer','click','--app',('pid:'+$app.Id),'--x',[int]($r.Left-$bounds.Left+[Math]::Min(80,$r.Width/2)),'--y',[int]($r.Top-$bounds.Top+$r.Height/2),'--no-screenshot','--json')
 if($modifiers){$args+=@('--modifiers',$modifiers)}
 $s=& $driver @args|ConvertFrom-Json;if(-not $s.ok){throw ($s.error|ConvertTo-Json -Compress)}
}

function Await([scriptblock]$condition,[string]$message){
 $deadline=[Diagnostics.Stopwatch]::StartNew()
 do {if(&$condition){return};Start-Sleep -Milliseconds 100}while($deadline.Elapsed.TotalSeconds -lt 15)
 throw $message
}
function Records {Get-Content -LiteralPath $trace -ErrorAction SilentlyContinue|ForEach-Object{try{$_|ConvertFrom-Json}catch{}}}
function MemorySnapshot([string]$name){
 Start-Sleep -Milliseconds 500
 $app.Refresh()
 $stats=Records|Where-Object{$_.interaction -eq 'preview_cache'}|Select-Object -Last 1
 $script:memory+=@{name=$name;private_commit_bytes=$app.PrivateMemorySize64;working_set_bytes=$app.WorkingSet64;media_cache=$stats.detail.stats}
}
function Check([string]$name,[bool]$passed){if(-not $passed){throw $name};$script:checks+=@{name=$name;passed=$true}}
try{
 Await {Test-Path $trace} 'No metrics'
 Await {@(Records|Where-Object{$_.status.ready -and -not $_.status.catching_up}).Count -gt 0} 'Fixture not ready'
 $null=SetQuery 'Alpha'
 Await {(State).result.snapshot.treeText -match 'row Alpha-000.txt'} 'Alpha search missing'
 if($MemorySnapshots){$null=PointClick 'Alpha-008.txt';MemorySnapshot 'one-tab-warm-preview'}
 $null=Key 'Ctrl+t' -Chord
 $null=SetQuery 'Beta'
 Await {(State).result.snapshot.treeText -match 'row Beta-000.txt'} 'Beta search missing'
 $null=Key 'Ctrl+Tab' -Chord
 Await {(State).result.snapshot.treeText -match 'edit Search files and folders Alpha'} 'Query not restored'
 Check 'Two searches remain independent; Ctrl+Tab wraps' $true
 $null=PointClick 'Alpha';$null=Key 'Ctrl+Return' -Chord
 Await {(State).result.snapshot.treeText -match 'Switch to tab 3: Alpha'} 'Folder new tab missing'
 $null=State
 $rows=(Records|Where-Object{$null -ne $_.query -and $_.query -eq '' -and $_.rows.Count -eq 120}|Select-Object -Last 1).rows
 $truth=@(Get-ChildItem -LiteralPath (Join-Path $root 'Alpha') -File|ForEach-Object Name)
 Check 'New folder tab equals independent directory listing (120 files)' (@(Compare-Object $truth $rows).Count -eq 0)
 $null=State;$r=& $driver computer scroll --app ('pid:'+$app.Id) --x 650 --y 500 --direction down --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw 'Scroll failed'}
 $before=((State).result.snapshot.treeText -split "`n"|Where-Object{$_ -match '\srow '}|Select-Object -First 1) -replace '^\s*\d+ ',''
 $null=Key 'Ctrl+Tab' -Chord;$null=Key 'Ctrl+Shift+Tab' -Chord
 $after=((State).result.snapshot.treeText -split "`n"|Where-Object{$_ -match '\srow '}|Select-Object -First 1) -replace '^\s*\d+ ',''
 Check 'List viewport returns to same row' ($before -eq $after -and $before -notmatch 'Alpha-000')
 $null=Key 'Ctrl+2' -Chord
 $null=State;$gridMove=& $driver computer scroll --app ('pid:'+$app.Id) --x 650 --y 500 --direction down --no-screenshot --json|ConvertFrom-Json
 if(-not $gridMove.ok){throw 'Grid scroll failed'}
 $null=Key 'Ctrl+Tab' -Chord;$null=Key 'Ctrl+Shift+Tab' -Chord
 $gridRestore=Records|Where-Object{$_.interaction -eq 'tab_restored'}|Select-Object -Last 1
 Check 'Grid view and its scroll offset survive tab switching' ($gridRestore.detail.grid_y -lt 0 -and (State).result.snapshot.treeText -match 'button Alpha-')
 $null=Key 'Ctrl+1' -Chord
 $null=Key 'Ctrl+t' -Chord;$null=SetQuery 'Owned'
 $menu=State
 if($menu.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) menu item Content contains'){throw 'Content suggestion missing'}
 $choose=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $choose.ok){throw 'Content selection failed'}
 Await {(State).result.snapshot.treeText -match '120 matches.*Complete'} 'Owned content scan not complete'
 $null=Key 'Ctrl+Tab' -Chord;$null=Key 'Ctrl+Shift+Tab' -Chord
 Await {(State).result.snapshot.treeText -match '120 matches.*Complete'} 'Content tab not restored'
 $contentRows=(Records|Where-Object{$_.query -eq 'Owned' -and $_.rows.Count -eq 120}|Select-Object -Last 1).rows
 Check 'Content search mode, query and Alpha scope survive tab switch' (@(Compare-Object $truth $contentRows).Count -eq 0)
 $null=Key 'Ctrl+w' -Chord
 Await {(State).result.snapshot.treeText -notmatch 'Close tab 4'} 'Content tab close failed'
 if($MemorySnapshots){
  $null=PointClick 'Alpha-008.txt';MemorySnapshot 'three-tabs-warm-preview'
  for($n=2;$n -lt 14;$n++){$null=PointClick ('Alpha-{0:d3}.txt' -f $n);Start-Sleep -Milliseconds 150}
  MemorySnapshot 'three-tabs-after-12-previews'
  $stats=Records|Where-Object{$_.interaction -eq 'preview_cache'}|Select-Object -Last 1
  if($stats){Check 'Inactive preview images/details are not retained' ($stats.detail.stats[2] -le 1 -and $stats.detail.stats[3] -le 1)}
 }
 $null=Capture 'tabs-dark' 
 $null=PointClick 'Alpha-008.txt';$null=Key 'Shift+Delete' -Chord
 $null=Capture 'delete-dark';$null=Click 'Cancel'
 $null=Click 'Settings';$null=Click 'Light';$null=Capture 'settings-light';$null=Click 'Done'
 $null=PointClick 'Alpha-008.txt';$null=Key 'Shift+Delete' -Chord
 $null=Capture 'delete-light';$null=Click 'Cancel';$null=Capture 'tabs-light'
 Check 'Modal Cancel preserves all 240 disposable files' (@(Get-ChildItem $root -File -Recurse).Count -eq 240)
 $idle=$null
 if($IdleSeconds -gt 0){
  Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class SeekTabsIdle {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [StructLayout(LayoutKind.Sequential)] public struct Input { public uint cbSize; public uint time; }
 [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref Input value);
 public static uint Stamp(){var i=new Input();i.cbSize=(uint)Marshal.SizeOf(i);GetLastInputInfo(ref i);return i.time;}
}
"@
  $app.Refresh();$w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle).Current.BoundingRectangle
  [void][SeekTabsIdle]::SetCursorPos([int]($w.Right-20),[int]($w.Bottom-12));Start-Sleep -Milliseconds 500
  $framesBefore=@(Records|Where-Object{$null -ne $_.frame_entry_ms}).Count
  $stamp=[SeekTabsIdle]::Stamp();$quiet=$true;$app.Refresh();$cpu=$app.TotalProcessorTime.TotalMilliseconds
  $clock=[Diagnostics.Stopwatch]::StartNew()
  while($clock.Elapsed.TotalSeconds -lt $IdleSeconds){
   if([SeekTabsIdle]::GetForegroundWindow() -ne $app.MainWindowHandle -or [SeekTabsIdle]::Stamp() -ne $stamp){$quiet=$false}
   Start-Sleep -Milliseconds 250
  }
  $app.Refresh();$cpuDelta=$app.TotalProcessorTime.TotalMilliseconds-$cpu
  $idle=@{tabs=3;seconds=$clock.Elapsed.TotalSeconds;cpu_ms=$cpuDelta;quiet_foreground=$quiet;redraws=(@(Records|Where-Object{$null -ne $_.frame_entry_ms}).Count-$framesBefore);private_commit_bytes=$app.PrivateMemorySize64;working_set_bytes=$app.WorkingSet64}
  Check 'Three-tab foreground idle has no measured CPU/redraws/input' ($quiet -and $cpuDelta -eq 0 -and $idle.redraws -eq 0)
 }
 $null=Click 'Close tab 2' 
 Await {(State).result.snapshot.treeText -notmatch 'Switch to tab 3'} 'Inactive close failed'
 $null=Key 'Ctrl+w' -Chord
 Await {(State).result.snapshot.treeText -notmatch 'Close tab 2'} 'Ctrl+W did not close one tab'
 Check 'Close inactive and active tabs without closing remaining window' (-not $app.HasExited)
 if($MemorySnapshots){MemorySnapshot 'one-tab-after-close'}
 try{$null=Key 'Ctrl+w' -Chord}catch{if(-not $app.WaitForExit(1000)){throw}}
 Check 'Closing final tab quits process' ($app.WaitForExit(5000) -and $app.ExitCode -eq 0)
 @{captured_at=[DateTime]::UtcNow.ToString('o');executable=$exe;sha256=(Get-FileHash $exe).Hash;fixture=$base;checks=$checks;idle=$idle;memory=$memory;visual_captures=@('tabs-dark','delete-dark','settings-light','delete-light','tabs-light');limitations='Native SDR at 125%; no physical mixed-DPI/HDR check; no real file deletion.'}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Output -Encoding UTF8
 'PASS '+$checks.Count+' focused interaction checks'
}finally{if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
