param([string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe',[ValidateSet('Explorer','Browser','Internal')][string]$Target='Explorer',[string]$Output='docs/benchmarks/v2-drag-followup.json',[switch]$PasteOnly)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;using System.Runtime.InteropServices;using System.Threading;
public static class SeekOleProbe {
 [DllImport("user32.dll")]public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")]public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int t,bool repaint);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")]public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 public static void Drag(int sx,int sy,int dx,int dy,bool cancel=false){
  SetCursorPos(sx,sy);Thread.Sleep(150);mouse_event(2,0,0,0,UIntPtr.Zero);
  try {Thread.Sleep(150);for(int i=1;i<=30;i++){SetCursorPos(sx+(dx-sx)*i/30,sy+(dy-sy)*i/30);Thread.Sleep(30);}Thread.Sleep(1500);if(cancel){keybd_event(27,0,0,UIntPtr.Zero);keybd_event(27,0,2,UIntPtr.Zero);Thread.Sleep(200);}}
  finally {mouse_event(4,0,0,0,UIntPtr.Zero);}
 }
}
'@
[void][SeekOleProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath('.bench-data/v2-stage-b-ole-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $base 'Source';$dest=Join-Path $base ('SeekDropTarget'+[DateTime]::UtcNow.Ticks);$data=Join-Path $base 'index';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $root,$dest,$data -Force|Out-Null
New-Item -ItemType Directory -Path (Join-Path $root 'Destination') -Force|Out-Null
if($Target -eq 'Browser'){[IO.File]::WriteAllText((Join-Path $root 'Second.txt'),'Second owned drag fixture')}
$source=Join-Path $root 'Drag-out.txt';[IO.File]::WriteAllText($source,'Owned Seek native drag experiment')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"dark","glass":true,"show_hidden":true}')
$exe=(Resolve-Path -LiteralPath $Executable).Path
$app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
$retained=$app.Handle;$explorer=$null;$server=$null;$chrome=$null;$checks=@()
try {
 $until=[DateTime]::UtcNow.AddSeconds(20);$row=$null
 do {$app.Refresh();if($app.HasExited){throw 'Seek exited'};if($app.MainWindowHandle -ne 0){$window=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle);$row=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Drag-out.txt'))};if(-not $row){Start-Sleep -Milliseconds 100}}while(-not $row -and [DateTime]::UtcNow -lt $until)
 if(-not $row){throw 'Owned drag row did not appear'}
 if($Target -eq 'Explorer') {
  Start-Process explorer.exe -ArgumentList ('/n,"'+$dest+'"') -WindowStyle Normal
  $targetName=[IO.Path]::GetFileName($dest)+' - File Explorer'
 } elseif($Target -eq 'Browser') {
  $server=Start-Process python -ArgumentList @('scripts/drag-upload-fixture.py',('"'+$base+'"')) -WindowStyle Hidden -PassThru
  $until=[DateTime]::UtcNow.AddSeconds(10)
  while(-not(Test-Path (Join-Path $base 'server.json')) -and [DateTime]::UtcNow -lt $until){Start-Sleep -Milliseconds 50}
  $port=(Get-Content (Join-Path $base 'server.json') -Raw|ConvertFrom-Json).port
  $chrome=Start-Process 'C:\Program Files\Google\Chrome\Application\chrome.exe' -ArgumentList @('--user-data-dir="'+(Join-Path $base 'browser-profile')+'"','--no-first-run','--no-default-browser-check',('--app=http://127.0.0.1:'+$port)) -WindowStyle Normal -PassThru
  $targetName='Seek owned drag upload check'
 } else {
  $targetName=$null
 }
 if($targetName){
  $until=[DateTime]::UtcNow.AddSeconds(15)
  do {
   $explorer=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$targetName))
   if(-not $explorer){Start-Sleep -Milliseconds 100}
  }while(-not $explorer -and [DateTime]::UtcNow -lt $until)
  if(-not $explorer){throw "Unique $Target target did not appear"}
 }
 $screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds
 $eh=if($explorer){[IntPtr]$explorer.Current.NativeWindowHandle}else{[IntPtr]::Zero}
 if($explorer){[void][SeekOleProbe]::MoveWindow($eh,1220,40,[Math]::Max(360,$screen.Width-1220),700,$true)}
 [void][SeekOleProbe]::MoveWindow($app.MainWindowHandle,0,40,1200,700,$true)
 [void][SeekOleProbe]::SetForegroundWindow($app.MainWindowHandle);Start-Sleep -Milliseconds 250
 # Fresh native bounds after window movement; the target is a new empty owned folder.
 $window=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $row=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Drag-out.txt'))
 function Find([string]$name){
  $win=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
  $node=$win.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
  if(-not $node){throw "Missing fresh UIA element: $name"};return $node
 }
 function ClickNode([string]$name){
  [void][SeekOleProbe]::SetForegroundWindow($app.MainWindowHandle);Start-Sleep -Milliseconds 180
  $r=(Find $name).Current.BoundingRectangle
  [void][SeekOleProbe]::SetCursorPos([int]($r.Left+$r.Width/2),[int]($r.Top+$r.Height/2));[SeekOleProbe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[SeekOleProbe]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 300
 }
 $targetRect=if($explorer){$explorer.Current.BoundingRectangle}else{$window.Current.BoundingRectangle}
 if($Target -eq 'Internal') {
  ClickNode $root
  $r=(Find 'Drag-out.txt').Current.BoundingRectangle;$d=(Find 'Destination').Current.BoundingRectangle
  [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($d.Left+70),[int]($d.Top+$d.Height/2))
  Start-Sleep -Milliseconds 600
  $moved=(Test-Path (Join-Path $root 'Destination/Drag-out.txt')) -and -not(Test-Path $source)
  $checks+=@{name='Internal folder-row move';passed=$moved}
  if($moved){
   ClickNode 'Destination';ClickNode 'File actions';ClickNode 'Open';Start-Sleep -Milliseconds 400
   $r=(Find 'Drag-out.txt').Current.BoundingRectangle;$d=(Find $root).Current.BoundingRectangle
   [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($d.Left+70),[int]($d.Top+$d.Height/2))
   Start-Sleep -Milliseconds 600
   $checks+=@{name='Internal sidebar move';passed=((Test-Path $source) -and -not(Test-Path (Join-Path $root 'Destination/Drag-out.txt')))}
  }
 } elseif($Target -eq 'Browser') {
  $keys=New-Object -ComObject WScript.Shell
  if(-not $PasteOnly){
  foreach($entry in @(@('input',210),@('drop',400))){
   $r=(Find 'Drag-out.txt').Current.BoundingRectangle
   [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($targetRect.Left+220),[int]($targetRect.Top+[int]$entry[1]))
   Start-Sleep -Milliseconds 650
   $records=@();if(Test-Path (Join-Path $base 'browser-records.ndjson')){$records=@(Get-Content (Join-Path $base 'browser-records.ndjson')|ForEach-Object{$_|ConvertFrom-Json})}
   $record=$records|Where-Object {$_.kind -eq $entry[0]}|Select-Object -Last 1
   $checks+=@{name=('Browser '+$entry[0]+' receives actual file bytes');passed=([bool]$record -and $record.files[0].name -eq 'Drag-out.txt' -and $record.files[0].sha256 -eq (Get-FileHash $source).Hash.ToLowerInvariant())}
  }
  ClickNode 'Drag-out.txt';$keys=New-Object -ComObject WScript.Shell;$keys.SendKeys('^a');Start-Sleep -Milliseconds 250
  $r=(Find 'Drag-out.txt').Current.BoundingRectangle
  [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($targetRect.Left+220),[int]($targetRect.Top+400))
  Start-Sleep -Milliseconds 650
  $records=@(Get-Content (Join-Path $base 'browser-records.ndjson')|ForEach-Object{$_|ConvertFrom-Json});$last=$records[-1]
  $multi=$last.files.Count -eq 2
  foreach($file in $last.files){$multi=$multi -and $file.sha256 -eq (Get-FileHash (Join-Path $root $file.name)).Hash.ToLowerInvariant()}
  $checks+=@{name='Browser multi-selection transfers both actual files';passed=$multi}
  $before=$records.Count
  $r=(Find 'Drag-out.txt').Current.BoundingRectangle
  [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($targetRect.Left+220),[int]($targetRect.Top+400),$true)
  Start-Sleep -Milliseconds 500
  $records=@(Get-Content (Join-Path $base 'browser-records.ndjson')|ForEach-Object{$_|ConvertFrom-Json})
  $checks+=@{name='Escape cancels without delivering files';passed=($records.Count -eq $before)}
  }
  $driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
  $state=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
  if($state.result.snapshot.treeText -notmatch '(?m)^\s*(\d+) row Drag-out.txt\s*$'){throw 'Fresh clipboard source row missing'}
  $null=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json
  foreach($caption in @('File actions','Copy')){
   $state=& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json
   if(-not $state.ok){throw $state.error.message}
   if($state.result.snapshot.treeText -notmatch ('(?m)^\s*(\d+) (?:button|menu item) '+[regex]::Escape($caption)+'\s*$')){throw ('Missing clipboard menu '+$caption)}
   $action=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
   if(-not $action.ok){throw $action.error.message}
  }
  Start-Sleep -Milliseconds 250
  $paths=@([Windows.Forms.Clipboard]::GetFileDropList()|ForEach-Object {$_.Replace('\\?\','')})
  if($paths -notcontains $source){throw 'File menu Copy did not produce the owned clipboard path'}
  $browserState=& $driver computer get-app-state --app ('pid:'+$explorer.Current.ProcessId) --restore-window --json|ConvertFrom-Json
  $browserState|ConvertTo-Json -Depth 14|Set-Content (Join-Path $base 'browser-before-paste.json') -Encoding UTF8

  [void][SeekOleProbe]::SetForegroundWindow($eh)
  [void][SeekOleProbe]::SetCursorPos([int]($targetRect.Left+220),[int]($targetRect.Top+400));[SeekOleProbe]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[SeekOleProbe]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  if(-not $keys.AppActivate($explorer.Current.ProcessId)){throw 'Browser target did not focus'};Start-Sleep -Milliseconds 200
  $keys.SendKeys('^v');Start-Sleep -Milliseconds 750
  $until=[DateTime]::UtcNow.AddSeconds(3);while(-not(Test-Path (Join-Path $base 'browser-records.ndjson')) -and [DateTime]::UtcNow -lt $until){Start-Sleep -Milliseconds 100}
  $records=@();if(Test-Path (Join-Path $base 'browser-records.ndjson')){$records=@(Get-Content (Join-Path $base 'browser-records.ndjson')|ForEach-Object{$_|ConvertFrom-Json})}
  $pasted=$records|Where-Object {$_.kind -eq 'paste'}|Select-Object -Last 1
  $pasteOK=[bool]$pasted -and $pasted.files.Count -ge 1
  foreach($file in $pasted.files){$pasteOK=$pasteOK -and $file.sha256 -eq (Get-FileHash (Join-Path $root $file.name)).Hash.ToLowerInvariant()}
  $checks+=@{name='Seek file clipboard pastes actual bytes into browser paste target';passed=$pasteOK}

 } else {
  $r=$row.Current.BoundingRectangle
  [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($targetRect.Left+250),[int]($targetRect.Top+350))
  Start-Sleep -Milliseconds 650
  $copy=Join-Path $dest 'Drag-out.txt'
  $checks+=@{name='Explorer receives identical file copy';passed=((Test-Path -LiteralPath $copy) -and ((Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $copy).Hash))}
 }
 $passed=@($checks|Where-Object {-not $_.passed}).Count -eq 0
 $events=@();if(Test-Path -LiteralPath $trace){foreach($line in Get-Content -LiteralPath $trace){try {$item=$line|ConvertFrom-Json;if($item.interaction -like 'ole_*'){$events+=$item}}catch{}}}
 $report=@{utc=[DateTime]::UtcNow.ToString('o');executable=$exe;sha256=(Get-FileHash -LiteralPath $exe).Hash;fixture=$base;target=$Target;checks=$checks;passed=$passed;source_preserved=(Test-Path -LiteralPath $source);source_bounds=$window.Current.BoundingRectangle.ToString();target_bounds=$targetRect.ToString();events=$events;browser_records=if($Target -eq "Browser"){$records}else{@()};method='Fresh UIA row/window bounds; native held-pointer drags (1500 ms target hold). Owned folder moves, Explorer copy, or loopback Chromium input/DataTransfer file-byte SHA256 verification according to target. External COPY only; browser cancellation uses Escape before button release.'}
 $report|ConvertTo-Json -Depth 10|Set-Content $Output -Encoding UTF8
 $report|ConvertTo-Json -Depth 10
} finally {
 if($explorer -and $explorer.Current.Name -eq $targetName){[void][SeekOleProbe]::PostMessageW([IntPtr]$explorer.Current.NativeWindowHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
 if($server -and -not $server.HasExited){Stop-Process -Id $server.Id}
 if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(10000)}
 if(-not $app.HasExited){throw 'Seek did not exit after drag checks'}
 if($report){$report.close_exit_code=$app.ExitCode;$report.close_passed=($app.ExitCode -eq 0);$report|ConvertTo-Json -Depth 10|Set-Content $Output -Encoding UTF8}
}
