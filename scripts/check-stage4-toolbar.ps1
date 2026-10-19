param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [string]$Output='docs/benchmarks/stage4-closeout-toolbar.json'
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SeekToolbarNative {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h,ref Point p);
 public static void Key(IntPtr h,int k){PostMessageW(h,0x100,new IntPtr(k),IntPtr.Zero);PostMessageW(h,0x101,new IntPtr(k),IntPtr.Zero);}
 public static void Press(IntPtr h,int x,int y,bool down){
  var p=new Point{X=x,Y=y};ScreenToClient(h,ref p);var at=new IntPtr((p.Y<<16)|(p.X&0xffff));
  SendMessageW(h,0x200,IntPtr.Zero,at);SendMessageW(h,down?0x201u:0x202u,new IntPtr(down?1:0),at);
 }
 public static void Scale(IntPtr h,int dpi){
  var r=new Rect{Left=0,Top=0,Right=960*dpi/96,Bottom=550*dpi/96};
  var p=Marshal.AllocHGlobal(Marshal.SizeOf(r));
  try {Marshal.StructureToPtr(r,p,false);SendMessageW(h,0x2e0,new IntPtr(dpi|(dpi<<16)),p);}
  finally {Marshal.FreeHGlobal(p);}
 }
}
'@
$previous=[SeekToolbarNative]::SetThreadDpiAwarenessContext([IntPtr](-4))
$driver=(Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe')
$exe=(Resolve-Path $Executable).Path
$fixture=[IO.Path]::GetFullPath('.bench-data/toolbar-finish-'+[DateTime]::UtcNow.Ticks)
$checks=[Collections.Generic.List[object]]::new();$samples=[Collections.Generic.List[object]]::new()
$actualDpi=0;$app=$null
function State {& $driver computer get-app-state --app ('pid:'+$app.Id) --restore-window --no-screenshot --json|ConvertFrom-Json}
function Activate([string]$name){
 $s=State
 $pattern='(?m)^\s*(\d+) (?:button|switch|checkbox|toggle button) '+[regex]::Escape($name)+'(?:, Secondary Actions:.*)?\s*$'
 if($s.result.snapshot.treeText -notmatch $pattern){throw ('Missing '+$name)}
 $r=& $driver computer click --app ('pid:'+$app.Id) --element-index $Matches[1] --no-screenshot --json|ConvertFrom-Json
 if(-not $r.ok){throw $r.error.message}
}
function Element([string]$name){
 $w=[Windows.Automation.AutomationElement]::FromHandle($hwnd)
 $w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
}
function Capture([string]$name){
 $path='.impeccable/review/finish-'+$theme+'-'+$name+'.png'
 & powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-seek-window.ps1 -ProcessId $app.Id -Output $path|Out-Null
 $path
}
function SameFill([string]$name){
 Start-Sleep -Milliseconds 150
 $path=Capture $name.Replace(' ','-')
 $win=New-Object SeekToolbarNative+Rect
 [void][SeekToolbarNative]::GetWindowRect($hwnd,[ref]$win)
 $bitmap=[Drawing.Bitmap]::new([IO.Path]::GetFullPath($path))
 try{
  $colors=@()
  foreach($label in @($listLabel,$name)){
   $b=(Element $label).Current.BoundingRectangle
   $x=[int]($b.X-$win.Left+5*$actualDpi/96);$y=[int]($b.Y-$win.Top+5*$actualDpi/96)
   $c=$bitmap.GetPixel($x,$y);$colors+=@($c.R,$c.G,$c.B) -join ','
  }
  $a=$colors[0].Split(',');$b=$colors[1].Split(',');$delta=0
  for($i=0;$i -lt 3;$i++){$delta=[Math]::Max($delta,[Math]::Abs([int]$a[$i]-[int]$b[$i]))}
  if($delta -gt 2){throw ($name+' active fill differs from List: '+($colors -join ' / '))}
  $checks.Add(@{name=($theme+' '+$name+' matches active List fill');passed=$true;reference_rgb=$colors[0];control_rgb=$colors[1];max_channel_delta=$delta;tolerance=2})
 }finally{$bitmap.Dispose()}
}
try{
 foreach($theme in @('light','dark')){
  $root=Join-Path $fixture ($theme+'/files');$data=Join-Path $fixture ($theme+'/index')
  New-Item -ItemType Directory -Force -Path $root,$data|Out-Null
  [IO.File]::WriteAllText((Join-Path $root 'Notes.txt'),'Toolbar and DPI validation.')
  [IO.File]::WriteAllText((Join-Path $data 'settings.json'),('{"theme":"'+$theme+'","glass":false,"accent":"#4878c0","show_hidden":false}'))
  $app=Start-Process $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"')) -WindowStyle Normal -PassThru
  $handle=$app.Handle;$deadline=[DateTime]::UtcNow.AddSeconds(20)
  do{Start-Sleep -Milliseconds 80;$s=State;if([DateTime]::UtcNow -gt $deadline){throw 'Toolbar did not start'}}while($s.result.snapshot.treeText -notmatch 'Sort results')
  $hwnd=$app.MainWindowHandle;$actualDpi=[SeekToolbarNative]::GetDpiForWindow($hwnd)
  $listLabel='List '+[char]0xb7+' Ctrl+1';$infoLabel='Info panel '+[char]0xb7+' Space'
  [SeekToolbarNative]::Key($hwnd,40)
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{Start-Sleep -Milliseconds 40;$e=Element $infoLabel;$on=$e.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [Windows.Automation.ToggleState]::On}while(-not $on -and [DateTime]::UtcNow -lt $deadline)
  if(-not $on){throw 'No selected-item inspector'}
  SameFill $infoLabel
  Activate 'Show hidden and system files';SameFill 'Show hidden and system files'
  Activate 'Sort results';SameFill 'Sort results';Activate 'Sort results'
  $s=State;$settingsBounds=(Element 'Settings').Current.BoundingRectangle
  $sx=[int]($settingsBounds.X+$settingsBounds.Width/2);$sy=[int]($settingsBounds.Y+$settingsBounds.Height/2)
  [SeekToolbarNative]::Press($hwnd,$sx,$sy,$true)
  try{SameFill 'Settings'}finally{[SeekToolbarNative]::Press($hwnd,$sx,$sy,$false)}
  Activate 'Done'
  foreach($dpi in @(96,120,144,192)){
   [SeekToolbarNative]::Scale($hwnd,$dpi);Start-Sleep -Milliseconds 180
   $s=State;$bounds=@()
   foreach($label in @('Toggle sidebar','Back','Forward',$listLabel,('Icons '+[char]0xb7+' Ctrl+2'),('Gallery '+[char]0xb7+' Ctrl+3'),'Show hidden and system files','Sort results',$infoLabel,'Settings','Minimize','Maximize or restore','Close')){
    $b=(Element $label).Current.BoundingRectangle
    if($b.Width -le 0 -or $b.Height -le 0){throw ('Missing bounds at DPI '+$dpi+': '+$label)}
    $bounds+=@{name=$label;x=$b.X;y=$b.Y;width=$b.Width;height=$b.Height}
   }
   $ordered=@($bounds|Sort-Object {[double]$_.x})
   for($i=1;$i -lt $ordered.Count;$i++){if($ordered[$i].x -lt $ordered[$i-1].x+$ordered[$i-1].width-1){throw ('Toolbar overlaps at DPI '+$dpi)}}
   $width=($bounds|Where-Object name -eq $listLabel).width
   if([Math]::Abs($width-32*$dpi/96) -gt 2){throw ('DPI handler did not update rendered target size: '+$width)}
   $path=Capture ('dpi-'+$dpi)
   $samples.Add(@{theme=$theme;simulated_dpi=$dpi;os_window_dpi=[SeekToolbarNative]::GetDpiForWindow($hwnd);bounds=$bounds;capture=$path})
  }
  [SeekToolbarNative]::Scale($hwnd,$actualDpi)
  [void]$app.CloseMainWindow();if(-not $app.WaitForExit(10000)){throw 'Close did not exit'};$app.WaitForExit()
  if($app.ExitCode -ne 0){throw 'Close failed'}
 }
 $checks.Add(@{name='Both themes retain nonoverlapping toolbar targets through 100/125/150/200 percent renderer transitions';passed=$true})
 @{executable=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;fixture=$fixture;actual_dpi=$actualDpi;checks=$checks;samples=$samples;note='WM_DPICHANGED is sent only to disposable Seek windows; Windows display settings are untouched. These are renderer/handler simulations, not physical mixed-DPI or HDR hardware certification.'}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 Write-Output ('PASS '+$checks.Count+' toolbar/display checks')
}catch{
 @{executable=$exe;checks=$checks;samples=$samples;failure=$_.Exception.Message}|ConvertTo-Json -Depth 8|Set-Content $Output -Encoding UTF8
 throw
}finally{
 if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(10000)}
 [void][SeekToolbarNative]::SetThreadDpiAwarenessContext($previous)
}
