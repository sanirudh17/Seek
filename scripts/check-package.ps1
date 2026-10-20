param([Parameter(Mandatory=$true)][string]$PackageDirectory,
      [string]$Output='docs/benchmarks/package-validation.json')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PackagingKnownFolders {
 [DllImport("shell32.dll")] static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
 public static string Get(string value){var id=new Guid(value);IntPtr p;int hr=SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out p);if(hr<0)return null;try{return Marshal.PtrToStringUni(p);}finally{Marshal.FreeCoTaskMem(p);}}
}
'@
$base=Join-Path $env:TEMP ('Seek-packaging-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $base|Out-Null
[IO.File]::WriteAllText((Join-Path $base '.seek-packaging-fixture'),'owned fixture')
$program=Join-Path $base 'Program';$data=Join-Path $base 'Data';$scope=Join-Path $base 'Scope'
New-Item -ItemType Directory -Path $data,$scope,(Join-Path $scope 'Owned folder')|Out-Null
[IO.File]::WriteAllText((Join-Path $scope 'Package-owned.txt'),'Disposable package fixture')
[IO.File]::WriteAllText((Join-Path $data 'retain-sentinel'),'keep this data unless explicitly approved')
[IO.File]::WriteAllText((Join-Path $data 'document-text-cache-sentinel'),'owned test text')
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"dark","glass":true,"check_updates_on_startup":false,"content_locations":[]}')
$package=(Resolve-Path -LiteralPath $PackageDirectory).Path
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Seek.PackagingTest.'+(Split-Path $base -Leaf).Substring(15)
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$helperBefore=@(Get-Service -Name ('SeekHelper-'+$sid) -ErrorAction SilentlyContinue|ForEach-Object Name)
$checks=@();$app=$null
function Check([string]$name,[bool]$value){if(-not $value){throw $name};$script:checks+=@{name=$name;passed=$true}}
function InvokeUninstall([string]$answer){
 $si=[Diagnostics.ProcessStartInfo]::new()
 $si.FileName=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
 $si.Arguments='-NoLogo -NoProfile -ExecutionPolicy Bypass -File "'+(Join-Path $program 'Uninstall.ps1')+'"'
 $si.UseShellExecute=$false;$si.CreateNoWindow=$true;$si.RedirectStandardInput=$true;$si.RedirectStandardOutput=$true;$si.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($si)
 if($answer -eq 'cancel'){$p.StandardInput.WriteLine('n')}else{
  $p.StandardInput.WriteLine('y')
  if($helperBefore.Count){$p.StandardInput.WriteLine('y')} # Explicitly keep the pre-existing real helper.
  $p.StandardInput.WriteLine($(if($answer -eq 'delete'){'y'}else{''}))
 }
 $p.StandardInput.Close();$stdout=$p.StandardOutput.ReadToEnd();$stderr=$p.StandardError.ReadToEnd()
 if(-not $p.WaitForExit(15000) -or $p.ExitCode -ne 0){throw ('Uninstall failed: '+$stderr)}
 [IO.File]::WriteAllText((Join-Path $base ('uninstall-'+$answer+'.log')),$stdout+$stderr)
}
try{
 & (Join-Path $package 'Install.ps1') -TestRoot $base -NonInteractive -DesktopShortcut
 $m=Get-Content (Join-Path $program 'install-manifest.json') -Raw|ConvertFrom-Json
 $registration=Get-ItemProperty $key
 Check 'Current-user Installed apps registration, version and target' ($registration.DisplayVersion -eq [Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $package 'seek.exe')).FileVersion -and $registration.InstallLocation -eq $program)
 $shell=New-Object -ComObject WScript.Shell
 Check 'Start and optional desktop shortcuts use the installed icon/executable' (@(@($m.start_shortcut,$m.desktop_shortcut)|Where-Object{$shell.CreateShortcut($_).TargetPath -ne (Join-Path $program 'seek.exe') -or $shell.CreateShortcut($_).IconLocation -notlike '*seek.ico*'}).Count -eq 0)
 foreach($name in @('seek.exe','seek-helper.exe')){
  $v=[Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $program $name))
  Check ($name+' has matching release metadata and Seek product name') ($v.FileVersion -eq [Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $package 'seek.exe')).FileVersion -and $v.ProductName -eq 'Seek')
 }
 Check 'Installed icon is byte-identical to the packaged icon' ((Get-FileHash (Join-Path $program 'seek.ico')).Hash -eq (Get-FileHash (Join-Path $package 'seek.ico')).Hash)
 $watch=[Diagnostics.Stopwatch]::StartNew()
 $app=Start-Process (Join-Path $program 'seek.exe') -ArgumentList @('--walker','--root',('"'+$scope+'"'),'--data',('"'+$data+'"')) -PassThru
 $found=$null
 do{
  Start-Sleep -Milliseconds 100;$app.Refresh()
  if($app.HasExited){throw 'Installed app exited before showing its fixture'}
  if($app.MainWindowHandle -ne 0){
   $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
   $found=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Package-owned.txt'))
  }
 }while(-not $found -and $watch.Elapsed.TotalSeconds -lt 30)
 Check 'Installed ordinary-user app launches and displays owned file' ([bool]$found)
 $launchMs=$watch.Elapsed.TotalMilliseconds
 $known=@{Downloads='374de290-123f-4565-9164-39c4925e467b';Desktop='b4bfcc3a-db2c-424c-b029-7fe99a87c641';Documents='fdd39ad0-238f-46af-adb4-6c85480369c7';Pictures='33e28130-4e1e-4676-835a-98395c3bc3bb';Videos='18989b1d-99b5-455b-841c-ab7c74e4ddfc';Music='4bd8d571-6d19-48d3-be97-422220080e43'}
 $knownCount=0
 foreach($name in $known.Keys){
  $path=[PackagingKnownFolders]::Get($known[$name]);$exists=$path -and [IO.Directory]::Exists($path)
  $button=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button)))
  if($exists){$knownCount++;Check ('Current-account Known Folder sidebar: '+$name) ([bool]$button)}
 }
 $rejected=$false
 try{& (Join-Path $package 'Install.ps1') -TestRoot $base -NonInteractive -Replace -NoDesktopShortcut}catch{$rejected=$true}
 Check 'Installer refuses to overwrite the running app' ($rejected -and -not $app.HasExited)
 Check 'Installed app closes normally' ($app.CloseMainWindow() -and $app.WaitForExit(10000) -and $app.ExitCode -eq 0)
 InvokeUninstall 'cancel'
 Check 'Uninstall Cancel leaves program, registration and saved data' ((Test-Path (Join-Path $program 'seek.exe')) -and (Test-Path $key) -and (Test-Path (Join-Path $data 'retain-sentinel')))
 InvokeUninstall 'keep'
 Check 'Default data answer keeps saved index/settings/cache fixtures' ((Test-Path (Join-Path $data 'retain-sentinel')) -and (Test-Path (Join-Path $data 'document-text-cache-sentinel')) -and -not(Test-Path $program) -and -not(Test-Path $key))
 Check 'Uninstall removes owned shortcuts' (-not(Test-Path $m.start_shortcut) -and -not(Test-Path $m.desktop_shortcut))
 & (Join-Path $package 'Install.ps1') -TestRoot $base -NonInteractive -NoDesktopShortcut
 $m=Get-Content (Join-Path $program 'install-manifest.json') -Raw|ConvertFrom-Json
 Check 'Desktop shortcut is optional' (-not $m.desktop_shortcut)
 InvokeUninstall 'delete'
 Check 'Explicit data approval removes only the owned data tree and program' (-not(Test-Path $data) -and -not(Test-Path $program) -and -not(Test-Path $key) -and (Test-Path (Join-Path $scope 'Package-owned.txt')))
 $helperAfter=@(Get-Service -Name ('SeekHelper-'+$sid) -ErrorAction SilentlyContinue|ForEach-Object Name)
 Check 'Existing optional helper service is unchanged' (($helperBefore -join ',') -eq ($helperAfter -join ','))
 @{checks=$checks;known_folders=$knownCount;alternate_account='Unavailable: no second enabled account. Manual standard-account instructions are in README.';profile_safety='Only marked TEMP fixture data and a unique HKCU test registration were written. Real Seek data and helper were untouched.';launch_smoke_observed_ms=$launchMs;executable_sha256=(Get-FileHash (Join-Path $package 'seek.exe')).Hash}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $Output -Encoding UTF8
 'PASS '+$checks.Count+' packaging checks'
}finally{if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
