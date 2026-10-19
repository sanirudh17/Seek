param(
 [string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',
 [string]$Output='.references/task2-ui.json',
 [switch]$KeepCaptures
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
if(-not(Test-Path '.bench-data/content-accuracy/files/Structured.pdf')){python scripts/make-content-accuracy-fixture.py .bench-data/content-accuracy/files}
Copy-Item -LiteralPath '.bench-data/content-accuracy/files/Structured.pdf' -Destination (Join-Path $allowed 'Probe.pdf')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),(@{theme='dark';glass=$true;accent='#909090';show_hidden=$true;content_locations=@($allowed)}|ConvertTo-Json))
foreach($name in @('t3','t3-notes.md','hash-t3-build.o','unrelated.txt')){[IO.File]::WriteAllText((Join-Path $root $name),'owned name fixture')}
for($n=0;$n -lt 35;$n++){Copy-Item -LiteralPath (Join-Path $allowed 'Probe.pdf') -Destination (Join-Path $allowed ('Page'+$n+'.pdf'))}
for($n=0;$n -lt 250;$n++){[IO.File]::WriteAllText((Join-Path $allowed ('Text'+$n+'.txt')),'quartz')}
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
 Click 'File actions';Click 'Search file contents'
 Click 'Search files and folders';SetEdit 'Search files and folders' 'quartz'
 Check 'Content still finds Markdown, code, UTF16 and DOCX' { $r=Records|Where-Object {$_.query -eq 'quartz' -and $_.rows -contains 'Note.md' -and $_.rows -contains 'Code.rs' -and $_.rows -contains 'Utf16.txt' -and $_.rows -contains 'Letter.docx'}; [bool]$r }
 Check 'Content has a partial presentation before final scan completion' {
  $partial=Records|Where-Object {$_.interaction -eq 'content_partial'}|Select-Object -First 1
  $finish=Records|Where-Object {$_.interaction -eq 'content_search' -and $_.detail.checked -gt 0}|Select-Object -First 1
  $present=Records|Where-Object {$_.query -eq 'quartz' -and $_.rows -contains 'Code.rs' -and $_.applied_entry_ms -ge $partial.entry_ms -and $_.applied_entry_ms -lt $finish.entry_ms}|Select-Object -First 1
  [bool]$partial -and [bool]$finish -and [bool]$present
 }
 SetEdit 'Search files and folders' 'different';SetEdit 'Search files and folders' 'quartz'
 Check 'New content query rejects earlier rows' {$r=Records|Where-Object {$_.query -eq 'quartz' -and $_.rows -contains 'Note.md' -and $_.rows -notcontains 'Other.txt'}|Select-Object -Last 1;[bool]$r}
 Check 'Complete progress has real denominator and no 200-result cutoff' { $r=Records|Where-Object {$_.interaction -eq 'content_search' -and $_.detail.finished}|Select-Object -Last 1; $r.detail.checked -eq $r.detail.total -and $r.detail.matches -gt 200 }
 SetEdit 'Search files and folders' 'quartz again'
 Click 'Cancel'
 Check 'Explicit cancellation preserves partial scan without restart' {Records|Where-Object {$_.interaction -eq 'content_search' -and $_.detail.cancelled}}
 Check 'Cancelled status is visible in the UI' {(State).result.snapshot.treeText.Contains('Cancelled')}
 Check 'Cancel publishes a final update without restarting' { $r=Records|Where-Object {$_.interaction -eq 'content_search' -and $_.detail.cancelled}|Select-Object -Last 1; $r.detail.checked -lt $r.detail.total }
 Capture 'progressive-content' 
 $app.CloseMainWindow()|Out-Null;$app.WaitForExit(10000)|Out-Null
 if(-not $app.HasExited){throw 'Close did not quit'}
 $checks.Add(@{name='Closing quits after content worker use';passed=$true})
} catch {try{(State).result.snapshot.treeText|Set-Content -LiteralPath (Join-Path $base 'failed-tree.txt')}catch{};$checks.Add(@{name=$_.Exception.Message;passed=$false});throw}
finally {
 @{executable=$exe;sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;fixture=$base;checks=@($checks);captures=@($captures);content_observations=@(Records|Where-Object {$_.interaction -in @('content_search','content_partial')})}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
 if($app -and -not $app.HasExited){$app.CloseMainWindow()|Out-Null}
}
