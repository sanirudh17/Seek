param([string]$Executable='target/x86_64-pc-windows-msvc/debug/seek.exe')
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
 public static void Drag(int sx,int sy,int dx,int dy){
  SetCursorPos(sx,sy);Thread.Sleep(150);mouse_event(2,0,0,0,UIntPtr.Zero);
  try {Thread.Sleep(150);for(int i=1;i<=30;i++){SetCursorPos(sx+(dx-sx)*i/30,sy+(dy-sy)*i/30);Thread.Sleep(30);}Thread.Sleep(1500);}
  finally {mouse_event(4,0,0,0,UIntPtr.Zero);}
 }
}
'@
[void][SeekOleProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath('.bench-data/v2-stage-b-ole-'+[DateTime]::UtcNow.Ticks)
$root=Join-Path $base 'Source';$dest=Join-Path $base ('SeekDropTarget'+[DateTime]::UtcNow.Ticks);$data=Join-Path $base 'index';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $root,$dest,$data -Force|Out-Null
$source=Join-Path $root 'Drag-out.txt';[IO.File]::WriteAllText($source,'Owned Seek native drag experiment')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"dark","glass":true,"show_hidden":true}')
$exe=(Resolve-Path -LiteralPath $Executable).Path
$app=Start-Process -FilePath $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"'),'--experiment-drag-out') -WindowStyle Normal -PassThru -RedirectStandardError (Join-Path $base 'stderr.log')
$retained=$app.Handle;$explorer=$null
try {
 $until=[DateTime]::UtcNow.AddSeconds(20);$row=$null
 do {$app.Refresh();if($app.HasExited){throw 'Seek exited'};if($app.MainWindowHandle -ne 0){$window=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle);$row=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Drag-out.txt'))};if(-not $row){Start-Sleep -Milliseconds 100}}while(-not $row -and [DateTime]::UtcNow -lt $until)
 if(-not $row){throw 'Owned drag row did not appear'}
 Start-Process explorer.exe -ArgumentList ('/n,"'+$dest+'"') -WindowStyle Normal
 $until=[DateTime]::UtcNow.AddSeconds(15)
 do {
  $explorer=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,([IO.Path]::GetFileName($dest)+' - File Explorer')))
  if(-not $explorer){Start-Sleep -Milliseconds 100}
 }while(-not $explorer -and [DateTime]::UtcNow -lt $until)
 if(-not $explorer){throw 'Unique Explorer target did not appear'}
 $screen=[Windows.Forms.Screen]::PrimaryScreen.Bounds
 $eh=[IntPtr]$explorer.Current.NativeWindowHandle
 [void][SeekOleProbe]::MoveWindow($eh,1220,40,[Math]::Max(360,$screen.Width-1220),700,$true)
 [void][SeekOleProbe]::MoveWindow($app.MainWindowHandle,0,40,1200,700,$true)
 [void][SeekOleProbe]::SetForegroundWindow($app.MainWindowHandle);Start-Sleep -Milliseconds 250
 # Fresh native bounds after window movement; the target is a new empty owned folder.
 $window=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $row=$window.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Drag-out.txt'))
 $r=$row.Current.BoundingRectangle;$target=$explorer.Current.BoundingRectangle
 [SeekOleProbe]::Drag([int]($r.Left+70),[int]($r.Top+$r.Height/2),[int]($target.Left+250),[int]($target.Top+350))
 Start-Sleep -Seconds 2
 $copy=Join-Path $dest 'Drag-out.txt'
 $passed=(Test-Path -LiteralPath $copy) -and ((Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $copy).Hash)
 $events=@();if(Test-Path -LiteralPath $trace){foreach($line in Get-Content -LiteralPath $trace){try {$item=$line|ConvertFrom-Json;if($item.interaction -like 'ole_*'){$events+=$item}}catch{}}}
 $report=@{utc=[DateTime]::UtcNow.ToString('o');executable=$exe;sha256=(Get-FileHash -LiteralPath $exe).Hash;fixture=$base;explorer_copy_passed=$passed;source_preserved=(Test-Path -LiteralPath $source);source_bounds=$window.Current.BoundingRectangle.ToString();target_bounds=$explorer.Current.BoundingRectangle.ToString();events=$events;method='Fresh native UIA row/window bounds. One continuous native pointer drag with 1500 ms held over an empty owned Explorer target. Only COPY offered.'}
 $report|ConvertTo-Json -Depth 10|Set-Content docs/benchmarks/v2-stage-b-drag-experiment.json -Encoding UTF8
 $report|ConvertTo-Json -Depth 10
} finally {
 if($explorer -and $explorer.Current.Name -eq ([IO.Path]::GetFileName($dest)+' - File Explorer')){[void][SeekOleProbe]::PostMessageW([IntPtr]$explorer.Current.NativeWindowHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
 if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(10000)}
}
