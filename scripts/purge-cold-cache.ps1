param([Parameter(Mandatory=$true)][string]$Tool, [Parameter(Mandatory=$true)][string]$Receipt)
# One-shot benchmark helper only. This script NEVER launches Seek.
$ErrorActionPreference='Stop'
$admin=([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if(-not $admin){throw 'Only the cache-purge diagnostic needs elevation; Seek must stay ordinary-user.'}
$toolFile=(Resolve-Path -LiteralPath $Tool).Path
$signature=Get-AuthenticodeSignature -LiteralPath $toolFile
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation'){throw 'Expected a valid Microsoft-signed RAMMap executable'}
function Standby {
 $c=Get-CimInstance Win32_PerfRawData_PerfOS_Memory
 [uint64]$c.StandbyCacheCoreBytes+[uint64]$c.StandbyCacheNormalPriorityBytes+[uint64]$c.StandbyCacheReserveBytes
}
$before=Standby
$actions=@()
foreach($flag in @('-Es','-Et')){
 $p=Start-Process -FilePath $toolFile -ArgumentList @('-accepteula',$flag) -WindowStyle Hidden -PassThru
 if(-not $p.WaitForExit(15000)){throw "RAMMap $flag did not finish"}
 if($p.ExitCode -ne 0){throw "RAMMap $flag failed: $($p.ExitCode)"}
 $actions+=@{action=$flag;exit_code=$p.ExitCode}
}
@{administrator_diagnostic_only=$admin;tool_sha256=(Get-FileHash -LiteralPath $toolFile).Hash;signature='Valid Microsoft';actions=$actions;standby_before_bytes=$before;standby_after_bytes=(Standby);completed_unix_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $Receipt -Encoding UTF8
