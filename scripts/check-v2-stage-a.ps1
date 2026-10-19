param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [string]$Output='docs/benchmarks/v2-stage-a-ui.json'
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class SeekDailyWindow {
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int t,bool repaint);
 [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
}
'@
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$base=[IO.Path]::GetFullPath('.bench-data/v2-stage-a-check-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $base 'files';$data=Join-Path $base 'index';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $root,$data,(Join-Path $root 'Destination'),(Join-Path $root 'Nested/Child') -Force|Out-Null
foreach($name in @('Alpha.txt','Beta.md','Photo.jpeg','Other.jpg','Picture.png','Report.pdf','Code.rs','Archive.zip','Movie.mp4','Song.mp3','Drag.txt','Clash.txt')){
 [IO.File]::WriteAllText((Join-Path $root $name),('Owned Seek Stage A fixture: '+$name))
}
[IO.File]::WriteAllText((Join-Path $root 'Nested/Child/deep.txt'),'Nested fixture')
[IO.File]::WriteAllText((Join-Path $root 'Destination/Clash.txt'),'Destination original - must not change')
Copy-Item -LiteralPath app/assets/spike.png -Destination (Join-Path $root 'Preview.png')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"dark","glass":true,"accent":"#909090","show_hidden":true}')
$exe=(Resolve-Path -LiteralPath $Executable).Path
$checks=[Collections.Generic.List[object]]::new()
$captures=[Collections.Generic.List[string]]::new()
$keys=New-Object -ComObject WScript.Shell
$app=$null
function State {
 $s=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
 if(-not $s.ok){throw $s.error.message};$s
}
function Click([string]$name,[string]$button='left') {
 $s=State
 if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|menu item|row|edit) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$')){throw ('Missing '+$name)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --mouse-button $button --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function NativeKey([string]$chord) {
 $null=State
 if(-not $keys.AppActivate($app.Id)){throw 'Cannot focus test Seek'}
 Start-Sleep -Milliseconds 60
 $keys.SendKeys($chord)
}
function SetEdit([string]$name,[string]$value) {
 $null=State
 $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $edit=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
 if(-not $edit){throw ('Missing edit '+$name)}
 $edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($value)
}
function Records {
 $stream=[IO.File]::Open($trace,'Open','Read','ReadWrite');$reader=[IO.StreamReader]::new($stream)
 try{$text=$reader.ReadToEnd()}finally{$reader.Dispose()}
 foreach($line in $text.Split("`n")){if($line.Trim()){try{$line|ConvertFrom-Json}catch{}}}
}
function WaitFor([scriptblock]$test,[int]$seconds=12) {
 $deadline=[DateTime]::UtcNow.AddSeconds($seconds)
 do{$app.Refresh();if($app.HasExited){throw ('Seek exited '+$app.ExitCode)};if(& $test){return};Start-Sleep -Milliseconds 70}while([DateTime]::UtcNow -lt $deadline)
 throw 'Timed out waiting for assertion'
}
function Check([string]$name,[scriptblock]$test) {
 try {WaitFor $test} catch {throw ($name+': '+$_.Exception.Message)}
 $checks.Add(@{name=$name;passed=$true});Write-Output ('PASS '+$name)
}
function Capture([string]$name) {
 $s=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --json|ConvertFrom-Json
 if(-not $s.ok -or -not $s.result.screenshot.path){throw 'Capture failed'}
 $path=[IO.Path]::GetFullPath('.impeccable/review/v2-stage-a-'+$name+'.png')
 New-Item -ItemType Directory -Path (Split-Path $path) -Force|Out-Null
 Copy-Item -LiteralPath $s.result.screenshot.path -Destination $path
 $captures.Add($path)
}
function SelectFile([string]$name){
 # A UIA row selection can change selection without giving its table keyboard
 # focus. Use a real pointer click from fresh native bounds for shortcut tests.
 $s=State
 $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $row=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
 if(-not $row){throw ('Missing row '+$name)}
 $r=$row.Current.BoundingRectangle;$win=$w.Current.BoundingRectangle
 $result=& $driver computer click --app ('pid:'+$app.Id) --x ([int]($r.Left-$win.Left+[Math]::Min(70,$r.Width/2))) --y ([int]($r.Top-$win.Top+$r.Height/2)) --no-screenshot --json|ConvertFrom-Json
 if(-not $result.ok){throw $result.error.message}
}
try {
 $app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
 $retained=$app.Handle
 WaitFor {if(Test-Path $trace){Records|Where-Object {$_.status.ready -and -not $_.status.catching_up}}} 60
 $app.Refresh()
 Click $root
 Check 'Folder scope lists indexed folders and files' {(State).result.snapshot.treeText -match 'row Destination'}
 Click 'Search files and folders'
 SetEdit 'Search files and folders' 'jpe'
 Check 'One JPEG suggestion, with a separate Name option' {
  $tree=(State).result.snapshot.treeText
  ([regex]::Matches($tree,'menu item Type: JPEG image')).Count -eq 1 -and $tree -match 'Name contains'
 }
 Click 'Type: JPEG image'
 Check 'JPEG filter matches both jpg and jpeg' {
  $last=Records|Where-Object {$_.status}|Select-Object -Last 1
  $last.query -eq 'ext:jpeg,jpg ' -and $last.rows.Count -eq 2 -and $last.rows -contains 'Other.jpg' -and $last.rows -contains 'Photo.jpeg'
 }
 Capture 'type-dark'
 Click 'Type: JPEG image'
 Click 'Any'
 Check 'Chip dropdown removes the type filter' {(Records|Where-Object {$_.status}|Select-Object -Last 1).query.Trim() -eq ''}
 Click 'File actions'
 Click 'Largest folders'
 Check 'Largest folders has an explicit return control' {(State).result.snapshot.treeText -match 'button All items'}
 Click 'All items'
 Click 'File actions'
 Click 'Largest files'
 Check 'Largest files can return through the actions menu' {(State).result.snapshot.treeText -match 'button All items'}
 Click 'File actions'
 Click 'All items'
 Check 'All items restores the ordinary view' {(State).result.snapshot.treeText -notmatch 'button All items'}
 SelectFile 'Alpha.txt'
 Click 'Alpha.txt' 'right'
 Check 'GPUI row menu exposes file actions' {
  $tree=(State).result.snapshot.treeText
  $tree -match 'menu item Open with' -and $tree -match 'menu item Properties' -and $tree -match 'menu item Copy path in quotes' -and $tree -match 'menu item Move to Recycle Bin'
 }
 NativeKey '{ESC}'
 SelectFile 'Alpha.txt'
 NativeKey '^c'
 Check 'List Ctrl+C writes Windows file clipboard' {[Windows.Forms.Clipboard]::GetFileDropList().Contains((Join-Path $root 'Alpha.txt'))}
 & powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/check-v2-stage-a-clipboard.ps1 -Executable $exe
 if($LASTEXITCODE -ne 0){throw 'Native Explorer clipboard check failed'}
 Check 'File clipboard interoperates with native Explorer' {(Get-Content docs/benchmarks/v2-stage-a-explorer-clipboard.json -Raw|ConvertFrom-Json).passed}
 $null=State
 NativeKey '{F2}'
 Check 'F2 opens Rename' {(State).result.snapshot.treeText -match 'edit File or folder name'}
 [void][SeekDailyWindow]::MoveWindow($app.MainWindowHandle,180,100,1200,760,$true)
 Check 'Rename input and both buttons fit the restored window' {
  $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle);$bounds=$w.Current.BoundingRectangle
  $save=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Save'))
  $save -and $bounds.Contains($save.Current.BoundingRectangle)
 }
 Capture 'rename-restored'
 SetEdit 'File or folder name' 'Renamed.txt'
 Click 'Save'
 Check 'Rename updates the file and list' {(Test-Path (Join-Path $root 'Renamed.txt')) -and (State).result.snapshot.treeText -match 'row Renamed.txt'}
 Click 'File actions'
 Click 'New folder'
 SetEdit 'File or folder name' 'Created'
 Click 'Save'
 Check 'New folder updates the file and list' {(Test-Path (Join-Path $root 'Created')) -and (State).result.snapshot.treeText -match 'row Created'}
 SelectFile 'Renamed.txt'
 NativeKey '^c'
 Check 'Copy after rename uses the new path' {[Windows.Forms.Clipboard]::GetFileDropList().Contains((Join-Path $root 'Renamed.txt'))}
 # Open a folder through the file menu, without invoking a file association.
 Click $root
 SelectFile 'Destination'
 Click 'File actions';Click 'Open'
 NativeKey '{DOWN}'
 NativeKey '^v'
 Check 'Paste copies into the current folder' {(Test-Path (Join-Path $root 'Destination/Renamed.txt')) -and (Test-Path (Join-Path $root 'Renamed.txt'))}
 Click $root
 SelectFile 'Clash.txt';NativeKey '^c'
 Check 'Collision source is on the file clipboard' {[Windows.Forms.Clipboard]::GetFileDropList().Contains((Join-Path $root 'Clash.txt'))}
 SelectFile 'Destination';Click 'File actions';Click 'Open';NativeKey '^v'
 Check 'Windows asks before replacing an existing file' {(State).result.snapshot.treeText -match 'button Replace the file in the destination'}
 Click 'Skip this file'
 Check 'Skipping the collision preserves both files' {
  [IO.File]::ReadAllText((Join-Path $root 'Destination/Clash.txt')) -eq 'Destination original - must not change' -and
  [IO.File]::ReadAllText((Join-Path $root 'Clash.txt')) -eq 'Owned Seek Stage A fixture: Clash.txt'
 }
 Check 'Windows collision dialog returns to Seek' {(State).result.snapshot.treeText -match 'button File actions'}
 Click $root
 SelectFile 'Beta.md';NativeKey '^x'
 Check 'Cut writes the selected file to the clipboard' {[Windows.Forms.Clipboard]::GetFileDropList().Contains((Join-Path $root 'Beta.md'))}
 SelectFile 'Created';Click 'File actions';Click 'Open'
 NativeKey '{DOWN}';NativeKey '^v'
 Check 'Cut and paste move the file' {(Test-Path (Join-Path $root 'Created/Beta.md')) -and -not(Test-Path (Join-Path $root 'Beta.md'))}
 Click $root
 $s=State;$tree=$s.result.snapshot.treeText
 if($tree -notmatch '(?m)^\s*(\d+) row Drag.txt\s*$'){throw 'Drag source missing'};$from=$Matches[1]
 if($tree -notmatch '(?m)^\s*(\d+) row Destination\s*$'){throw 'Drag target missing'};$to=$Matches[1]
 $drag=& $driver computer drag --app ('pid:'+$app.Id) --from-element-index $from --to-element-index $to --no-screenshot --json|ConvertFrom-Json
 if(-not $drag.ok){throw $drag.error.message}
 Check 'Typed row drag moves to the folder row' {(Test-Path (Join-Path $root 'Destination/Drag.txt')) -and -not(Test-Path (Join-Path $root 'Drag.txt'))}
 SelectFile 'Destination';Click 'File actions';Click 'Open'
 $s=State;$tree=$s.result.snapshot.treeText
 if($tree -notmatch '(?m)^\s*(\d+) row Drag.txt\s*$'){throw 'Sidebar drag source missing'};$from=$Matches[1]
 if($tree -notmatch ('(?m)^\s*(\d+) button '+[regex]::Escape($root)+'\s*$')){throw 'Sidebar drag target missing'};$to=$Matches[1]
 $drag=& $driver computer drag --app ('pid:'+$app.Id) --from-element-index $from --to-element-index $to --no-screenshot --json|ConvertFrom-Json
 if(-not $drag.ok){throw $drag.error.message}
 Check 'Typed row drag moves to the sidebar location' {(Test-Path (Join-Path $root 'Drag.txt')) -and -not(Test-Path (Join-Path $root 'Destination/Drag.txt'))}
 Click $root
 SelectFile 'Renamed.txt';NativeKey '{DELETE}'
 Check 'Delete moves the owned file to the Recycle Bin and updates the list' {-not(Test-Path (Join-Path $root 'Renamed.txt')) -and (State).result.snapshot.treeText -notmatch 'row Renamed.txt'}
 Click 'Search files and folders'
 SetEdit 'Search files and folders' 'owned text clipboard'
 NativeKey '^a';NativeKey '^c'
 Check 'Search Ctrl+C acts on text' {[Windows.Forms.Clipboard]::ContainsText() -and [Windows.Forms.Clipboard]::GetText() -eq 'owned text clipboard' -and -not [Windows.Forms.Clipboard]::ContainsFileDropList()}
 SetEdit 'Search files and folders' ''
 NativeKey '{ESC}'
 Click $root
 SelectFile 'Preview.png'
 Click ('Gallery '+[char]183+' Ctrl+3')
 Check 'Real image uses the large unframed preview request' {
  Records|Where-Object {$_.interaction -eq 'preview_ready' -and $_.detail.name -eq 'Preview.png' -and $_.detail.edge -eq 768 -and $_.detail.image}
 }
 Capture 'preview-release-dark'
 Start-Sleep -Seconds 2
 $settledReplies=@(Records|Where-Object {$_.interaction -eq 'preview_ready'}).Count
 Start-Sleep -Seconds 2
 Check 'Gallery thumbnail batches settle without repeated requests' {@(Records|Where-Object {$_.interaction -eq 'preview_ready'}).Count -eq $settledReplies}
 Click ('List '+[char]183+' Ctrl+1')
 Click 'Settings';Click 'Light';Click 'Done'
 Click 'File actions';Click 'Type';Capture 'actions-light'
 NativeKey '{ESC}';NativeKey '{ESC}'
 Click 'Search files and folders';SetEdit 'Search files and folders' 'jpeg'
 Check 'Light suggestions contain one JPEG choice' {([regex]::Matches((State).result.snapshot.treeText,'menu item Type: JPEG image')).Count -eq 1}
 Capture 'suggestions-light'
 Click 'Type: JPEG image';Click 'Type: JPEG image';Capture 'type-light';Click 'Any'
 Click 'Settings';Click 'Dark';Click 'Done'
 Capture 'list-dark'
 [void]$app.CloseMainWindow()
 if(-not $app.WaitForExit(10000)){throw 'Close did not quit'}
 $app.WaitForExit();if($app.ExitCode -ne 0){throw ('Nonzero exit '+$app.ExitCode)}
 $checks.Add(@{name='Closing the window quits the process';passed=$true})
} catch {
 $checks.Add(@{name=$_.Exception.Message;passed=$false})
 throw
} finally {
 if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(10000)}
 @{executable=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;fixture=$base;checks=@($checks.ToArray());captures=@($captures.ToArray());keyboard='WScript native keyboard delivery; every action verified independently. Orca hotkey delivery of Ctrl+C was unasserted.';scope='Owned fixtures only. Preview.png is a real bundled image; other media extensions are filter/menu fixtures. Native regression separately verifies a large real bitmap and folder artwork.'}|ConvertTo-Json -Depth 9|Set-Content -LiteralPath $Output -Encoding UTF8
}
