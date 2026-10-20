param([Parameter(Mandatory=$true)][string]$PackageDirectory,[string]$Compiler,[switch]$ChecksumsOnly)
$ErrorActionPreference='Stop'
$payload=(Resolve-Path -LiteralPath $PackageDirectory).Path
$version=[Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $payload 'seek.exe')).FileVersion
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid executable metadata'}
if(-not $Compiler){$Compiler=Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'}
if(-not(Test-Path -LiteralPath $Compiler)){throw 'Install NSIS 3 or supply -Compiler; no administrator action is performed by this script'}
$output=Join-Path (Split-Path $payload) ('Seek-'+$version+'-Setup-x64.exe')
if(-not $ChecksumsOnly){
 & $Compiler '/V2' ('/DVERSION='+$version) ('/DPAYLOAD='+$payload) ('/DOUTPUT='+$output) (Join-Path $PSScriptRoot '../packaging/Seek.nsi')
 if($LASTEXITCODE -ne 0){throw 'NSIS compiler failed'}
}
if([Diagnostics.FileVersionInfo]::GetVersionInfo($output).FileVersion -ne $version){throw 'Installer metadata mismatch'}
$hash=(Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
$checksum=Join-Path (Split-Path $payload) 'SHA256SUMS.txt'
$lines=@()
if(Test-Path -LiteralPath $checksum){$lines=@([IO.File]::ReadAllLines($checksum)|Where-Object{$_ -notmatch [regex]::Escape((Split-Path $output -Leaf))})}
[IO.File]::WriteAllText($checksum,($lines+($hash+'  '+(Split-Path $output -Leaf)) -join "`n")+"`n",[Text.UTF8Encoding]::new($false))
Write-Output $output
