param([string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',[string]$Output='docs/benchmarks/stage3-settings-ui.json')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$fixture=[IO.Path]::GetFullPath('.bench-data/settings-check-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $fixture 'files';$data=Join-Path $fixture 'index'
New-Item -ItemType Directory -Force -Path $root,(Join-Path $root 'Excluded'),$data|Out-Null
[IO.File]::WriteAllText((Join-Path $root 'Notes.txt'),'Seek settings fixture.')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"light","glass":true,"accent":"#707070","show_hidden":true}')
$exe=(Resolve-Path $Executable).Path
$checks=[Collections.Generic.List[object]]::new()
function State { & $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json }
function Activate([string]$name){
 $pattern='(?m)^\s*(\d+) (?:button|switch|checkbox|menuitem|menu item|radio button|radiobutton|tab) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$'
 $deadline=[DateTime]::UtcNow.AddSeconds(3)
 do{$s=State;$found=$s.result.snapshot.treeText -match $pattern;if(-not $found){Start-Sleep -Milliseconds 30}}while(-not $found -and [DateTime]::UtcNow -lt $deadline)
 if(-not $found){throw ('Missing control: '+$name)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function Element([string]$name){$w=[Windows.Automation.AutomationElement]::FromHandle($script:hwnd);$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))}
function WaitSaved([scriptblock]$condition){$deadline=[DateTime]::UtcNow.AddSeconds(5);do{try{$script:saved=Get-Content (Join-Path $data 'settings.json') -Raw -Encoding UTF8|ConvertFrom-Json;if(& $condition $script:saved){return}}catch{};Start-Sleep -Milliseconds 30}while([DateTime]::UtcNow -lt $deadline);throw 'Saved settings did not match'}
function SetField([string]$name,[string]$value){$s=State;if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) edit '+[regex]::Escape($name)+'(?: .*|,.*)?$')){throw ('Missing field: '+$name)};$r=& $driver computer set-value --app ('pid:'+$app.Id) --element-index $Matches[1] --value $value --no-screenshot --json|ConvertFrom-Json;if(-not $r.ok){throw $r.error.message}}
function Capture([string]$name){Start-Sleep -Milliseconds 200;& powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-seek-window.ps1 -ProcessId $app.Id -Output ('.impeccable/review/settings-'+$name+'.png')}
function StartSeek {
 $script:app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"')) -WindowStyle Normal -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do{Start-Sleep -Milliseconds 100;$s=State;if([DateTime]::UtcNow -gt $deadline){throw 'Settings test window did not start'}}while($s.result.snapshot.treeText -notmatch '(?m)^\s*\d+ button Settings\s*$')
 $script:hwnd=[IntPtr]$s.result.snapshot.window.id
}
try {
 StartSeek
 Activate 'Settings';Activate 'Appearance';Activate 'Graphite'
 WaitSaved {param($v)$v.accent -eq '#909090'}
 Capture 'appearance-light'
 Activate 'Choose accent color';Start-Sleep -Milliseconds 150
 $s=State
 if($s.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) (?:radio button|radiobutton) (#[0-9A-Fa-f]{6})(?:,.*)?\s*$'){throw 'Color picker palette did not open'}
 Capture 'picker-light'
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw 'Palette color activation failed'}
 WaitSaved {param($v)$v.accent -ne '#909090' -and $v.accent -match '^#[0-9a-f]{6}$'}
 $picked=$saved.accent
 $checks.Add(@{name='Working color picker persists a palette selection';passed=$true;accent=$picked})
 SetField 'Hex color' '#445566';Activate 'Apply';WaitSaved {param($v)$v.accent -eq '#445566'}
 Activate 'Graphite';WaitSaved {param($v)$v.accent -eq '#909090'}
 Activate 'Glass: on';WaitSaved {param($v)-not $v.glass}
 Activate 'Glass: off';WaitSaved {param($v)$v.glass}
 $checks.Add(@{name='Custom hex, softened Graphite and glass switch persist';passed=$true})
 Activate 'Files and search';Activate 'Hidden files: shown';Activate 'Hidden';WaitSaved {param($v)-not $v.show_hidden}
 Activate 'Hidden files: hidden';Activate 'Shown';WaitSaved {param($v)$v.show_hidden}
 Activate 'Icons';WaitSaved {param($v)$v.default_view -eq 'icons'}
 Activate 'Gallery';WaitSaved {param($v)$v.default_view -eq 'gallery'}
 Activate 'List';WaitSaved {param($v)$v.default_view -eq 'list'}
 $bounds=(Element 'Default view options').Current.BoundingRectangle;$last=(Element 'Gallery').Current.BoundingRectangle
 if($bounds.Right-$last.Right -gt 3 -or [Math]::Abs($bounds.Y-$last.Y) -gt 3){throw 'View selector frame extends beyond Gallery'}
 Capture 'files-light'
 $checks.Add(@{name='Hidden menu and all views persist; frame ends at Gallery';passed=$true;group_width=$bounds.Width;trailing_gap_px=$bounds.Right-$last.Right})
 Activate 'Locations';Capture 'locations-light'
 Activate 'All drives (default)';Activate 'Custom folders'
 SetField 'Full folder paths' $root;SetField 'C:\path; C:\other' (Join-Path $root 'Excluded')
 Activate 'Save locations';WaitSaved {param($v)$v.indexed_locations.Count -eq 1 -and $v.exclusions.Count -eq 1}
 $checks.Add(@{name='Custom locations and exclusions persist';passed=$true})
 Activate 'Helper'
 $helperControl=Element 'Uninstall helper'
 if(-not $helperControl){$helperControl=Element 'Install helper'}
 if(-not $helperControl -or $helperControl.Current.BoundingRectangle.Width -gt 250){throw 'Helper action is missing or stretched across its card'}
 Capture 'helper-light'
 $checks.Add(@{name='Compact helper action displayed without install or uninstall';passed=$true;button_width=$helperControl.Current.BoundingRectangle.Width})
 Activate 'Appearance';Activate 'Dark';WaitSaved {param($v)$v.theme -eq 'dark'};Capture 'appearance-dark'
 Activate 'Files and search';Capture 'files-dark'
 Activate 'Locations';Capture 'locations-dark'
 Activate 'Helper';Capture 'helper-dark'
 $checks.Add(@{name='All four redesigned sections captured in Light and Dark';passed=$true})
 Activate 'Done'
 $memory=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($app.Id)"
 $watch=[Diagnostics.Stopwatch]::StartNew();[void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Close did not exit'};$app.WaitForExit();if($app.ExitCode -ne 0){throw 'Nonzero exit'}
 $close=@{ms=$watch.Elapsed.TotalMilliseconds;exit=$app.ExitCode}
 StartSeek;Activate 'Settings';Activate 'Appearance'
 $hex=Element 'Hex color'
 if(-not $hex -or $hex.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -ne '#909090'){throw 'Saved Graphite did not restore'}
 Activate 'Locations'
 $paths=Element 'Full folder paths'
 if(-not $paths -or $paths.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value -ne $root){throw 'Saved custom location did not restore'}
 Activate 'Done';[void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Restart close did not exit'}
 $checks.Add(@{name='Saved preferences restore after complete process exit';passed=$true;close=$close})
 @{executable=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;fixture=$fixture;checks=$checks;private_working_set_bytes=[long]$memory.WorkingSetPrivate;private_commit_bytes=[long]$memory.PrivateBytes;close=$close;helper_modified=$false}|ConvertTo-Json -Depth 10|Set-Content $Output -Encoding UTF8
 Write-Output ('PASS '+$checks.Count+' settings checks')
} catch {
 try{Capture 'failure';(State)|ConvertTo-Json -Depth 12|Set-Content .references/settings-failure-state.json -Encoding UTF8}catch{}
 @{executable=$exe;fixture=$fixture;checks=$checks;failure=$_.Exception.Message}|ConvertTo-Json -Depth 10|Set-Content $Output -Encoding UTF8
 throw
} finally {if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
