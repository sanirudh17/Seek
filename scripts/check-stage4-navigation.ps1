param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [string]$Output='docs/benchmarks/stage4-navigation-ui.json',
 [switch]$RealProfile,
 [switch]$ProfileOnly
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class SeekNavigationKeys {
 [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
 public static void Key(IntPtr h,int k){PostMessageW(h,0x100,new IntPtr(k),IntPtr.Zero);PostMessageW(h,0x101,new IntPtr(k),IntPtr.Zero);}
}
'@
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$exe=(Resolve-Path -LiteralPath $Executable).Path
$fixture=[IO.Path]::GetFullPath('.bench-data/navigation-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $fixture 'files';$data=Join-Path $fixture 'index';$trace=Join-Path $fixture 'trace.ndjson'
New-Item -ItemType Directory -Path $data -Force|Out-Null
if($RealProfile){
 foreach($name in @('index.bin','roots.json','settings.json','opened.json','opened-dates.json')){
  $path=Join-Path (Join-Path $env:LOCALAPPDATA 'Seek') $name
  if(Test-Path -LiteralPath $path){Copy-Item -LiteralPath $path -Destination (Join-Path $data $name)}
 }
 # Whole-drive metrics must stay in memory, otherwise the trace watches itself.
 $collectorSource=(Get-Content scripts/measure-drive-startup.ps1 -Raw) -split "Add-Type @'",2
 $collectorSource=($collectorSource[1] -split "'@",2)[0]
 Add-Type $collectorSource.Replace('records.Count<512','records.Count<8192')
 $collector=[SeekStage4Collector]::new();$trace=$collector.Path
 $arguments=@('--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"'))
}else{
 New-Item -ItemType Directory -Path (Join-Path $root 'Expansion/Nested') -Force|Out-Null
 for($i=0;$i -lt 2048;$i++){[IO.File]::WriteAllText((Join-Path $root ('Expansion/child-{0:0000}.txt' -f $i)),'Indexed child.')}
 [IO.File]::WriteAllText((Join-Path $root 'Expansion/Nested/deep.txt'),'Nested child.')
 $arguments=@('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"'))
}
$checks=[Collections.Generic.List[object]]::new();$timings=[Collections.Generic.List[object]]::new()
function Records {
 if($RealProfile){$lines=$collector.Snapshot()}else{
  if(-not(Test-Path -LiteralPath $trace)){return}
  $stream=[IO.File]::Open($trace,'Open','Read','ReadWrite');$reader=[IO.StreamReader]::new($stream)
  try{$lines=$reader.ReadToEnd().Split("`n")}finally{$reader.Dispose()}
 }
 foreach($line in $lines){try{$line|ConvertFrom-Json}catch{}}
}
function WaitTrace([scriptblock]$condition,[double]$after=-1,[int]$seconds=15){
 $deadline=[DateTime]::UtcNow.AddSeconds($seconds)
 do{
  $app.Refresh();if($app.HasExited){throw ('Seek exited: '+$app.ExitCode)}
  $record=Records|Where-Object{($_.entry_ms -gt $after -or $_.applied_entry_ms -gt $after)}|Where-Object $condition|Select-Object -Last 1
  if($record){return $record};Start-Sleep -Milliseconds 30
 }while([DateTime]::UtcNow -lt $deadline)
 throw 'Timed out waiting for navigation assertion'
}
function State {& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json}
function Activate([string]$name){
 $state=State;if(-not $state.ok){throw $state.error.message}
 $pattern='(?m)^\s*(\d+) (?:button|switch|checkbox|toggle button) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$'
 if($state.result.snapshot.treeText -notmatch $pattern){throw ('Missing control '+$name)}
 $result=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $result.ok){throw $result.error.message}
}
function SetQuery([string]$text){
 $w=[Windows.Automation.AutomationElement]::FromHandle($hwnd)
 $edit=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
 $edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($text)
}
$app=Start-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Normal -PassThru
$retained=$app.Handle
try{
 $initial=WaitTrace {$_.status.ready -and ($RealProfile -or -not $_.status.catching_up)} -1 180
 $app.Refresh();$hwnd=$app.MainWindowHandle
 if($RealProfile){
  $last=$initial.applied_entry_ms
  foreach($name in @('Downloads','Desktop','Documents','Pictures','Videos','Downloads')){
   Activate $name
   $row=WaitTrace {$_.status.ready -and $null -ne $_.query_to_present_ms} $last 30
   $last=$row.applied_entry_ms
   $timings.Add(@{location=$name;rows=$row.rows.Count;request_to_present_ms=$row.query_to_present_ms;catching_up=$row.status.catching_up})
  }
  $checks.Add(@{name='Real sidebar folders present cached indexed contents';passed=$true})
  if($RealProfile){
   foreach($name in @('Downloads','Desktop','Documents','Pictures','Videos','Downloads')){
    Activate $name
    $row=WaitTrace {$_.status.ready -and $null -ne $_.query_to_present_ms} $last 30
    $last=$row.applied_entry_ms
    $timings.Add(@{location=$name;rows=$row.rows.Count;request_to_present_ms=$row.query_to_present_ms;catching_up=$row.status.catching_up;phase='second sweep'})
   }
  }
  $slow=@($timings|Where-Object {$_.request_to_present_ms -gt 100})
  if($slow.Count){throw ('Real folder navigation exceeded 100 ms: '+(($slow|ForEach-Object{$_.location+' '+[Math]::Round($_.request_to_present_ms,2)+' ms'}) -join ', '))}
  $checks.Add(@{name='All measured real-folder presentations finish within 100 ms';passed=$true})
 }else{
  SetQuery "'Expansion"
  $query=WaitTrace {$_.query -eq "'Expansion" -and $_.rows.Count -eq 1} $initial.applied_entry_ms
  [SeekNavigationKeys]::Key($hwnd,40)
  $selected=WaitTrace {$_.interaction -eq 'select' -and $_.detail.name -eq 'Expansion'} $query.applied_entry_ms
  [SeekNavigationKeys]::Key($hwnd,39)
  $expanded=WaitTrace {$_.interaction -eq 'tree_rows' -and $_.detail.rows.Count -eq 2050} $selected.entry_ms
  $paint=WaitTrace {$_.generation -gt $query.generation -and $null -ne $_.query_to_present_ms} $selected.entry_ms
  $timings.Add(@{location='Expand indexed folder';rows=2050;request_to_present_ms=$paint.query_to_present_ms})
  if($paint.query_to_present_ms -gt 100){throw ('Indexed folder expansion exceeded 100 ms: '+$paint.query_to_present_ms)}
  $checks.Add(@{name='Indexed folder expands all direct children';passed=$true;rows=$expanded.detail.rows.Count})
  $state=State
  $window=[Windows.Automation.AutomationElement]::FromHandle($hwnd)
  $rect=$window.Current.BoundingRectangle
  for($page=0;$page -lt 4;$page++){
   $state=State
   $result=& $driver computer scroll --app ('pid:'+$app.Id) --x ([int]($rect.Width*0.48)) --y ([int]($rect.Height*0.55)) --direction down --pages 240 --no-screenshot --json|ConvertFrom-Json
   if(-not $result.ok){throw $result.error.message}
  }
  $scrolled=WaitTrace {$_.interaction -eq 'visible_rows' -and $_.detail.start -gt 200} $expanded.entry_ms
  $before=$scrolled.detail.start
  for($i=0;$i -lt 8;$i++){[IO.File]::WriteAllText((Join-Path $root ('Expansion/new-{0:0000}.txt' -f $i)),'Live update.')}
  $updated=WaitTrace {$_.interaction -eq 'tree_rows' -and $_.detail.rows.Count -eq 2058} $scrolled.entry_ms
  Start-Sleep -Milliseconds 250
  $last=Records|Where-Object {$_.interaction -eq 'visible_rows'}|Select-Object -Last 1
  if($last.detail.start -lt $before-2){throw ('Scroll jumped: '+$before+' -> '+$last.detail.start)}
  $checks.Add(@{name='Selected offscreen folder does not pull scrolling back during live updates';passed=$true;before=$before;after=$last.detail.start})
  [IO.File]::Delete((Join-Path $root 'Expansion/child-0000.txt'))
  $deleted=WaitTrace {$_.interaction -eq 'tree_rows' -and $_.detail.rows.Count -eq 2057} $updated.entry_ms
  Start-Sleep -Milliseconds 250
  $last=Records|Where-Object {$_.interaction -eq 'visible_rows'}|Select-Object -Last 1
  if($last.detail.start -lt $before-3){throw 'Deleting a row above the viewport jumped to the selected folder'}
  $checks.Add(@{name='Deleting above the viewport retains the scroll anchor';passed=$true;before=$before;after=$last.detail.start})
  Activate 'Settings';Activate 'Files and search'
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-seek-window.ps1 -ProcessId $app.Id -Output '.impeccable/review/navigation-hidden-left.png'|Out-Null
  $checks.Add(@{name='Hidden-files selector left alignment captured';passed=$true})
 }
 $ready=if($ProfileOnly){$initial}else{WaitTrace {$_.status.ready -and -not $_.status.catching_up} -1 180}
 if(-not $ready){throw 'Synchronization never completed'}
 if(-not $ProfileOnly){$checks.Add(@{name='Catch-up completes with an ordered boundary';passed=$true;catch_up_ms=$ready.status.catch_up_ms})}
 [void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Close did not exit'};$app.WaitForExit()
 if($app.ExitCode -ne 0){throw 'Close returned failure'}
 @{executable=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;fixture=$fixture;real_profile_copy=[bool]$RealProfile;checks=$checks;timings=$timings;profiles=@(Records|Where-Object{$_.interaction -eq 'browse_profile'});exit_code=$app.ExitCode}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 Write-Output ('PASS '+$checks.Count+' navigation checks')
}catch{
 try{(State)|ConvertTo-Json -Depth 12|Set-Content .references/navigation-ui-failure.json -Encoding UTF8}catch{}
 @{checks=$checks;timings=$timings;profiles=@(Records|Where-Object{$_.interaction -eq 'browse_profile'});failure=$_.Exception.Message;fixture=$fixture}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 throw
}finally{
 $app.Refresh();if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(10000)}
 if($collector){$collector.Dispose()}
}
