param([Parameter(Mandatory=$true)][string]$PackageDirectory,[Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$folder=(Resolve-Path -LiteralPath $PackageDirectory).Path
$top=Split-Path $folder -Leaf
if($top -notmatch '^Seek-(\d+\.\d+\.\d+)-windows-x64$'){throw 'Unexpected package folder'}
$version=$Matches[1]
$stream=[IO.File]::Open([IO.Path]::GetFullPath($Destination),[IO.FileMode]::CreateNew)
$zip=$null
try{
 $zip=[IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
 foreach($file in @(Get-ChildItem -LiteralPath $folder -Force|Sort-Object Name)){
  if($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Update payload must contain only regular flat files'}
  $entry=$zip.CreateEntry(($top+'/'+$file.Name),[IO.Compression.CompressionLevel]::Optimal)
  $entry.LastWriteTime=[DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
  $sourceStream=[IO.File]::OpenRead($file.FullName);$entryStream=$entry.Open()
  try{$sourceStream.CopyTo($entryStream)}finally{$entryStream.Dispose();$sourceStream.Dispose()}
 }
}finally{if($zip){$zip.Dispose()};$stream.Dispose()}
& (Join-Path $PSScriptRoot 'check-update-zip.ps1') -Archive $Destination -Version $version
