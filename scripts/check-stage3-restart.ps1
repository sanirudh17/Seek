param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [string]$Output='docs/benchmarks/stage3-restart.json',
 [switch]$KeepOpen
)
# Reuses the completed Stage 3 fixture; no new indexing workload or service changes.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SeekRestartKeys {
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
}
'@
$previous=Get-Content docs/benchmarks/stage3-release-ui-check.json -Raw -Encoding UTF8|ConvertFrom-Json
$data=Join-Path $previous.fixture 'index';$trace=Join-Path $previous.fixture ('restart-'+[DateTime]::UtcNow.Ticks+'.ndjson')
$saved=Get-Content (Join-Path $data 'settings.json') -Raw -Encoding UTF8|ConvertFrom-Json
$exe=(Resolve-Path -LiteralPath $Executable).Path
$app=Start-Process -FilePath $exe -ArgumentList @('--walker','--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $previous.fixture 'restart-stderr.txt')
$handle=$app.Handle
function Records {
 if(-not(Test-Path $trace)){return}
 $s=[IO.File]::Open($trace,'Open','Read','ReadWrite');$r=[IO.StreamReader]::new($s)
 try{$text=$r.ReadToEnd()}finally{$r.Dispose()}
 foreach($l in $text.Split("`n")){try{$l|ConvertFrom-Json}catch{}}
}
function Wait-Record([scriptblock]$Condition,[double]$After=-1){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$app.Refresh();if($app.HasExited){throw 'Unexpected app exit'};$r=Records|Where-Object {($_.entry_ms -gt $After -or $_.applied_entry_ms -gt $After)}|Where-Object $Condition|Select-Object -Last 1;if($r){return $r};Start-Sleep -Milliseconds 30}while($watch.Elapsed.TotalSeconds -lt 15)
 throw 'Restart assertion timed out'
}
function State { & (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json }
function Button([string]$Name){
 $s=State;if(-not $s.ok){throw $s.error.message}
 if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|switch|checkbox) '+[regex]::Escape($Name)+'\s*$')){throw "Missing $Name"}
 $r=& (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function Query([string]$Text){
 $app.Refresh();$w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $e=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
 $e.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
 [void][SeekRestartKeys]::SetForegroundWindow($app.MainWindowHandle)
 return Wait-Record {$_.query -eq $Text}
}
try{
 $ready=Wait-Record {$_.status.ready -and -not $_.status.catching_up -and $null -ne $_.entry_to_present_ms}
 $state=State
 if($saved.default_view -ne 'icons' -or $state.result.snapshot.treeText -match 'Sort by Name'){throw 'Saved Icons view did not restore'}
 if($ready.status.entries -ne 2072){throw 'Saved indexed locations did not restore'}
 $q=Query "'Nested";if($q.rows.Count -ne 0){throw 'Saved exclusion did not restore'}
 $q=Query "'Preview.mp4"
 [void][SeekRestartKeys]::PostMessageW($app.MainWindowHandle,0x100,[IntPtr]40,[IntPtr]0)
 [void][SeekRestartKeys]::PostMessageW($app.MainWindowHandle,0x101,[IntPtr]40,[IntPtr]0)
 [void](Wait-Record {$_.interaction -eq 'select' -and $_.detail.name -eq 'Preview.mp4'} $q.applied_entry_ms)
 Button 'List · Ctrl+1';[void](Query '!child !hidden')
 Button 'Settings';Button 'Dark';Button 'Light';Button 'Blue';Button 'Done'
 $last=Records|Where-Object interaction -eq select|Select-Object -Last 1
 if($last.detail.name -ne 'Preview.mp4'){throw 'Settings clicks changed the selected result underneath'}
 # Rebuild is an explicit user action on this tiny owned fixture only.
 $before=(Records|Where-Object {$_.status.ready -and -not $_.status.catching_up}|Select-Object -Last 1)
 Button 'Settings';Button 'Rebuild current index';Button 'Done'
 $rebuilt=Wait-Record {$_.status.ready -and -not $_.status.catching_up -and $_.status.catch_up_ms -ne $before.status.catch_up_ms} $before.applied_entry_ms
 $memory=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($app.Id)"
 $settings=Get-Content (Join-Path $data 'settings.json') -Raw -Encoding UTF8|ConvertFrom-Json
 $close=$null
 if(-not $KeepOpen){$watch=[Diagnostics.Stopwatch]::StartNew();[void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Close did not quit'};$app.WaitForExit();$close=@{ms=$watch.Elapsed.TotalMilliseconds;exit_code=$app.ExitCode};if($app.ExitCode -ne 0){throw 'Nonzero exit'}}
 [ordered]@{executable=$exe;sha256=(Get-FileHash $exe).Hash;pid=$app.Id;trace=$trace;fixture=$previous.fixture;saved_preferences_restored=$true;indexed_locations_restored=$true;exclusions_restored=$true;default_icons_restored=$true;explicit_rebuild_completed=$true;rebuild_catch_up_ms=$rebuilt.status.catch_up_ms;settings_clicks_preserve_selection=$true;helper_modified=$false;entry_to_correct_present_ms=$ready.entry_to_present_ms;catch_up_ms=$ready.status.catch_up_ms;private_working_set_bytes=[long]$memory.WorkingSetPrivate;private_commit_bytes=[long]$memory.PrivateBytes;close=$close;settings=$settings}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Output -Encoding UTF8
 Write-Output "Restart and settings hit-testing checks passed: $Output (PID $($app.Id))"
}catch{
 [ordered]@{pid=$app.Id;fixture=$previous.fixture;failure=$_.Exception.Message}|ConvertTo-Json|Set-Content -LiteralPath $Output -Encoding UTF8
 throw
}finally{if(-not $KeepOpen -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
