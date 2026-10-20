param([Parameter(Mandatory=$true)][string]$PackageDirectory,[string]$Output='docs/benchmarks/update-validation.json')
$ErrorActionPreference='Stop'
$package=(Resolve-Path -LiteralPath $PackageDirectory).Path
$files=@('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Install.ps1','Install.cmd','Uninstall.ps1','Uninstall.cmd','Update.ps1')
$base=Join-Path $env:TEMP ('Seek-packaging-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $base|Out-Null
[IO.File]::WriteAllText((Join-Path $base '.seek-packaging-fixture'),'owned updater checks')
$checks=@()
function Check([string]$name,[bool]$value){if(-not $value){throw $name};$script:checks+=@{name=$name;passed=$true}}
foreach($mode in @('success','locked','tampered')){
 $target=Join-Path $base $mode;$data=Join-Path $target 'Data'
 New-Item -ItemType Directory -Path $data -Force|Out-Null
 foreach($file in $files){Copy-Item -LiteralPath (Join-Path $package $file) -Destination $target}
 $exe=Join-Path $target 'seek.exe'
 $stream=[IO.File]::Open($exe,[IO.FileMode]::Append);try{$bytes=[Text.Encoding]::UTF8.GetBytes('owned old-version sentinel');$stream.Write($bytes,0,$bytes.Length)}finally{$stream.Dispose()}
 $before=(Get-FileHash $exe).Hash
 [IO.File]::WriteAllText((Join-Path $data 'retain'),'saved user data')
 $stage=Join-Path $env:TEMP ('Seek-update-'+$PID+'-'+[DateTime]::UtcNow.Ticks)
 $payload=Join-Path $stage 'payload';New-Item -ItemType Directory -Path $payload -Force|Out-Null
 $hashes=@{}
 foreach($file in $files){Copy-Item -LiteralPath (Join-Path $package $file) -Destination $payload;$hashes[$file]=(Get-FileHash (Join-Path $payload $file)).Hash}
 $request=Join-Path $stage 'request.json'
 @{product='Seek';version=[Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $package 'seek.exe')).FileVersion;stage=$stage;target=$target;parent_pid=0;data=$data;root='';walker=$true;hashes=$hashes}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $request -Encoding UTF8
 $lock=$null
 if($mode -eq 'locked'){$lock=[IO.File]::Open((Join-Path $target 'seek-helper.exe'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)}
 if($mode -eq 'tampered'){[IO.File]::AppendAllText((Join-Path $payload 'README.md'),'tampered')}
 try{
  $savedPreference=$ErrorActionPreference;$ErrorActionPreference='Continue'
  & (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $package 'Update.ps1') -Request $request -NoRestart *> (Join-Path $base ($mode+'.log'))
  $code=$LASTEXITCODE
 }finally{$ErrorActionPreference=$savedPreference;if($lock){$lock.Dispose()}}
 $result=Get-Content (Join-Path $stage 'update-result.json') -Raw|ConvertFrom-Json
 if($mode -eq 'success'){
  Check 'Verified payload replaces program files successfully' ($code -eq 0 -and $result.success -and (Get-FileHash $exe).Hash -eq $hashes['seek.exe'])
  Check 'Successful update handshakes before replacement' (Test-Path (Join-Path $stage 'ready'))
 }elseif($mode -eq 'locked'){
  Check 'Locked destination fails and restores previously replaced executable' ($code -ne 0 -and -not $result.success -and (Get-FileHash $exe).Hash -eq $before)
 }else{
  Check 'Changed payload is rejected before handshake or replacement' ($code -ne 0 -and -not $result.success -and -not(Test-Path (Join-Path $stage 'ready')) -and (Get-FileHash $exe).Hash -eq $before)
 }
 Check ($mode+': saved data is unchanged') ((Get-Content (Join-Path $data 'retain') -Raw) -eq 'saved user data')
}
@{checks=$checks;scope='Owned TEMP program/data copies only; no real installation or data was touched.';limitations='The replacement worker is exercised with the same version and NoRestart. A future-version end-to-end automatic download/restart cannot be tested against the first public release.'}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $Output -Encoding UTF8
'PASS '+$checks.Count+' update replacement checks'
exit 0
