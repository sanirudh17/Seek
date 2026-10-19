param([string]$Bin='target/x86_64-pc-windows-msvc/release')
$ErrorActionPreference='Stop'
$seekCli=(Resolve-Path -LiteralPath (Join-Path $Bin 'seek-cli.exe')).Path
$seekBase=[IO.Path]::GetFullPath('.bench-data/stage2-reset-'+[DateTime]::UtcNow.Ticks)
if(-not $seekBase.StartsWith([IO.Path]::GetFullPath('.bench-data')+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Fixture is outside the workspace benchmark directory'}
$seekRoot=Join-Path $seekBase 'files'
$seekData=Join-Path $seekBase 'index'
[void](New-Item -ItemType Directory -Path $seekRoot -Force)
[IO.File]::WriteAllText((Join-Path $seekRoot 'before.txt'),'before')
[IO.File]::WriteAllText((Join-Path $seekRoot 'obsolete.txt'),'obsolete')
& $seekCli index --source walker --root $seekRoot --data $seekData | Out-Null
if($LASTEXITCODE){throw 'Fixture index failed'}
# Build a synthetic checkpoint on this OWNED index only. This enables a real
# helper reset/reconciliation check without touching the Windows journal.
$seekRootsJson=Get-Content (Join-Path $seekData 'roots.json') -Raw
# Preserve nested arrays exactly; PowerShell pipeline enumeration can unwrap
# a single root and turn Vec<Vec<u8>> into a flat byte array.
$seekState='{"roots":'+$seekRootsJson+',"cursors":[{"drive":67,"serial":123,"journal_id":456,"next_usn":77}],"ntfs_baseline":false}'
$seekStateBytes=[Text.Encoding]::UTF8.GetBytes($seekState)
$seekIndexPath=Join-Path $seekData 'index.bin'
$seekCore=[IO.File]::ReadAllBytes($seekIndexPath)
$seekFooter=$seekCore+$seekStateBytes+[BitConverter]::GetBytes([uint32]$seekStateBytes.Length)+[Text.Encoding]::ASCII.GetBytes('SEEKUSN1')
[IO.File]::WriteAllBytes($seekIndexPath,[byte[]]$seekFooter)
& $seekCli invalidate-journal --data $seekData --output docs/benchmarks/stage2-checkpoint-diagnostic.json | Out-Null
if($LASTEXITCODE){throw 'Checkpoint diagnostic failed'}
$seekChanged=[IO.File]::ReadAllBytes($seekIndexPath)
$seekStateLength=[BitConverter]::ToUInt32($seekChanged,$seekChanged.Length-12)
$seekChangedState=[Text.Encoding]::UTF8.GetString($seekChanged,$seekChanged.Length-12-$seekStateLength,$seekStateLength)|ConvertFrom-Json
if($seekChangedState.cursors[0].journal_id -ne 457){throw 'Diagnostic did not change the test checkpoint ID'}
# The engine process is closed during these changes. All paths are verified
# descendants of the named workspace fixture above.
Move-Item -LiteralPath (Join-Path $seekRoot 'before.txt') -Destination (Join-Path $seekRoot 'renamed-closed.txt')
Remove-Item -LiteralPath (Join-Path $seekRoot 'obsolete.txt')
[IO.File]::WriteAllText((Join-Path $seekRoot 'created-closed.txt'),'created while engine was closed')
& $seekCli verify --source helper --root $seekRoot --data $seekData --output docs/benchmarks/stage2-actual-helper-reset.json | Out-Null
if($LASTEXITCODE){throw 'Actual helper reset verification failed'}
$seekResult=Get-Content docs/benchmarks/stage2-actual-helper-reset.json -Raw -Encoding UTF8|ConvertFrom-Json
if($seekResult.status.source -notmatch 'usn'){throw 'No actual helper journal connection'}
Write-Output "Helper-connected closed reconciliation verified $($seekResult.report.ground_truth) entries, zero discrepancies. This starts from a walker baseline; a later decoder fallback can mask the specific reset branch. It does not validate MFT performance or ordinary delta replay."
