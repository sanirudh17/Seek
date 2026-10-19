param(
 [string]$Cli='target/x86_64-pc-windows-msvc/release/seek-cli.exe',
 [string]$Profile=(Join-Path $env:LOCALAPPDATA 'Seek'),
 [string]$Output='docs/benchmarks/v2-stage-a-drive-closed.json'
)
$ErrorActionPreference='Stop'
$exe=(Resolve-Path -LiteralPath $Cli).Path
$base=[IO.Path]::GetFullPath('.bench-data/v2-a-drive-truth-'+[DateTime]::UtcNow.Ticks)
$downloads=(New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
$root=Join-Path $downloads ('Seek-Stage-A-Check-'+[DateTime]::UtcNow.Ticks)
$seed=Join-Path $base 'seed'
New-Item -ItemType Directory -Path $seed,(Join-Path $root 'Nested/Child'),(Join-Path $root 'Destination') -Force|Out-Null
foreach($name in @('rename.txt','move.txt','edit.txt','leave-scope.txt')){[IO.File]::WriteAllText((Join-Path $root $name),'Owned closed-app test')}
[IO.File]::WriteAllText((Join-Path $root 'Nested/Child/deep.txt'),'Owned nested test')
foreach($name in @('index.bin','roots.json')){Copy-Item -LiteralPath (Join-Path $Profile $name) -Destination $seed}
function MoveOwned([string]$source,[string]$destination) {
 $resolved=(Resolve-Path -LiteralPath $source).Path
 $target=[IO.Path]::GetFullPath($destination)
 foreach($path in @($resolved,$target)){
  if(-not($path.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase) -or $path.StartsWith($base+'\',[StringComparison]::OrdinalIgnoreCase))){throw 'Test move escaped owned fixture'}
 }
 Move-Item -LiteralPath $resolved -Destination $target
}
function Verify([string]$data,[string]$source,[string]$path) {
 $log=Join-Path $base ([IO.Path]::GetFileNameWithoutExtension($path)+'.log')
 # Invoke through cmd so PowerShell 5 does not treat native stderr as a
 # terminating ErrorRecord. All arguments are controlled fixture paths.
 $command='""'+$exe+'" verify --root C:\ --data "'+$data+'" --source '+$source+' --progress --verify-root "'+$root+'" --output "'+[IO.Path]::GetFullPath($path)+'" > "'+$log+'" 2>&1"'
 & cmd.exe /d /s /c $command
 if($LASTEXITCODE -ne 0){throw ('Drive verification failed; see '+$log)}
 $r=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
 if($r.report.missing.Count -or $r.report.extra.Count -or $r.report.stale.Count -or $r.report.denied.Count){throw 'Nonzero truth discrepancy'}
}
try {
 Verify $seed 'helper' 'docs/benchmarks/v2-stage-a-drive-seed.json'
 foreach($source in @('helper','walker')){
  $data=Join-Path $base $source
  New-Item -ItemType Directory -Path $data|Out-Null
  foreach($name in @('index.bin','roots.json')){Copy-Item -LiteralPath (Join-Path $seed $name) -Destination $data}
 }
 # Everything below is an owned fixture. Move out of the verification scope
 # to test removals without permanently deleting any test file.
 MoveOwned (Join-Path $root 'rename.txt') (Join-Path $root 'Renamed.txt')
 MoveOwned (Join-Path $root 'move.txt') (Join-Path $root 'Destination/moved.txt')
 [IO.File]::WriteAllText((Join-Path $root 'edit.txt'),('Changed while Seek was closed. '*19))
 MoveOwned (Join-Path $root 'leave-scope.txt') (Join-Path $base 'left-scope.txt')
 New-Item -ItemType Directory -Path (Join-Path $root 'New/Deep')|Out-Null
 [IO.File]::WriteAllText((Join-Path $root 'New/Deep/new.txt'),'New closed-app subtree')
 MoveOwned (Join-Path $root 'Nested') (Join-Path $root 'RenamedNested')
 foreach($source in @('helper','walker')){
  $path='docs/benchmarks/v2-stage-a-drive-closed-'+$source+'.json'
  Verify (Join-Path $base $source) $source $path
  Write-Output ('PASS whole-C: '+$source+' index agrees with owned Downloads truth after closed changes')
 }
} finally {
 @{cli=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;fixture=$root;data=$base;source_profile=$Profile;scope='Whole real C: index; independent truth of an owned Downloads subtree only. Other changing C: files are not independently certified. No test file permanently deleted; no helper installation change.'}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $Output -Encoding UTF8
}
