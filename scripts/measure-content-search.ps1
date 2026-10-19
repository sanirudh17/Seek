param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [string]$Output='.references/content-search-release',
 [string]$CommonWord='note',
 [string]$OlderWord='rudkin',
 [switch]$Quick,
 [switch]$Evaluate,
 [string]$CatalogEvidence
)
# Opt-in read-only measurement on current-user Known Folders. Diagnostics exit
# before profile initialization. Never loads the real Seek settings/index.
$ErrorActionPreference='Stop'
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class SeekContentKnown {
 [DllImport("shell32.dll")]static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
 public static string Read(string value){Guid id=new Guid(value);IntPtr p;int hr=SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out p);if(hr<0)throw new Exception("Known folder unavailable: "+hr);try{return Marshal.PtrToStringUni(p);}finally{Marshal.FreeCoTaskMem(p);}}
}
'@
$exe=(Resolve-Path -LiteralPath $Executable).Path
$prefix=[IO.Path]::GetFullPath($Output)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($prefix))|Out-Null
$locations=@([SeekContentKnown]::Read('374DE290-123F-4565-9164-39C4925E467B'),[SeekContentKnown]::Read('FDD39AD0-238F-46AF-ADB4-6C85480369C7'))
$cases=@(@{query=$CommonWord;locations=$locations;include_paths=$true},@{query=$OlderWord;locations=$locations;include_paths=$true})
$manifest=$prefix+'-manifest.json'
[IO.File]::WriteAllText($manifest,($cases|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
$measureArgs=@('--content-audit',('"'+$manifest+'"'))
if($Quick){$measureArgs+='--content-quick'}
if($Evaluate){$measureArgs+='--content-evaluate'}
if($CatalogEvidence){
 $catalogCopy=$prefix+'-catalog.json'
 [IO.File]::WriteAllText($catalogCopy,[IO.File]::ReadAllText([IO.Path]::GetFullPath($CatalogEvidence)),[Text.UTF8Encoding]::new($false))
 $measureArgs+=@('--catalog-evidence',('"'+$catalogCopy+'"'))
}
$app=Start-Process -FilePath $exe -ArgumentList $measureArgs -PassThru -Wait -WindowStyle Hidden -RedirectStandardOutput ($prefix+'.json') -RedirectStandardError ($prefix+'.err')
if($app.ExitCode -ne 0){throw ('Measurement failed: '+[IO.File]::ReadAllText($prefix+'.err'))}
Write-Output ('Report: '+$prefix+'.json')
