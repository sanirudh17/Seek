param(
    [string]$Bin = 'target/x86_64-pc-windows-msvc/debug',
    [string]$Output = 'docs/benchmarks/stage2-smoke.json'
)
$ErrorActionPreference = 'Stop'
$seekCli = (Resolve-Path -LiteralPath (Join-Path $Bin 'seek-cli.exe')).Path
$seekHelper = (Resolve-Path -LiteralPath (Join-Path $Bin 'seek-helper.exe')).Path
$seekIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$seekSid = $seekIdentity.User.Value
$seekElevated = ([Security.Principal.WindowsPrincipal]$seekIdentity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($seekElevated) { throw 'This smoke check deliberately requires an unelevated terminal.' }
& $seekCli ntfs-probe --drive C --output docs/benchmarks/stage2-ntfs-probe.json | Out-Null
if ($LASTEXITCODE) { throw 'NTFS probe failed' }
$seekNtfs = Get-Content docs/benchmarks/stage2-ntfs-probe.json -Raw -Encoding UTF8 | ConvertFrom-Json
& $seekCli helper-probe --output docs/benchmarks/stage2-service-probe.json | Out-Null
if ($LASTEXITCODE) { throw 'Service probe failed' }
$seekService = Get-Content docs/benchmarks/stage2-service-probe.json -Raw -Encoding UTF8 | ConvertFrom-Json
if ($seekService.available) { throw 'An installed helper is active; skip the temporary console server check.' }
$seekAudit = Get-Content docs/benchmarks/real-world-check.json -Raw -Encoding UTF8 | ConvertFrom-Json
$seekRoot = $seekAudit.folders[0].path
$ErrorActionPreference = 'Continue' # This expected native failure writes stderr.
& $seekCli verify --source helper --root $seekRoot --data .bench-data/stage2-forced-helper 2> .references/stage2-forced-helper.log | Out-Null
$seekForcedExit = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($seekForcedExit -eq 0) { throw 'Forced helper unexpectedly accepted an absent service' }
$seekForcedMessage = Get-Content .references/stage2-forced-helper.log -Raw

function Read-SeekPipeBytes($stream, [int]$count) {
    $bytes = New-Object byte[] $count
    $offset = 0
    while ($offset -lt $count) {
        $task = $stream.ReadAsync($bytes, $offset, $count-$offset)
        if (-not $task.Wait(5000)) { throw 'Temporary helper read timed out' }
        if ($task.Result -eq 0) { throw 'Temporary helper closed its pipe' }
        $offset += $task.Result
    }
    return ,$bytes
}
$seekConsole = Start-Process -FilePath $seekHelper -ArgumentList @('serve', '--sid', $seekSid) -WindowStyle Hidden -PassThru -RedirectStandardError .references/stage2-console.log
try {
    Start-Sleep -Milliseconds 300 # Test startup only.
    & $seekCli helper-probe --console --output docs/benchmarks/stage2-console-probe.json | Out-Null
    if ($LASTEXITCODE) { throw 'Console Ping failed' }
    $seekPong = Get-Content docs/benchmarks/stage2-console-probe.json -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($seekPong.trusted_installed_service -or $seekPong.response.Pong.version -ne 1) { throw 'Console trust/protocol mismatch' }
    # Exercise the actual server Sync + disconnect lifecycle without elevation.
    # Raw-volume access must produce an explicit fallback, rather than an empty index.
    $seekPipe = [IO.Pipes.NamedPipeClientStream]::new('.', ('SeekHelper-'+$seekSid), [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous, [Security.Principal.TokenImpersonationLevel]::Impersonation)
    $seekSync = @()
    try {
        $seekPipe.Connect(3000)
        $seekInternal = '/'+$seekRoot.Replace('\','/').TrimEnd('/')
        $seekRootBytes = [Text.Encoding]::UTF8.GetBytes($seekInternal)
        $seekRequest = @{Sync=@{version=1;roots=@(,$seekRootBytes);cursors=@();scan=$true}} | ConvertTo-Json -Depth 8 -Compress
        $seekPayload = [Text.Encoding]::UTF8.GetBytes($seekRequest)
        $seekLength = [BitConverter]::GetBytes([uint32]$seekPayload.Length)
        $seekPipe.Write($seekLength,0,4)
        $seekPipe.Write($seekPayload,0,$seekPayload.Length)
        do {
            $seekFrameLength = [BitConverter]::ToUInt32((Read-SeekPipeBytes $seekPipe 4),0)
            if ($seekFrameLength -eq 0 -or $seekFrameLength -gt 2097152) { throw 'Invalid frame length' }
            $seekFrame = [Text.Encoding]::UTF8.GetString((Read-SeekPipeBytes $seekPipe $seekFrameLength)) | ConvertFrom-Json
            $seekSync += $seekFrame
            if ($seekFrame.Error) { throw $seekFrame.Error.message }
        } while (-not $seekFrame.Ready)
    } finally { $seekPipe.Dispose() }
    if (@($seekSync | Where-Object Fallback).Count -ne 1 -or $seekSync[-1].Ready.cursors.Count -ne 0) { throw 'Expected explicit unelevated Sync fallback' }
    Start-Sleep -Milliseconds 100
    & $seekCli helper-probe --console --output .bench-data/stage2-after-disconnect.json | Out-Null
    if ($LASTEXITCODE) { throw 'Server did not recover after client disconnect' }
    # An unregistered process must never become the production index source.
    & $seekCli verify --root $seekRoot --data .bench-data/real-world/Downloads --output docs/benchmarks/stage2-real-downloads.json | Out-Null
    if ($LASTEXITCODE) { throw 'Downloads verification failed' }
    $seekDownloads = Get-Content docs/benchmarks/stage2-real-downloads.json -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($seekDownloads.status.source -ne 'walker') { throw 'Unregistered server was trusted' }
    $seekConsole.Refresh()
    $seekCpu = $seekConsole.TotalProcessorTime.TotalMilliseconds
    $seekIdle = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds 3
    $seekConsole.Refresh()
    $seekCpuDelta = $seekConsole.TotalProcessorTime.TotalMilliseconds - $seekCpu
    $seekIdleWallMs = $seekIdle.Elapsed.TotalMilliseconds
    $seekMemory = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekConsole.Id)"
    $seekReport = [ordered]@{
        elevated=$seekElevated; ntfs=$seekNtfs; installed_service=$seekService
        forced_helper_exit=$seekForcedExit; forced_helper_message=$seekForcedMessage.Trim()
        console_ping=$seekPong; console_sync=$seekSync; reconnect_after_disconnect=$true
        production_ignored_unregistered_server=($seekDownloads.status.source -eq 'walker')
        console_idle_wall_ms=$seekIdleWallMs; console_idle_cpu_ms=$seekCpuDelta
        console_private_working_set_bytes=[long]$seekMemory.WorkingSetPrivate
        console_total_working_set_bytes=[long]$seekConsole.WorkingSet64
        console_private_commit_bytes=[long]$seekConsole.PrivateMemorySize64
        note='Temporary unelevated console server; not an installed service or raw-NTFS performance measurement. No UAC, installation or user-file mutation.'
    }
} finally {
    # Stop only the exact temporary process launched by this script.
    if (-not $seekConsole.HasExited) { Stop-Process -InputObject $seekConsole }
    $seekConsole.WaitForExit()
}
$seekReport | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $Output -Encoding UTF8
foreach ($seekFolder in @($seekAudit.folders | Select-Object -Skip 1)) {
    & $seekCli verify --root $seekFolder.path --data ('.bench-data/real-world/'+$seekFolder.name) --output ('docs/benchmarks/stage2-real-'+$seekFolder.name.ToLowerInvariant()+'.json') | Out-Null
    if ($LASTEXITCODE) { throw "Real verification failed: $($seekFolder.name)" }
}
& $seekCli self-test --source walker --output docs/benchmarks/stage2-walker-correctness.json | Out-Null
if ($LASTEXITCODE) { throw 'Walker correctness check failed' }
Write-Output "Stage 2 unelevated smoke checks passed. Reports: $Output"
