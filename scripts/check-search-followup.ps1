param(
 [string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',
 [string]$Output='docs/benchmarks/search-followup-ui.json',
 [switch]$FinishOnly,
 [switch]$VisualOnly,
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
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$base=[IO.Path]::GetFullPath('.bench-data/v2-stage-b-check-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $base 'files';$data=Join-Path $base 'index';$trace=Join-Path $base 'trace.ndjson'
$allowed=Join-Path $root 'Allowed';$outside=Join-Path $root 'Outside'
New-Item -ItemType Directory -Path $root,$data,$allowed,$outside -Force|Out-Null
[IO.File]::WriteAllText((Join-Path $allowed 'Note.md'),"First line`nquartz body marker")
[IO.File]::WriteAllText((Join-Path $allowed 'Code.rs'),'let quartz = 42;')
[IO.File]::WriteAllText((Join-Path $allowed 'Utf16.txt'),'quartz unicode marker',[Text.Encoding]::Unicode)
[IO.File]::WriteAllText((Join-Path $allowed '.env'),'quartz must stay protected')
[IO.File]::WriteAllText((Join-Path $allowed 'passwords.txt'),'quartz must stay protected')
[IO.File]::WriteAllText((Join-Path $outside 'Outside.txt'),'quartz outside scope')
[IO.File]::WriteAllText((Join-Path $allowed 'quartz-filename.txt'),'no body match')
[IO.File]::WriteAllText((Join-Path $allowed 'Other.txt'),'different marker')
$docx=Join-Path $allowed 'Letter.docx'
$zip=[IO.Compression.ZipFile]::Open($docx,[IO.Compression.ZipArchiveMode]::Create)
try {$part=$zip.CreateEntry('word/document.xml');$writer=[IO.StreamWriter]::new($part.Open());try {$writer.Write('<w:document xmlns:w="word"><w:p><w:r><w:t>quar</w:t></w:r><w:r><w:t>tz DOCX body</w:t></w:r></w:p></w:document>')}finally{$writer.Dispose()}}finally{$zip.Dispose()}
Copy-Item -LiteralPath (Join-Path $env:USERPROFILE 'Downloads/Seek-Stage-B-PDF-Probe/probe.pdf') -Destination (Join-Path $allowed 'Probe.pdf')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),(@{theme='dark';glass=$true;accent='#909090';show_hidden=$true;content_locations=@($allowed)}|ConvertTo-Json))
foreach($name in @('t3','t3-notes.md','hash-t3-build.o','unrelated.txt')){[IO.File]::WriteAllText((Join-Path $root $name),'owned name fixture')}
$exe=(Resolve-Path -LiteralPath $Executable).Path
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

try {
 $app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
 $retained=$app.Handle
 Check 'Fixture is ready' {Records|Where-Object {$_.status.ready -and -not $_.status.catching_up}}
 Click 'Search files and folders';SetEdit 'Search files and folders' 't3'
 Check 'Plain t3 ranks exact filename first' { $r=Records|Where-Object {$_.query -eq 't3' -and $_.rows}|Select-Object -Last 1; $r.rows[0] -eq 't3' }
 NativeKey '{ENTER}'
 Check 'Enter on Name contains keeps exact t3 first' { $r=Records|Where-Object {$_.query -eq 'name:"t3"' -and $_.rows}|Select-Object -Last 1; $r -and $r.rows -and $r.rows[0] -eq 't3' -and @($r.rows|Where-Object {$_ -notlike '*t3*'}).Count -eq 0 }
 Capture 'name-contains-fixed'
 SetEdit 'Search files and folders' 'quartz'
 NativeKey '{ESC}';Click 'File actions';Click 'Search file contents'
 Check 'Content still finds Markdown, code, UTF16 and DOCX' { $t=(State).result.snapshot.treeText;$t.Contains('row Note.md') -and $t.Contains('row Code.rs') -and $t.Contains('row Utf16.txt') -and $t.Contains('row Letter.docx') }
 Check 'Content has a partial presentation before final scan completion' {
  $partial=Records|Where-Object {$_.interaction -eq 'content_partial'}|Select-Object -First 1
  $finish=Records|Where-Object {$_.interaction -eq 'content_search'}|Select-Object -First 1
  $present=Records|Where-Object {$_.query -eq 'quartz' -and $_.rows -contains 'Code.rs' -and $_.applied_entry_ms -ge $partial.entry_ms -and $_.applied_entry_ms -lt $finish.entry_ms}|Select-Object -First 1
  [bool]$partial -and [bool]$finish -and [bool]$present
 }
 SetEdit 'Search files and folders' 'different';SetEdit 'Search files and folders' 'quartz'
 Check 'New content query rejects earlier rows' {$t=(State).result.snapshot.treeText;$t.Contains('row Note.md') -and -not $t.Contains('row Other.txt')}
 Capture 'progressive-content'
 $app.CloseMainWindow()|Out-Null;$app.WaitForExit(10000)|Out-Null
 if(-not $app.HasExited){throw 'Close did not quit'}
 $checks.Add(@{name='Closing quits after content worker use';passed=$true})
} catch {try{(State).result.snapshot.treeText|Set-Content -LiteralPath (Join-Path $base 'failed-tree.txt')}catch{};$checks.Add(@{name=$_.Exception.Message;passed=$false});throw}
finally {
 @{executable=$exe;sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;fixture=$base;checks=@($checks);captures=@($captures);content_observations=@(Records|Where-Object {$_.interaction -in @('content_search','content_partial')})}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
 if($app -and -not $app.HasExited){$app.CloseMainWindow()|Out-Null}
}
