param(
 [string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',
 [string]$Output='docs/benchmarks/search-followup-ui.json',
 [switch]$FinishOnly,
 [switch]$VisualOnly,
 [switch]$DriveAlias,
 [switch]$ColdIndex,
 [switch]$SmokeOnly
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$driver=Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe'
$base=[IO.Path]::GetFullPath('.bench-data/task1-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $base 'files';$data=Join-Path $base 'fresh-data';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $root,$data,(Join-Path $root 'Empty'),(Join-Path $root 'OneDrive/Documents') -Force|Out-Null
[IO.File]::WriteAllText((Join-Path $root 'OneDrive/Documents/redirected.txt'),'owned redirected fixture')
[IO.File]::WriteAllText((Join-Path $root 'top.txt'),'owned metadata fixture')
$bulk=Join-Path $root 'Bulk';New-Item -ItemType Directory -Path $bulk|Out-Null
for($i=0;$i -lt 3500;$i++){[void][IO.Directory]::CreateDirectory((Join-Path $bulk ('d'+$i)))}
if($ColdIndex){$root=(Resolve-Path -LiteralPath target).Path}
$taskDrive=$null
if($DriveAlias){
 $used=@([IO.DriveInfo]::GetDrives()|ForEach-Object {$_.Name.Substring(0,1)})
 $taskDrive=@('Z','Y','X','W','V','U','T')|Where-Object {$_ -notin $used}|Select-Object -First 1
 if(-not $taskDrive){throw 'No free temporary drive letter'}
 & subst.exe ($taskDrive+':') $root
 if($LASTEXITCODE -ne 0){throw 'Owned drive alias could not be created'}
 $root=$taskDrive+':\'
 [IO.File]::WriteAllText((Join-Path $data 'settings.json'),(@{indexed_locations=@($root)}|ConvertTo-Json))
}

$exe=(Resolve-Path -LiteralPath $Executable).Path
$matrix=[Collections.Generic.List[object]]::new()
$checks=[Collections.Generic.List[object]]::new();$captures=[Collections.Generic.List[string]]::new();$keys=New-Object -ComObject WScript.Shell;$app=$null
function State {
 $s=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
 if(-not $s.ok){throw $s.error.message};$s
}
function Click([string]$name,[string]$button='left') {
 $s=State
 $suffix=if($name -eq 'Search files and folders'){'(?:[ ,][^\r\n]*)?'}else{'(?:, Secondary Actions:.*)?'}
 if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|menu item|row|edit) '+[regex]::Escape($name)+$suffix+'\s*$')){throw ('Missing '+$name)}
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
 if(-not(Test-Path -LiteralPath $trace)){return}
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
 $path=[IO.Path]::GetFullPath('.impeccable/review/v2-stage-b-'+$name+'.png')
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

Add-Type @'
using System;using System.Runtime.InteropServices;
public static class Task1Known {
 [DllImport("shell32.dll")]static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
 public static string Read(string value){Guid id=new Guid(value);IntPtr p;int hr=SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out p);if(hr<0)return null;try{return Marshal.PtrToStringUni(p);}finally{Marshal.FreeCoTaskMem(p);}}
}
'@
$known=@(
 @('Downloads','374DE290-123F-4565-9164-39C4925E467B'),@('Desktop','B4BFCC3A-DB2C-424C-B029-7FE99A87C641'),
 @('Documents','FDD39AD0-238F-46AF-ADB4-6C85480369C7'),@('Pictures','33E28130-4E1E-4676-835A-98395C3BC3BB'),
 @('Videos','18989B1D-99B5-455B-841C-AB7C74E4DDFC'),@('Music','4BD8D571-6D19-48D3-BE97-422220080E43'))
function Independent([string]$folder){@(Get-ChildItem -LiteralPath $folder -Force -ErrorAction Stop|Where-Object {($_.Attributes -band ([IO.FileAttributes]::Hidden -bor [IO.FileAttributes]::System)) -eq 0}|ForEach-Object {$_.Name}|Sort-Object)}
function CompareFolder([string]$button,[string]$folder){
 $expected=@(Independent $folder);$prior=(Records|Where-Object {$null -ne $_.generation}|Select-Object -Last 1).generation
 Click $button
 WaitFor { $r=Records|Where-Object {$null -ne $_.generation -and $_.generation -gt $prior -and $_.query -eq ''}|Select-Object -Last 1; $r -and @($r.rows).Count -eq $expected.Count -and @(Compare-Object $expected @($r.rows|Sort-Object)).Count -eq 0 }
 $r=Records|Where-Object {$null -ne $_.generation}|Select-Object -Last 1
 if($expected.Count -eq 0){Check 'Empty folder has a completed empty state' {(State).result.snapshot.treeText.Contains('This folder has no visible items.')}}
 $matrix.Add(@{button=$button;path=$folder;expected=$expected.Count;actual=@($r.rows).Count;missing=0;extra=0;index_ready=$r.status.ready;query_to_present_ms=$r.query_to_present_ms})
 Write-Output ('PASS independent directory truth: '+$button+' ('+$expected.Count+')')
}
try {
 $launchArgs=@('--walker','--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"'))
 if(-not $DriveAlias){$launchArgs+=@('--root',('"'+$root+'"'))}
 $app=Start-Process -FilePath $exe -ArgumentList $launchArgs -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
 $retained=$app.Handle
 WaitFor {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero}
 WaitFor {(State).result.snapshot.treeText.Contains('button '+$root)}
 if(-not $FinishOnly){
 CompareFolder $root $root
 if(-not $DriveAlias){foreach($item in $known){$path=[Task1Known]::Read($item[1]);if($path -and (Test-Path -LiteralPath $path -PathType Container)){CompareFolder $item[0] $path}}}
 }
 if(-not $ColdIndex -and -not $DriveAlias){
 Click $root;WaitFor {(State).result.snapshot.treeText.Contains('row Empty')}
 SelectFile 'Empty';Click 'File actions';Click 'Open'
 WaitFor {(State).result.snapshot.treeText.Contains('This folder has no visible items.')}
 $checks.Add(@{name='Genuinely empty folder is distinguished from loading';passed=$true})
 Click 'File actions';Click 'Search file contents';Click $root
 Check 'Sidebar exits Content mode and restores directory rows' {$t=(State).result.snapshot.treeText; -not $t.Contains('Exit content search') -and $t.Contains('row top.txt')}
 Click 'File actions';Click 'Largest files';Click $root
 Check 'Sidebar exits Largest mode and restores folders' {$t=(State).result.snapshot.treeText;$t.Contains('row Empty') -and -not $t.Contains('button All items')}

 }
 $app.CloseMainWindow()|Out-Null;$app.WaitForExit(10000)|Out-Null
 if(-not $app.HasExited){throw 'Close did not quit'}
 $checks.Add(@{name='Closing quits';passed=$true})
} catch {$checks.Add(@{name=$_.Exception.Message;passed=$false});try{(State).result.snapshot.treeText|Set-Content (Join-Path $base 'failed-tree.txt')}catch{};throw}
finally {
 @{sha256=(Get-FileHash $exe).Hash;fixture=$base;fresh_no_settings=(-not $DriveAlias);drive_alias=[bool]$DriveAlias;checks=@($checks);folders=@($matrix);cold_index=[bool]$ColdIndex;events=@(Records|Where-Object {$_.interaction -in @('folder_metadata','folder_view')})}|ConvertTo-Json -Depth 12|Set-Content $Output -Encoding UTF8
 if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}
 if($taskDrive){& subst.exe ($taskDrive+':') /D|Out-Null}
}
