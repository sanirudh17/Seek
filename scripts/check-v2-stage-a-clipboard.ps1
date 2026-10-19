param([string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SeekClipboardCheck {
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]public static extern bool PostMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
}
'@
$paths=[Windows.Forms.Clipboard]::GetFileDropList()
$fixturePrefix=(Resolve-Path -LiteralPath '.bench-data').Path+'\v2-stage-a-check-'
if($paths.Count -ne 1 -or -not $paths[0].StartsWith($fixturePrefix,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($paths[0]) -ne 'Alpha.txt'){throw 'Expected the owned Seek CF_HDROP clipboard'}
$source=$paths[0]
$leaf='ExplorerPaste'+[DateTime]::UtcNow.Ticks
$dest=Join-Path ([IO.Path]::GetDirectoryName([IO.Path]::GetDirectoryName($source))) $leaf
New-Item -ItemType Directory -Path $dest|Out-Null
$target=Join-Path $dest 'Alpha.txt'
Start-Process explorer.exe -ArgumentList ('/n,"'+$dest+'"') -WindowStyle Normal
$deadline=[DateTime]::UtcNow.AddSeconds(15)
$window=$null
do {
 Start-Sleep -Milliseconds 200
 $window=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,($leaf+' - File Explorer')))
}while(-not $window -and [DateTime]::UtcNow -lt $deadline)
if(-not $window){throw 'Owned Explorer window not observed through native UIA'}
$hwnd=[IntPtr]$window.Current.NativeWindowHandle
try {
 [void][SeekClipboardCheck]::SetForegroundWindow($hwnd)
 Start-Sleep -Milliseconds 100
 if([SeekClipboardCheck]::GetForegroundWindow() -ne $hwnd){throw 'Explorer is not foreground'}
 (New-Object -ComObject WScript.Shell).SendKeys('^v')
 $deadline=[DateTime]::UtcNow.AddSeconds(10)
 while(-not(Test-Path -LiteralPath $target) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
 $passed=(Test-Path -LiteralPath $target) -and (Get-FileHash -LiteralPath $source).Hash -eq (Get-FileHash -LiteralPath $target).Hash
 @{passed=$passed;source=$source;destination=$target;sha256=(Get-FileHash -LiteralPath $Executable).Hash;method='Seek CF_HDROP, independently read by Windows Clipboard API, pasted with native Ctrl+V into a unique empty Explorer folder; foreground HWND checked and copied SHA256 compared. UIA fallback avoids an Orca Explorer catalog handoff failure.'}|ConvertTo-Json|Set-Content docs/benchmarks/v2-stage-a-explorer-clipboard.json -Encoding UTF8
 if(-not $passed){throw 'Native Explorer paste not verified'}
 Write-Output 'PASS Native Explorer paste has identical bytes'
} finally {
 $current=[Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,($leaf+' - File Explorer')))
 if($current){[void][SeekClipboardCheck]::PostMessageW([IntPtr]$current.Current.NativeWindowHandle,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
}
