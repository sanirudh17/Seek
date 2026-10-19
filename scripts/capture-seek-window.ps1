param([Parameter(Mandatory=$true)][int]$ProcessId,[Parameter(Mandatory=$true)][string]$Output)
# Capture only Seek, including when another application covers its window.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SeekCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder lpClassName, int nMaxCount);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc enumProc, IntPtr lParam);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
 public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
 public static IntPtr FindSeekWindow(uint pid) {
     IntPtr found = IntPtr.Zero;
     EnumWindows((h, l) => {
         uint p;
         GetWindowThreadProcessId(h, out p);
         if (p == pid) {
             var sb = new System.Text.StringBuilder(256);
             GetClassName(h, sb, 256);
             if (sb.ToString() == "Zed::Window") {
                 found = h;
                 return false;
             }
         }
         return true;
     }, IntPtr.Zero);
     return found;
 }
}
'@
$handle = [SeekCapture]::FindSeekWindow([uint32]$ProcessId)
if ($handle -eq [IntPtr]::Zero) {
    $handle=(Get-Process -Id $ProcessId).MainWindowHandle
}
if($handle -eq [IntPtr]::Zero){throw 'Seek has no window'}
$previous=[SeekCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
try {
 $rect=New-Object SeekCapture+Rect
 if(-not [SeekCapture]::GetWindowRect($handle,[ref]$rect)){throw 'Cannot read Seek bounds'}
 $bitmap=New-Object Drawing.Bitmap ($rect.Right-$rect.Left),($rect.Bottom-$rect.Top)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 $dc=$graphics.GetHdc()
 try { if(-not [SeekCapture]::PrintWindow($handle,$dc,2)){throw 'Seek capture failed'} }
 finally { $graphics.ReleaseHdc($dc) }
 $bitmap.Save([IO.Path]::GetFullPath($Output),[Drawing.Imaging.ImageFormat]::Png)
 $graphics.Dispose();$bitmap.Dispose()
} finally { [void][SeekCapture]::SetThreadDpiAwarenessContext($previous) }
