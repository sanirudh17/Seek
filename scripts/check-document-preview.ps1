param(
 [string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',
 [ValidateSet('dark','light')][string]$Theme='dark',
 [string]$Output='.references/task3-ui.json',
 [switch]$Idle,
 [switch]$WordOnly,
 [switch]$SmokeOnly
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
$driver=Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe'
$base=[IO.Path]::GetFullPath('.bench-data/document-ui-'+[DateTime]::UtcNow.Ticks)
$root=(Resolve-Path '.bench-data/document-preview/files').Path
$data=Join-Path $base 'data';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $data -Force|Out-Null
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),(@{theme=$Theme;glass=$true;accent='#909090';show_hidden=$true}|ConvertTo-Json))
$checks=[Collections.Generic.List[object]]::new();$captures=[Collections.Generic.List[string]]::new();$idleSamples=[Collections.Generic.List[object]]::new();$app=$null
function State {
 $s=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
 if(-not $s.ok){throw $s.error.message};$s
}
function Click([string]$name) {
 $s=State
 $suffix=if($name -eq 'Search files and folders'){'(?:[ ,][^\r\n]*)?'}else{'(?:, Secondary Actions:.*)?'}
 if($s.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|row|edit) '+[regex]::Escape($name)+$suffix+'\s*$')){throw ('Missing '+$name)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function Records {
 if(-not(Test-Path -LiteralPath $trace)){return}
 $stream=[IO.File]::Open($trace,'Open','Read','ReadWrite');$reader=[IO.StreamReader]::new($stream)
 try{$text=$reader.ReadToEnd()}finally{$reader.Dispose()}
 foreach($line in $text.Split("`n")){if($line.Trim()){try{$line|ConvertFrom-Json}catch{}}}
}
function WaitFor([scriptblock]$test,[int]$seconds=15) {
 $deadline=[DateTime]::UtcNow.AddSeconds($seconds)
 do{$app.Refresh();if($app.HasExited){throw ('Seek exited '+$app.ExitCode)};if(& $test){return};Start-Sleep -Milliseconds 70}while([DateTime]::UtcNow -lt $deadline)
 throw 'Timed out waiting for assertion'
}
function Check([string]$name,[scriptblock]$test) {
 try{WaitFor $test}catch{throw ($name+': '+$_.Exception.Message)}
 $checks.Add(@{name=$name;passed=$true});Write-Output ('PASS '+$name)
}
function Search([string]$value) {
 Click 'Search files and folders'
 $window=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $edit=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Search files and folders'))
 $edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($value)
 WaitFor {(State).result.snapshot.treeText.Contains('row '+$value)}
 Click $value
}
function Preview([string]$name,[int]$page=0) {
 Records|Where-Object {$_.interaction -eq 'document_preview' -and $_.detail.name -eq $name -and $_.detail.page -eq $page}|Select-Object -Last 1
}
function Capture([string]$name) {
 $s=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --json|ConvertFrom-Json
 if(-not $s.ok -or -not $s.result.screenshot.path){throw 'Capture failed'}
 $path=[IO.Path]::GetFullPath('.impeccable/review/documents-'+$Theme+'-'+$name+'.png')
 New-Item -ItemType Directory -Path (Split-Path $path) -Force|Out-Null
 Copy-Item -LiteralPath $s.result.screenshot.path -Destination $path;$captures.Add($path)
}
function IdleSample([string]$label) {
 $null=State;Start-Sleep -Seconds 5;$app.Refresh()
 $cpu=$app.TotalProcessorTime.TotalMilliseconds;$private=$app.PrivateMemorySize64;$working=$app.WorkingSet64;$threads=$app.Threads.Count
 $watch=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 15;$app.Refresh()
 $idleSamples.Add(@{label=$label;elapsed_ms=$watch.Elapsed.TotalMilliseconds;cpu_ms=$app.TotalProcessorTime.TotalMilliseconds-$cpu;private_before=$private;private_after=$app.PrivateMemorySize64;working_before=$working;working_after=$app.WorkingSet64;threads_before=$threads;threads_after=$app.Threads.Count})
}
try {
 $app=Start-Process -FilePath (Resolve-Path $Executable).Path -ArgumentList @('--trace-frames','--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
 $retained=$app.Handle
 Check 'Fixture index ready' { $app.MainWindowHandle -ne 0 -and (Records|Where-Object {$_.status.ready -and -not $_.status.catching_up}) }
 if($SmokeOnly){
  WaitFor {(State).result.snapshot.treeText.Contains('edit Search files and folders')}
  Search 'Structured.pdf'
  Check 'Production PDF preview' {$r=Preview 'Structured.pdf';$r -and $r.detail.success -and $r.detail.pages -eq 2}
  Search 'Table.xlsx'
  Check 'Production Excel data table' {$r=Preview 'Table.xlsx';$r -and $r.detail.success -and $r.detail.sheet_rows -eq 3}
  Search 'TextOnly.docx'
  Check 'Production Word text fallback' {$r=Preview 'TextOnly.docx';$r -and $r.detail.success -and $r.detail.source -eq 'Word text'}
  $app.CloseMainWindow()|Out-Null;$app.WaitForExit(10000)|Out-Null
  if(-not $app.HasExited){throw 'Close did not quit'}
  $checks.Add(@{name='Production close quits';passed=$true})
  return
 }
 if(-not $WordOnly){
 WaitFor {(State).result.snapshot.treeText.Contains('edit Search files and folders')}
 Search 'Structured.pdf'
 Check 'Windows PDF first page rendered' { $r=Preview 'Structured.pdf';$r -and $r.detail.success -and $r.detail.pages -eq 2 -and (State).result.snapshot.treeText.Contains('Page 1 of 2') }
 Capture 'pdf-first'
 Click 'Next PDF page'
 Check 'Next PDF page rendered' { $r=Preview 'Structured.pdf' 1;$r -and $r.detail.success -and (State).result.snapshot.treeText.Contains('Page 2 of 2') }
 Capture 'pdf-second'
 Click 'Previous PDF page'
 Check 'Previous PDF page returns' {(State).result.snapshot.treeText.Contains('Page 1 of 2')}
 Search 'Table.xlsx'
 Check 'First Excel sheet data table rendered' {$r=Preview 'Table.xlsx';$r -and $r.detail.success -and $r.detail.sheet_rows -eq 3 -and (State).result.snapshot.treeText.Contains('First sheet: First sheet')}
 Capture 'excel'
 }
 Search 'Structured.docx'
 Check 'Word thumbnail or text fallback rendered' {$r=Preview 'Structured.docx';$r -and $r.detail.success -and $r.detail.source -like 'Word*'}
 Capture 'word'
 Search 'TextOnly.docx'
 Check 'Word without thumbnail uses document text' {$r=Preview 'TextOnly.docx';$r -and $r.detail.success -and $r.detail.source -eq 'Word text' -and (State).result.snapshot.treeText.Contains('quartzmarker')}
 Capture 'word-text'
 Search 'Thumbnail.docx'
 Check 'Real nonblank shell thumbnail is used for Word' {$r=Preview 'Thumbnail.docx';$r -and $r.detail.success -and $r.detail.source -eq 'Word thumbnail'}
 Capture 'word-thumbnail'
 Search 'Broken.pdf'
 Check 'Broken PDF clearly unavailable' {$r=Preview 'Broken.pdf';$r -and -not $r.detail.success -and (State).result.snapshot.treeText.Contains('Preview unavailable')}
 Search 'Encrypted.pdf'
 Check 'Encrypted PDF clearly unavailable' {$r=Preview 'Encrypted.pdf';$r -and -not $r.detail.success -and (State).result.snapshot.treeText.Contains('Preview unavailable')}
 Capture 'unavailable'
 Search 'Structured.pdf'
 Check 'Reselecting a document renders again' {(State).result.snapshot.treeText.Contains('Page 1 of 2')}
 if($Idle){IdleSample 'PDF selected'}
 Click 'Info panel · Space'
 Check 'Closing panel clears selected document state' {$r=Records|Where-Object {$_.interaction -eq 'document_request'}|Select-Object -Last 1;$r -and -not $r.detail.active -and -not $r.detail.retained}
 if($Idle){IdleSample 'Panel closed'}
 Check 'No preview child remains after panel closes' {@(Get-CimInstance Win32_Process -Filter "Name='seek.exe'"|Where-Object {$_.ParentProcessId -eq $app.Id -and $_.CommandLine -like '*--document-preview-host*'}).Count -eq 0}
 $app.CloseMainWindow()|Out-Null;$app.WaitForExit(10000)|Out-Null
 if(-not $app.HasExited){throw 'Close did not quit'}
 $checks.Add(@{name='Closing quits';passed=$true})
} catch {try{(State).result.snapshot.treeText|Set-Content -LiteralPath (Join-Path $base 'failed-tree.txt')}catch{};$checks.Add(@{name=$_.Exception.Message;passed=$false});throw}
finally {
 @{theme=$Theme;fixture=$base;checks=@($checks);captures=@($captures);idle=@($idleSamples);previews=@(Records|Where-Object {$_.interaction -eq 'document_preview'})}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
 if($app -and -not $app.HasExited){$app.CloseMainWindow()|Out-Null}
}
