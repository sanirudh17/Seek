param([switch]$Confirmed, [switch]$KeepData, [switch]$DeleteData, [switch]$KeepHelper)
$ErrorActionPreference='Stop'
function Ask([string]$question){(Read-Host ($question+' [y/N]')) -match '^(?i)y(es)?$'}
if($KeepData -and $DeleteData){throw 'Choose KeepData or DeleteData, not both'}
$manifestFile=Join-Path $PSScriptRoot 'install-manifest.json'
if(-not(Test-Path -LiteralPath $manifestFile)){throw 'No Seek install manifest found. Run the installed Uninstall.cmd, not the copy inside the downloaded ZIP.'}
$m=Get-Content -LiteralPath $manifestFile -Raw|ConvertFrom-Json
$install=[IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')
$expected=if($m.test_root){Join-Path $m.test_root 'Program'}else{Join-Path $env:LOCALAPPDATA 'Programs\Seek'}
$data=if($m.test_root){Join-Path $m.test_root 'Data'}else{Join-Path $env:LOCALAPPDATA 'Seek'}
if($m.product -ne 'Seek' -or $install -ne [IO.Path]::GetFullPath($expected).TrimEnd('\') -or $m.data_root -ne $data){throw 'Install manifest does not match this user and installation'}
if($m.test_root){
 $test=[IO.Path]::GetFullPath($m.test_root).TrimEnd('\');$temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')+'\'
 if(-not $test.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -or (Split-Path $test -Leaf) -notmatch '^Seek-packaging-[a-f0-9]{32}$' -or -not(Test-Path -LiteralPath (Join-Path $test '.seek-packaging-fixture'))){throw 'Invalid packaging fixture'}
}
if((Get-Item -LiteralPath $install).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Install directory must not be a link'}
$exe=Join-Path $install 'seek.exe'
foreach($p in Get-Process seek -ErrorAction SilentlyContinue){if($p.Path -eq $exe){throw 'Close this installation of Seek before uninstalling'}}
if(-not $Confirmed -and -not(Ask 'Uninstall Seek program files and shortcuts?')){Write-Host 'Cancelled. Nothing removed.';exit 0}
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$helper=Get-Service -Name ('SeekHelper-'+$sid) -ErrorAction SilentlyContinue
if($helper -and -not $KeepHelper){
 if(-not(Ask 'The optional helper service remains installed. Remove it in Seek Settings first (requires UAC), or continue keeping that service?')){Write-Host 'Cancelled. Remove the helper in Settings, then uninstall again.';exit 0}
}
if(-not $KeepData -and -not $DeleteData){$DeleteData=Ask ('Also delete your saved index, settings and document-text cache in '+$data+'? Default is KEEP.')}
$shell=New-Object -ComObject WScript.Shell
foreach($link in @($m.start_shortcut,$m.desktop_shortcut)){
 if($link -and (Test-Path -LiteralPath $link)){
  if($shell.CreateShortcut($link).TargetPath -eq $exe){Remove-Item -LiteralPath $link -Force}else{Write-Warning ('Preserving a shortcut that now points elsewhere: '+$link)}
 }
}
$id=if($m.test_root){'Seek.PackagingTest.'+(Split-Path $m.test_root -Leaf).Substring(15)}else{'Seek'}
if($m.registry_id -ne $id){throw 'Unexpected uninstall registry key'}
Remove-Item -LiteralPath ('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$id) -Force -ErrorAction SilentlyContinue
# Delete only known package files; never recursively remove a program directory.
foreach($file in @('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Install.ps1','Install.cmd','Uninstall.ps1','Uninstall.cmd','Update.ps1','install-manifest.json')){
 $path=Join-Path $install $file
 if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path -Force}
}
if(@(Get-ChildItem -LiteralPath $install -Force).Count -eq 0){Remove-Item -LiteralPath $install -Force}else{Write-Warning ('Preserved unrecognized files in '+$install)}
function RemoveSavedTree([string]$path){
 # Do not follow junctions/symlinks out of the explicitly approved data tree.
 $item=Get-Item -LiteralPath $path -Force
 if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Saved data contains a link. It was preserved for manual review.'}
 foreach($child in Get-ChildItem -LiteralPath $path -Force){
  if($child.Attributes -band [IO.FileAttributes]::ReparsePoint){Write-Warning ('Preserved linked data: '+$child.Name);continue}
  if($child.PSIsContainer){RemoveSavedTree $child.FullName}else{Remove-Item -LiteralPath $child.FullName -Force}
 }
 if(@(Get-ChildItem -LiteralPath $path -Force).Count -eq 0){Remove-Item -LiteralPath $path -Force}
}
if($DeleteData -and (Test-Path -LiteralPath $data)){RemoveSavedTree $data}
Write-Host 'Seek program uninstalled.'
if($DeleteData){Write-Host 'Approved saved-data removal finished.'}else{Write-Host ('Saved index, settings and document text retained: '+$data)}
if($helper){Write-Warning 'The explicitly retained optional helper is still installed. See README for the administrator removal command.'}
