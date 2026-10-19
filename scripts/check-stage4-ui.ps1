param([string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',[string]$Output='docs/benchmarks/stage4-ui-corrections.json')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$fixture=[IO.Path]::GetFullPath('.bench-data/stage4-ui-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $fixture 'files';$data=Join-Path $fixture 'index'
New-Item -ItemType Directory -Force -Path $root,$data|Out-Null
[IO.File]::WriteAllText((Join-Path $root 'Notes.txt'),'Seek UI regression fixture.')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"light","glass":true,"accent":"#d5d2d0","show_hidden":true}')
$exe=(Resolve-Path $Executable).Path
$checks=[Collections.Generic.List[object]]::new()
function State { & $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json }
function Activate([string]$name){
 $pattern='(?m)^\s*(\d+) (?:button|switch|checkbox|toggle button) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$'
 $deadline=[DateTime]::UtcNow.AddSeconds(3)
 do{$s=State;$found=$s.result.snapshot.treeText -match $pattern;if(-not $found){Start-Sleep -Milliseconds 30}}while(-not $found -and [DateTime]::UtcNow -lt $deadline)
 if(-not $found){throw ('Missing control: '+$name)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function Element([string]$name){$window=[Windows.Automation.AutomationElement]::FromHandle($script:hwnd);$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))}
function WaitView([string]$name){
 $deadline=[DateTime]::UtcNow.AddSeconds(3)
 do{
  $button=Element ($name+' '+[char]0xb7+' Ctrl+'+@{List=1;Icons=2;Gallery=3}[$name])
  $active=$button -and $button.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [Windows.Automation.ToggleState]::On
  if(-not $active){Start-Sleep -Milliseconds 30}
 }while(-not $active -and [DateTime]::UtcNow -lt $deadline)
 if(-not $active){throw ('Open app did not switch to '+$name)}
}
function Capture([string]$name){Start-Sleep -Milliseconds 150;& powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-seek-window.ps1 -ProcessId $app.Id -Output ('.impeccable/review/stage4-'+$name+'.png')}
function SetHex([string]$color){
 $s=State
 if($s.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) edit Hex color(?: .*|,.*)?$'){throw 'Missing hex field'}
 $r=& $driver computer set-value --app ('pid:'+$app.Id) --element-index $Matches[1] --value $color --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
 Activate 'Apply'
}
try {
 $app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"')) -WindowStyle Normal -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do{Start-Sleep -Milliseconds 100;$s=State;if([DateTime]::UtcNow -gt $deadline){throw 'UI test did not start'}}while($s.result.snapshot.treeText -notmatch '(?m)^\s*\d+ button Settings\s*$')
 $script:hwnd=[IntPtr]$s.result.snapshot.window.id
 foreach($view in @('Icons','Gallery','List')){
  Activate 'Settings';Activate 'Files and search';Activate $view;Activate 'Done';WaitView $view
  $saved=Get-Content (Join-Path $data 'settings.json') -Raw -Encoding UTF8|ConvertFrom-Json
  if($saved.default_view -ne $view.ToLower()){throw 'Default view did not persist'}
  $checks.Add(@{name=('Default '+$view+' applies immediately to the already-open app');passed=$true})
 }
 foreach($theme in @('Light','Dark')){
  Activate 'Settings';Activate 'Appearance';Activate $theme
  SetHex '#d5d2d0';Capture ('settings-'+$theme.ToLower())
  Activate 'Files and search';Capture ('selection-'+$theme.ToLower());Activate 'Done';WaitView 'List'
  Capture ('toolbar-'+$theme.ToLower()+'-pale')
  $checks.Add(@{name=($theme+' custom pale accent and stronger settings selections captured');passed=$true})
 }
 foreach($color in @('#ffffff','#000000')){
  Activate 'Settings';Activate 'Appearance';SetHex $color;Activate 'Done';WaitView 'List';Capture ('toolbar-dark-'+$color.TrimStart('#'))
 }
 $checks.Add(@{name='Black and white custom accents keep the active view identifiable';passed=$true;verification='Captured for visual review; active view also verified through the native toggle state.'})
 $watch=[Diagnostics.Stopwatch]::StartNew();[void]$app.CloseMainWindow()
 if(-not $app.WaitForExit(10000)){throw 'Close did not exit'}
 $app.WaitForExit()
 if($app.ExitCode -ne 0){throw 'Close returned a failure'}
 @{sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;executable=$exe;fixture=$fixture;checks=$checks;close_ms=$watch.Elapsed.TotalMilliseconds;exit_code=$app.ExitCode}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 Write-Output ('PASS '+$checks.Count+' UI corrections')
} catch {
 try{Capture 'failure';(State)|ConvertTo-Json -Depth 12|Set-Content .references/stage4-ui-failure.json -Encoding UTF8}catch{}
 @{executable=$exe;fixture=$fixture;checks=$checks;failure=$_.Exception.Message}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 throw
} finally {if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
