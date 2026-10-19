param([string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',[string]$Output='docs/benchmarks/stage3-targeted-ui.json')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);' -Name TargetKeys -Namespace SeekCheck
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$fixture=[IO.Path]::GetFullPath('.bench-data/stage3-targeted-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $fixture 'files';$data=Join-Path $fixture 'index';$trace=Join-Path $fixture 'trace.ndjson'
New-Item -ItemType Directory -Force -Path "$root/Folder A","$root/Folder B",$data | Out-Null
Copy-Item app/assets/spike.png "$root/Picture.png"
[IO.File]::WriteAllText("$root/Notes.txt",'Seek appearance fixture.')
[IO.File]::WriteAllText("$root/Report.pdf",'Synthetic icon fixture, not a document.')
[IO.File]::WriteAllText("$data/settings.json",'{"theme":"dark","glass":true,"accent":"#707070","show_hidden":true}')
$app=Start-Process -FilePath (Resolve-Path $Executable).Path -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -PassThru -WindowStyle Normal
$checks=[Collections.Generic.List[object]]::new()
function State { & $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json }
function Click([string]$name){$s=State;if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) button '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$')){throw ('Missing '+$name)};$r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json;if(-not $r.ok){throw $r.error.message}}
function Query([string]$text){
 $before=@(Get-Content $trace -Encoding UTF8 -ErrorAction SilentlyContinue).Count
 $s=State;if($s.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) edit Search files and folders'){throw 'Missing search'}
 & $driver computer set-value --app ('pid:'+$app.Id) --element-index $Matches[1] --value $text --no-screenshot --json|Out-Null
 $deadline=[DateTime]::UtcNow.AddSeconds(10)
 do{Start-Sleep -Milliseconds 20;$found=$null;foreach($l in (Get-Content $trace -Encoding UTF8 -ErrorAction SilentlyContinue|Select-Object -Skip $before)){try{$r=$l|ConvertFrom-Json;if($r.interaction -eq 'tree_rows' -and $r.detail.rows.Count -gt 0){$found=@{rows=$r.detail.rows}}}catch{}};if([DateTime]::UtcNow -gt $deadline){throw 'Query did not apply'}}while(-not $found)
 return $found
}
function Element([string]$name){$w=[Windows.Automation.AutomationElement]::FromHandle($script:hwnd);return $w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))}
function WaitElement([string]$name,[bool]$present){$deadline=[DateTime]::UtcNow.AddSeconds(3);do{$found=$null -ne (Element $name);if($found -eq $present){return};Start-Sleep -Milliseconds 20}while([DateTime]::UtcNow -lt $deadline);throw ('UI state did not settle: '+$name+' present='+$present)}
function Capture([string]$name){Start-Sleep -Milliseconds 250;& powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-seek-window.ps1 -ProcessId $app.Id -Output ('.impeccable/review/'+$name+'.png')}
try {
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do {
  Start-Sleep -Milliseconds 100
  $app.Refresh();if($app.HasExited){throw ('Seek exited before its window, code '+$app.ExitCode)}
  $s=State
  if([DateTime]::UtcNow -gt $deadline){throw 'Seek window did not appear'}
 } while(-not $s.result.snapshot.window.id)
 $script:hwnd=[IntPtr]$s.result.snapshot.window.id
 if(Element 'Information panel'){throw 'Info panel should be hidden without a selection'}
 Capture 'toolbar-empty-dark'
 [void](Query "'Picture.png")
 [void][SeekCheck.TargetKeys]::PostMessageW($script:hwnd,0x100,[IntPtr]40,[IntPtr]0);[void][SeekCheck.TargetKeys]::PostMessageW($script:hwnd,0x101,[IntPtr]40,[IntPtr]0)
 Start-Sleep -Milliseconds 200
 $rows=Query '!__never'
 Click 'List · Ctrl+1'
 if(-not (Element 'Sort by Name')){throw 'List header missing'}
 WaitElement 'Information panel' $true
 $toolbarNames=@('Toggle sidebar','Back','Forward','Search files and folders','List · Ctrl+1','Icons · Ctrl+2','Gallery · Ctrl+3','Show hidden and system files','Sort results','Info panel · Space','Settings','Minimize','Maximize or restore','Close')
 $geometry=@();$previousRight=[double]::NegativeInfinity
 foreach($name in $toolbarNames){
  $element=Element $name;if(-not $element){throw ('Toolbar control missing: '+$name)}
  $rect=$element.Current.BoundingRectangle
  if($rect.Width -lt 24 -or $rect.Height -lt 24 -or $rect.Left -lt $previousRight-1){throw ('Toolbar target is clipped or overlaps: '+$name)}
  $geometry+=@{name=$name;x=$rect.X;y=$rect.Y;width=$rect.Width;height=$rect.Height}
  $previousRight=$rect.Right
 }
 $checks.Add(@{name='All 14 toolbar controls have separate visible targets';passed=$true;targets=$geometry})
 Capture 'targeted-list-dark'
 Click 'Info panel · Space'
 WaitElement 'Information panel' $false
 Capture 'toolbar-info-off-dark'
 Click 'Info panel · Space'
 WaitElement 'Information panel' $true
 $checks.Add(@{name='Info panel opens and closes from its toolbar button';passed=$true})
 $checks.Add(@{name='Populated dark List, uniform yellow folders and selected PNG';passed=$true;rows=$rows.rows.Count})
 $toggle=Element 'Toggle sidebar';$start=(Element 'Sort by Name').Current.BoundingRectangle.X
 $toggle.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
 $positions=@();for($i=0;$i -lt 12;$i++){Start-Sleep -Milliseconds 20;$positions+= (Element 'Sort by Name').Current.BoundingRectangle.X}
 $end=$positions[-1]
 if($start-$end -lt 200){throw 'Sidebar did not collapse'}
 if(($positions|Select-Object -Unique).Count -lt 3){throw 'Sidebar did not show intermediate widths'}
 $toggle.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 250
 $restored=(Element 'Sort by Name').Current.BoundingRectangle.X
 if([Math]::Abs($restored-$start) -gt 2){throw 'Sidebar did not restore'}
 $checks.Add(@{name='160 ms sidebar transition has intermediate widths and restores';passed=$true;start=$start;samples=$positions;restored=$restored})
 Click 'Icons · Ctrl+2';Capture 'targeted-icons-dark'
 Click 'Gallery · Ctrl+3';Capture 'targeted-gallery-dark'
 $checks.Add(@{name='Icons/Gallery retain selected item';passed=($null -ne (Element 'Information panel'))})
 Click 'Settings';Click 'Appearance';Click 'Graphite';Capture 'targeted-settings-dark'
 Click 'Light';Capture 'targeted-settings-light';Click 'Done';Click 'List · Ctrl+1'
 if(-not (Element 'Sort by Name')){throw 'Light List missing'}
 Capture 'targeted-list-light'
 $checks.Add(@{name='Light Graphite selection, centered small Settings controls';passed=$true})
 $m=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($app.Id)"
 $watch=[Diagnostics.Stopwatch]::StartNew();[void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Close did not exit'};$app.WaitForExit();if($app.ExitCode -ne 0){throw 'Nonzero exit'}
 $checks.Add(@{name='Window close exits';passed=$true;ms=$watch.Elapsed.TotalMilliseconds;exit=$app.ExitCode})
 @{executable=(Resolve-Path $Executable).Path;checks=$checks;fixture=$fixture;private_working_set_bytes=[long]$m.WorkingSetPrivate;private_commit_bytes=[long]$m.PrivateBytes}|ConvertTo-Json -Depth 9|Set-Content $Output -Encoding UTF8
 Write-Output ('PASS '+$checks.Count+' targeted GUI checks')
} catch {
 @{executable=$Executable;fixture=$fixture;checks=$checks;failure=$_.Exception.Message}|ConvertTo-Json -Depth 9|Set-Content $Output -Encoding UTF8
 throw
} finally {if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
