param([switch]$DesktopShortcut, [switch]$NoDesktopShortcut, [switch]$Replace,
      [switch]$NonInteractive, [string]$TestRoot)
$ErrorActionPreference='Stop'
function Ask([string]$question){(Read-Host ($question+' [y/N]')) -match '^(?i)y(es)?$'}
function IsAdmin {([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)}
if(IsAdmin){throw 'Run Install.cmd normally, not as administrator. Seek installs for the current user.'}
if($DesktopShortcut -and $NoDesktopShortcut){throw 'Choose only one desktop-shortcut option'}
if($NonInteractive -and -not $TestRoot){throw 'NonInteractive is reserved for an owned packaging fixture'}
$test=$null
if($TestRoot){
 $test=[IO.Path]::GetFullPath($TestRoot).TrimEnd('\')
 $temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')+'\'
 if(-not $test.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path $test -Leaf) -notmatch '^Seek-packaging-[a-f0-9]{32}$' -or -not(Test-Path -LiteralPath (Join-Path $test '.seek-packaging-fixture'))){throw 'TestRoot must be an owned, marked Seek packaging fixture under TEMP'}
}
$install=if($test){Join-Path $test 'Program'}else{Join-Path $env:LOCALAPPDATA 'Programs\Seek'}
$data=if($test){Join-Path $test 'Data'}else{Join-Path $env:LOCALAPPDATA 'Seek'}
$id=if($test){'Seek.PackagingTest.'+(Split-Path $test -Leaf).Substring(15)}else{'Seek'}
$label=if($test){'Seek packaging test '+$id.Split('.')[-1]}else{'Seek'}
$programs=if($test){Join-Path $test 'StartMenu'}else{[Environment]::GetFolderPath('Programs')}
$desktop=if($test){Join-Path $test 'Desktop'}else{[Environment]::GetFolderPath('DesktopDirectory')}
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$id
$files=@('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Uninstall.ps1','Uninstall.cmd','Update.ps1')
foreach($file in $files){if(-not(Test-Path -LiteralPath (Join-Path $PSScriptRoot $file) -PathType Leaf)){throw ('Incomplete package: '+$file)}}
$version=[Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $PSScriptRoot 'seek.exe')).FileVersion
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'Package executable has invalid version metadata'}
if(Test-Path -LiteralPath $install){
 if((Get-Item -LiteralPath $install -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Install directory must not be a junction or symbolic link'}
 if(-not $Replace -and -not(Ask 'A Seek installation already exists. Replace its program files (saved data is retained)?')){throw 'Installation cancelled'}
}
$exe=Join-Path $install 'seek.exe'
foreach($p in Get-Process seek -ErrorAction SilentlyContinue){if($p.Path -eq $exe){throw 'Close this installation of Seek before installing'}}
if(-not $DesktopShortcut -and -not $NoDesktopShortcut -and -not $NonInteractive){$DesktopShortcut=Ask 'Create a desktop shortcut?'}
$startLink=Join-Path $programs ($label+'.lnk');$desktopLink=Join-Path $desktop ($label+'.lnk')
foreach($link in @($startLink)+$(if($DesktopShortcut){@($desktopLink)}else{@()})){
 if((Test-Path -LiteralPath $link) -and -not $Replace -and -not(Ask ('Replace existing shortcut '+(Split-Path $link -Leaf)+'?'))){throw 'Installation cancelled'}
}
New-Item -ItemType Directory -Path $install,$programs -Force|Out-Null
foreach($file in $files){
 $destination=Join-Path $install $file
 if((Test-Path -LiteralPath $destination) -and ((Get-Item -LiteralPath $destination -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Refusing to overwrite a linked file'}
 Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $destination -Force
}
$shell=New-Object -ComObject WScript.Shell
foreach($link in @($startLink)+$(if($DesktopShortcut){@($desktopLink)}else{@()})){
 New-Item -ItemType Directory -Path (Split-Path $link) -Force|Out-Null
 $shortcut=$shell.CreateShortcut($link);$shortcut.TargetPath=$exe;$shortcut.WorkingDirectory=$install;$shortcut.IconLocation=(Join-Path $install 'seek.ico')+',0';$shortcut.Description='Seek - fast local file search';$shortcut.Save()
}
$ownedDesktop=$DesktopShortcut -or ((Test-Path -LiteralPath $desktopLink) -and $shell.CreateShortcut($desktopLink).TargetPath -eq $exe)
$manifest=@{product='Seek';version=$version;install_root=$install;data_root=$data;test_root=$test;registry_id=$id;start_shortcut=$startLink;desktop_shortcut=$(if($ownedDesktop){$desktopLink}else{$null});files=$files}
[IO.File]::WriteAllText((Join-Path $install 'install-manifest.json'),($manifest|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
New-Item -Path $key -Force|Out-Null
$properties=@{DisplayName=$label;DisplayVersion=$version;Publisher='Seek contributors';InstallLocation=$install;DisplayIcon=(Join-Path $install 'seek.ico');URLInfoAbout='https://github.com/sanirudh17/Seek';UninstallString=('"'+$env:SystemRoot+'\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "'+(Join-Path $install 'Uninstall.ps1')+'"')}
foreach($name in $properties.Keys){New-ItemProperty -Path $key -Name $name -Value $properties[$name] -PropertyType String -Force|Out-Null}
foreach($name in @('NoModify','NoRepair')){New-ItemProperty -Path $key -Name $name -Value 1 -PropertyType DWord -Force|Out-Null}
$size=[int][Math]::Ceiling((Get-ChildItem -LiteralPath $install -File|Measure-Object Length -Sum).Sum/1KB)
New-ItemProperty -Path $key -Name EstimatedSize -Value $size -PropertyType DWord -Force|Out-Null
Write-Host ('Installed Seek '+$version+': '+$exe)
Write-Host 'No helper service, autostart, global shortcut, tray or background updater process was installed.'
