param([Parameter(Mandatory=$true)][string]$Archive,[Parameter(Mandatory=$true)][string]$Version)
$ErrorActionPreference='Stop'
if($Version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid package version'}
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$prefix='Seek-'+$Version+'-windows-x64/'
$allowed=@('seek.exe','seek-helper.exe','seek.ico','README.md','LICENSE','THIRD-PARTY-NOTICES.txt','Install.ps1','Install.cmd','Uninstall.ps1','Uninstall.cmd','Update.ps1')
$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$zip=[IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $Archive).Path)
try{
 foreach($entry in $zip.Entries){
  $name=$entry.FullName
  if($name.Contains('\') -or -not $name.StartsWith($prefix,[StringComparison]::Ordinal)){throw ('Non-portable ZIP entry: '+$name)}
  if($name -eq $prefix){continue}
  $leaf=$name.Substring($prefix.Length)
  if($leaf -cnotin $allowed -or -not $seen.Add($leaf)){throw ('Unexpected or duplicate ZIP entry: '+$name)}
 }
 if($seen.Count -ne $allowed.Count){throw 'Incomplete update ZIP'}
}finally{$zip.Dispose()}
Write-Output ('PASS: '+$seen.Count+' portable update ZIP entries')
