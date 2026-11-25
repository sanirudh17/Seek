# Owned archive fixtures only; does not run or install any packaged program.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$fixture=Join-Path $PSScriptRoot ('../.bench-data/update-zip-'+[guid]::NewGuid().ToString('N'))
$folder=Join-Path $fixture 'Seek-1.0.2-windows-x64'
New-Item -ItemType Directory -Path $folder -Force|Out-Null
$files=@('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Install.ps1','Install.cmd','Uninstall.ps1','Uninstall.cmd','Update.ps1')
foreach($name in $files){[IO.File]::WriteAllText((Join-Path $folder $name),'owned fixture '+$name)}
$first=Join-Path $fixture 'first.zip';$second=Join-Path $fixture 'second.zip'
& (Join-Path $PSScriptRoot 'write-update-zip.ps1') -PackageDirectory $folder -Destination $first
& (Join-Path $PSScriptRoot 'write-update-zip.ps1') -PackageDirectory $folder -Destination $second
if((Get-FileHash $first).Hash -ne (Get-FileHash $second).Hash){throw 'ZIP writer is not deterministic'}
$checks=2
foreach($bad in @('Seek-1.0.2-windows-x64\seek.exe','Seek-1.0.2-windows-x64/../seek.exe','Other/seek.exe','Seek-1.0.2-windows-x64/extra.exe')){
 $archive=Join-Path $fixture ('bad-'+$checks+'.zip')
 $zip=[IO.Compression.ZipFile]::Open($archive,[IO.Compression.ZipArchiveMode]::Create)
 try{$null=$zip.CreateEntry($bad)}finally{$zip.Dispose()}
 $rejected=$false
 try{& (Join-Path $PSScriptRoot 'check-update-zip.ps1') -Archive $archive -Version '1.0.2'}catch{$rejected=$true}
 if(-not $rejected){throw ('Unsafe archive accepted: '+$bad)}
 $checks++
}
Write-Output ('PASS '+$checks+' update ZIP packaging checks; fixtures retained under .bench-data')
