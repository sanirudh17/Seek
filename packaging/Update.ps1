param([Parameter(Mandatory=$true)][string]$Request, [switch]$NoRestart)
# Finite, unelevated worker launched only after Download and restart is clicked.
$ErrorActionPreference='Stop'
$ready=$false;$replaced=@();$oldVersion=$null;$key=$null;$manifestBackup=$false
function QuoteArg([string]$value){'"'+[regex]::Replace($value,'(\\+)$','$1$1')+'"'}
function RestartSeek {
 if($NoRestart){return}
 $args=@('--data',(QuoteArg $r.data))
 if($r.root){$args+=@('--root',(QuoteArg $r.root))}
 if($r.walker){$args+='--walker'}
 Start-Process -FilePath (Join-Path $target 'seek.exe') -ArgumentList $args -WorkingDirectory $target -WindowStyle Normal|Out-Null
}
try{
 $r=Get-Content -LiteralPath $Request -Raw|ConvertFrom-Json
 $stage=[IO.Path]::GetFullPath($r.stage).TrimEnd('\')
 $temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
 if((Split-Path $stage -Parent) -ne $temp -or (Split-Path $stage -Leaf) -notmatch '^Seek-update-\d+-\d+$' -or [IO.Path]::GetFullPath($Request) -ne (Join-Path $stage 'request.json')){throw 'Unexpected update staging directory'}
 if((Get-Item -LiteralPath $stage).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Update stage must not be a link'}
 if($r.product -ne 'Seek' -or $r.version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid update request'}
 $target=[IO.Path]::GetFullPath($r.target).TrimEnd('\')
 if((Get-Item -LiteralPath $target).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Program directory must not be a link'}
 $current=Join-Path $target 'seek.exe';$payload=Join-Path $stage 'payload';$backup=Join-Path $stage 'backup'
 if([Diagnostics.FileVersionInfo]::GetVersionInfo($current).ProductName -ne 'Seek'){throw 'Target is not Seek'}
 $files=@('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Install.ps1','Install.cmd','Uninstall.ps1','Uninstall.cmd','Update.ps1')
 foreach($name in $files){
  $source=Join-Path $payload $name
  if(-not(Test-Path -LiteralPath $source -PathType Leaf) -or (Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint){throw ('Invalid update file: '+$name)}
  if((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $r.hashes.$name){throw ('Staged file checksum mismatch: '+$name)}
  $destination=Join-Path $target $name
  if((Test-Path -LiteralPath $destination) -and (Get-Item -LiteralPath $destination).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Program files must not be links'}
 }
 if([Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $payload 'seek.exe')).FileVersion -ne $r.version){throw 'Downloaded executable version does not match the release'}
 if(Test-Path -LiteralPath $backup){throw 'Refusing to reuse an update backup'}
 $manifest=Join-Path $target 'install-manifest.json'
 $installed=Test-Path -LiteralPath $manifest
 if($installed){
  $m=Get-Content -LiteralPath $manifest -Raw|ConvertFrom-Json
  if($m.product -ne 'Seek' -or $m.install_root -ne $target -or ($m.registry_id -ne 'Seek' -and $m.registry_id -notmatch '^Seek\.PackagingTest\.[a-f0-9]{32}$')){throw 'Installed package manifest does not match the target'}
  $files=@($files|Where-Object{$_ -notin @('Install.ps1','Install.cmd')})
  $key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$m.registry_id
  if(Test-Path $key){
   $registration=Get-ItemProperty $key
   if($registration.InstallLocation -ne $target){throw 'Installed-app registration points elsewhere'}
   $oldVersion=$registration.DisplayVersion
  }
 }
 # Parent remains open until this handshake. Preflight/policy failures leave it running.
 [IO.File]::WriteAllText((Join-Path $stage 'ready'),'ready')
 $ready=$true
 if($r.parent_pid -gt 0){
  $parent=Get-Process -Id $r.parent_pid -ErrorAction SilentlyContinue
  if($parent -and -not $parent.WaitForExit(45000)){throw 'Seek did not close; program files were not changed'}
 }
 New-Item -ItemType Directory -Path $backup|Out-Null
 foreach($name in $files){if(Test-Path -LiteralPath (Join-Path $target $name)){Copy-Item -LiteralPath (Join-Path $target $name) -Destination (Join-Path $backup $name)}}
 if($installed){Copy-Item -LiteralPath $manifest -Destination (Join-Path $backup 'install-manifest.json');$manifestBackup=$true}
 foreach($name in $files){
  # Include the attempted file, since a failed copy could have truncated it.
  $replaced+=,$name
  Copy-Item -LiteralPath (Join-Path $payload $name) -Destination (Join-Path $target $name) -Force
 }
 if([Diagnostics.FileVersionInfo]::GetVersionInfo($current).FileVersion -ne $r.version){throw 'Installed executable failed version validation'}
 if($installed){
  $m.version=$r.version
  [IO.File]::WriteAllText($manifest,($m|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
  if($key -and (Test-Path $key)){Set-ItemProperty -Path $key -Name DisplayVersion -Value $r.version}
 }
 RestartSeek
 [IO.File]::WriteAllText((Join-Path $stage 'update-result.json'),'{"success":true}')
 # All descendants were created here from the fixed file list. The absolute
 # stage was checked above against this account's TEMP before recursive removal.
 if(-not $NoRestart){Remove-Item -LiteralPath $stage -Recurse -Force}
}catch{
 $reason=$_.Exception.Message;$rollbackErrors=@()
 foreach($name in $replaced){
  try{
   $saved=Join-Path $backup $name;$destination=Join-Path $target $name
   if(Test-Path -LiteralPath $saved){Copy-Item -LiteralPath $saved -Destination $destination -Force}else{Remove-Item -LiteralPath $destination -Force -ErrorAction SilentlyContinue}
  }catch{$rollbackErrors+=$name}
 }
 if($manifestBackup){Copy-Item -LiteralPath (Join-Path $backup 'install-manifest.json') -Destination $manifest -Force}
 if($oldVersion -and $key){Set-ItemProperty -Path $key -Name DisplayVersion -Value $oldVersion}
 $detail=@{success=$false;error=$reason;rollback_errors=$rollbackErrors}
 if($stage -and (Test-Path -LiteralPath $stage)){[IO.File]::WriteAllText((Join-Path $stage 'update-result.json'),($detail|ConvertTo-Json))}
 if($ready -and -not $NoRestart){
  Add-Type -AssemblyName System.Windows.Forms
  [void][Windows.Forms.MessageBox]::Show(('The update did not finish. Previous files were restored where possible. Details: '+(Join-Path $stage 'update-result.json')),'Seek update')
  RestartSeek
 }
 Write-Error $reason
 exit 1
}
