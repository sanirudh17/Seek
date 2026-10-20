param([Parameter(Mandatory=$true)][string]$Installer,[string]$Output='docs/benchmarks/installer-101.json')
$ErrorActionPreference='Stop'
$installerPath=(Resolve-Path -LiteralPath $Installer).Path
$checks=@()
function Check([string]$name,[bool]$passed){if(-not $passed){throw $name};$script:checks+=@{name=$name;passed=$true}}
function RunInstaller([string]$fixture){
 $process=Start-Process -FilePath $installerPath -ArgumentList @('/S',('/FIXTURE="'+$fixture+'"')) -WindowStyle Hidden -PassThru -Wait
 Check 'Native setup completed normally' ($process.ExitCode -eq 0)
}
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$helperBefore=@(Get-Service -Name ('SeekHelper-'+$sid) -ErrorAction SilentlyContinue|ForEach-Object Name)
foreach($mode in @('keep','delete')){
 $fixture=Join-Path $env:TEMP ('Seek-packaging-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path $fixture|Out-Null
 [IO.File]::WriteAllText((Join-Path $fixture '.seek-packaging-fixture'),'native installer fixture')
 $data=Join-Path $fixture 'Data';New-Item -ItemType Directory -Path $data|Out-Null
 [IO.File]::WriteAllText((Join-Path $data 'keep-sentinel'),'owned saved data')
 $program=Join-Path $fixture 'Program'
 RunInstaller $fixture
 $manifest=Get-Content -LiteralPath (Join-Path $program 'install-manifest.json') -Raw|ConvertFrom-Json
 $key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$manifest.registry_id
 $registration=Get-ItemProperty -LiteralPath $key
 $uninstaller=Join-Path $program 'Uninstall.exe'
 $expected=[Diagnostics.FileVersionInfo]::GetVersionInfo($installerPath).FileVersion
 Check 'Native uninstaller is registered and version matches' ((Test-Path -LiteralPath $uninstaller) -and $registration.UninstallString -eq ('"'+$uninstaller+'"') -and $registration.DisplayVersion -eq $expected)
 Check 'Installer writes only marked fixture locations' ($manifest.install_root -eq $program -and $manifest.data_root -eq $data -and $manifest.start_shortcut.StartsWith($fixture,[StringComparison]::OrdinalIgnoreCase))
 Check 'Start shortcut exists and desktop shortcut is optional' ((Test-Path -LiteralPath $manifest.start_shortcut) -and -not $manifest.desktop_shortcut)
 Check 'Saved data is retained during install' ((Get-Content (Join-Path $data 'keep-sentinel') -Raw) -eq 'owned saved data')
 # The existing updater also stages its install scripts in the program folder.
 [IO.File]::WriteAllText((Join-Path $program 'Install.ps1'),'owned updater fixture')
 [IO.File]::WriteAllText((Join-Path $program 'Install.cmd'),'owned updater fixture')
 $rejected=Start-Process -FilePath $installerPath -ArgumentList @('/S',('/FIXTURE="'+$fixture+'"')) -WindowStyle Hidden -PassThru -Wait
 Check 'Silent setup refuses replacement without approval' ($rejected.ExitCode -ne 0 -and (Test-Path -LiteralPath (Join-Path $program 'seek.exe')))
 if($mode -eq 'keep'){
  [IO.File]::WriteAllText((Join-Path $program 'unrecognized-owned.txt'),'preserve')
 }
 $args=@('/S');if($mode -eq 'delete'){$args+='/DELETE_DATA'}
 $uninstall=Start-Process -FilePath $uninstaller -ArgumentList $args -WindowStyle Hidden -PassThru -Wait
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 while(((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath (Join-Path $program 'seek.exe')) -or (Test-Path -LiteralPath $uninstaller)) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
 Check 'Native uninstall removes program and registration' (-not(Test-Path -LiteralPath (Join-Path $program 'seek.exe')) -and -not(Test-Path -LiteralPath $key))
 Check 'Native uninstall removes its own executable' (-not(Test-Path -LiteralPath $uninstaller))
 Check 'Native uninstall removes owned shortcut' (-not(Test-Path -LiteralPath $manifest.start_shortcut))
 Check 'Native uninstall removes updater-staged installer scripts' (-not(Test-Path -LiteralPath (Join-Path $program 'Install.ps1')) -and -not(Test-Path -LiteralPath (Join-Path $program 'Install.cmd')))
 if($mode -eq 'keep'){
  Check 'Default native uninstall retains saved data and unexpected program files' ((Test-Path -LiteralPath (Join-Path $data 'keep-sentinel')) -and (Test-Path -LiteralPath (Join-Path $program 'unrecognized-owned.txt')))
 }else{
  Check 'Explicit native uninstall data approval removes only fixture data' (-not(Test-Path -LiteralPath $data) -and -not(Test-Path -LiteralPath $program) -and (Test-Path -LiteralPath (Join-Path $fixture '.seek-packaging-fixture')))
 }
}
$helperAfter=@(Get-Service -Name ('SeekHelper-'+$sid) -ErrorAction SilentlyContinue|ForEach-Object Name)
Check 'Optional real helper service registration unchanged' (($helperBefore -join ',') -eq ($helperAfter -join ','))
@{checks=$checks;installer_sha256=(Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash;test_scope='Marked TEMP fixtures and unique HKCU registrations only. No production profile or installation changed.';alternate_account='Not verified; only the current ordinary-user account was used.'}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $Output -Encoding UTF8
'PASS '+$checks.Count+' native installer checks'
