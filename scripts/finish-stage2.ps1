# Run this ONE script in administrator PowerShell, using your usual Windows account.
# Close Seek first. To remove ONLY Seek's helper later, rerun with -Uninstall.
# Writes docs/benchmarks/stage2-final-results.json. No user files or Windows journal
# are changed. Fixtures and indexes are created only inside this checkout.
[CmdletBinding()]
param(
    [switch]$Uninstall,
    [switch]$CheckOnly,
    [switch]$Worker,
    [string]$RunDirectory,
    [string]$UserSid
)
$ErrorActionPreference = 'Stop'
$seekRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$seekResults = Join-Path $seekRepo 'docs\benchmarks\stage2-final-results.json'
$seekOwnedBase = Join-Path $seekRepo '.bench-data\stage2-final'
$seekBin = Join-Path $seekRepo 'target\x86_64-pc-windows-msvc\release'
$seekCli = Join-Path $seekBin 'seek-cli.exe'
$seekHelper = Join-Path $seekBin 'seek-helper.exe'
$seekApp = Join-Path $seekBin 'seek.exe'
$seekChecks = [Collections.Generic.List[object]]::new()
$seekEncoding = New-Object Text.UTF8Encoding $false
Set-Location -LiteralPath $seekRepo

# This native boundary starts the verifier with the desktop as parent. It checks
# both SID and elevation before launch. It does not change any account, policy,
# scheduled task, registry setting, journal, permission or privilege.
if (-not ('SeekStage2Native' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.IO;
using System.IO.Pipes;
using System.Collections.Generic;
using System.Threading.Tasks;
public sealed class SeekStage2Metrics : IDisposable {
 readonly NamedPipeServerStream pipe;
 readonly List<string> records=new List<string>();
 public readonly string Path;
 public SeekStage2Metrics() {
  string name="SeekStage2Metrics-"+Guid.NewGuid().ToString("N");Path="\\\\.\\pipe\\"+name;
  PipeSecurity security=new PipeSecurity();security.SetAccessRuleProtection(true,false);
  security.AddAccessRule(new PipeAccessRule(WindowsIdentity.GetCurrent().User,PipeAccessRights.FullControl,System.Security.AccessControl.AccessControlType.Allow));
  pipe=new NamedPipeServerStream(name,PipeDirection.In,1,PipeTransmissionMode.Byte,PipeOptions.Asynchronous,4096,4096,security);
  Task.Run(async delegate {
   try {await pipe.WaitForConnectionAsync();using(var reader=new StreamReader(pipe)){string line;while((line=await reader.ReadLineAsync())!=null){lock(records){if(records.Count<256)records.Add(line);}}}}
   catch(IOException){}catch(ObjectDisposedException){}
  });
 }
 public string[] Snapshot(){lock(records){return records.ToArray();}}
 public void Dispose(){pipe.Dispose();}
}
public static class SeekStage2Native {
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct STARTUPINFO {
  public int cb; public string reserved,desktop,title;
  public int x,y,xSize,ySize,xCount,yCount,fill,flags; public short show,reserved2;
  public IntPtr reserved3,input,output,error;
 }
 [StructLayout(LayoutKind.Sequential)] struct PROCESSINFO { public IntPtr process,thread; public int pid,tid; }
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct STARTUPINFOEX { public STARTUPINFO startup; public IntPtr attributes; }
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(IntPtr t,int c,IntPtr b,int n,out int used);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("user32.dll")] static extern IntPtr GetShellWindow();
 [DllImport("user32.dll",SetLastError=true)] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode,ExactSpelling=true)] static extern bool CreateProcessW(string exe,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr env,string dir,ref STARTUPINFOEX startup,out PROCESSINFO process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,uint flags,ref IntPtr bytes);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr bytes,IntPtr previous,IntPtr returned);
 [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint code);
 [DllImport("userenv.dll",SetLastError=true)] static extern bool CreateEnvironmentBlock(out IntPtr env,IntPtr token,bool inherit);
 [DllImport("userenv.dll")] static extern bool DestroyEnvironmentBlock(IntPtr env);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr h,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr h,out uint code);
 [DllImport("shell32.dll")] static extern int SHGetKnownFolderPath([MarshalAs(UnmanagedType.LPStruct)] Guid id,uint flags,IntPtr token,out IntPtr path);
 public static bool Elevated(IntPtr token) {
  IntPtr b=Marshal.AllocHGlobal(4); try {int n;if(!GetTokenInformation(token,20,b,4,out n))throw new Win32Exception();return Marshal.ReadInt32(b)!=0;}finally{Marshal.FreeHGlobal(b);}
 }
 public static string KnownFolder(string id) {
  IntPtr p;int hr=SHGetKnownFolderPath(new Guid(id),0,IntPtr.Zero,out p);if(hr<0)Marshal.ThrowExceptionForHR(hr);
  try{return Marshal.PtrToStringUni(p);}finally{Marshal.FreeCoTaskMem(p);}
 }
 public static string Quote(string value) {
  if(value.IndexOf('\0')>=0)throw new ArgumentException("NUL in argument");
  StringBuilder b=new StringBuilder("\"");int slashes=0;
  foreach(char c in value) {if(c=='\\'){slashes++;continue;} if(c=='"'){b.Append('\\',slashes*2+1);b.Append(c);}else{b.Append('\\',slashes);b.Append(c);}slashes=0;}
  b.Append('\\',slashes*2);b.Append('"');return b.ToString();
 }
 static void VerifyOrdinaryToken(IntPtr token,string sid) {
  using(var identity=new WindowsIdentity(token)) {
   if(identity.User.Value!=sid || Elevated(token))throw new InvalidOperationException("The desktop and worker must belong to the installing user and run without elevation.");
  }
 }
 // The documented parent-process attribute inherits Explorer's ordinary token.
 // No token assignment privilege, secondary-logon service, or ACL change is
 // needed. The child stays suspended until its actual SID/elevation is checked.
 static IntPtr OrdinaryDesktop(string sid,out IntPtr token) {
  IntPtr process=IntPtr.Zero;token=IntPtr.Zero;
  try {
   IntPtr shell=GetShellWindow();uint pid;
   if(shell==IntPtr.Zero || GetWindowThreadProcessId(shell,out pid)==0)throw new InvalidOperationException("No ordinary Windows desktop is available for this session.");
   process=OpenProcess(0x1080,false,pid);if(process==IntPtr.Zero)throw NativeError("OpenProcess(ordinary desktop query/create-process)");
   if(!OpenProcessToken(process,0x000A,out token))throw NativeError("OpenProcessToken(ordinary desktop)");
   VerifyOrdinaryToken(token,sid);
   IntPtr result=process;process=IntPtr.Zero;return result;
  }catch{if(token!=IntPtr.Zero){CloseHandle(token);token=IntPtr.Zero;}throw;}
  finally{if(process!=IntPtr.Zero)CloseHandle(process);}
 }
 public static void VerifyOrdinaryTokenSource(string sid) {
  IntPtr token;IntPtr desktop=OrdinaryDesktop(sid,out token);CloseHandle(token);CloseHandle(desktop);
 }
 // Returns owned process handle; caller waits, then closes it.
 public static IntPtr StartOrdinary(string exe,string[] args,string dir,string sid) {
  IntPtr desktop=IntPtr.Zero,token=IntPtr.Zero,env=IntPtr.Zero,list=IntPtr.Zero,parent=IntPtr.Zero,childToken=IntPtr.Zero;
  PROCESSINFO process=new PROCESSINFO();bool initialized=false;
  try {
   if(!File.Exists(exe) || !Directory.Exists(dir))throw new InvalidOperationException("Worker executable or directory does not exist: "+exe+"; "+dir);
   desktop=OrdinaryDesktop(sid,out token);
   if(!CreateEnvironmentBlock(out env,token,false))throw NativeError("CreateEnvironmentBlock");
   IntPtr bytes=IntPtr.Zero;
   InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);
   if(bytes==IntPtr.Zero)throw NativeError("Size parent-process attribute list");
   list=Marshal.AllocHGlobal(bytes);
   if(!InitializeProcThreadAttributeList(list,1,0,ref bytes))throw NativeError("Initialize parent-process attribute list");
   initialized=true;parent=Marshal.AllocHGlobal(IntPtr.Size);Marshal.WriteIntPtr(parent,desktop);
   if(!UpdateProcThreadAttribute(list,0,new IntPtr(0x20000),parent,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero))throw NativeError("Set ordinary desktop parent");
   StringBuilder command=new StringBuilder(Quote(exe));foreach(string arg in args)command.Append(" ").Append(Quote(arg));
   STARTUPINFOEX startup=new STARTUPINFOEX();startup.startup.cb=Marshal.SizeOf(startup);startup.startup.flags=1;startup.startup.show=0;startup.startup.desktop="winsta0\\default";startup.attributes=list;
   if(!CreateProcessW(exe,command,IntPtr.Zero,IntPtr.Zero,false,0x08080404,env,dir,ref startup,out process))throw NativeError("CreateProcessW(ordinary desktop parent)");
   if(!OpenProcessToken(process.process,0x0008,out childToken))throw NativeError("Open suspended worker token");
   VerifyOrdinaryToken(childToken,sid);
   if(ResumeThread(process.thread)==0xffffffff)throw NativeError("Resume verified ordinary worker");
   IntPtr result=process.process;process.process=IntPtr.Zero;return result;
  }finally {
   if(childToken!=IntPtr.Zero)CloseHandle(childToken);
   if(process.thread!=IntPtr.Zero)CloseHandle(process.thread);
   if(process.process!=IntPtr.Zero){TerminateProcess(process.process,1);WaitForSingleObject(process.process,10000);CloseHandle(process.process);}
   if(initialized)DeleteProcThreadAttributeList(list);if(list!=IntPtr.Zero)Marshal.FreeHGlobal(list);if(parent!=IntPtr.Zero)Marshal.FreeHGlobal(parent);
   if(env!=IntPtr.Zero)DestroyEnvironmentBlock(env);if(token!=IntPtr.Zero)CloseHandle(token);if(desktop!=IntPtr.Zero)CloseHandle(desktop);
  }
 }
 static Win32Exception NativeError(string operation) {int code=Marshal.GetLastWin32Error();return new Win32Exception(code,operation+" ("+code+"): "+new Win32Exception(code).Message);}
 public static int Wait(IntPtr process,int milliseconds) {
  uint wait=WaitForSingleObject(process,(uint)milliseconds);if(wait==258)return -1;if(wait!=0)throw new Win32Exception();
  uint code;if(!GetExitCodeProcess(process,out code))throw new Win32Exception();return (int)code;
 }
 public static void Close(IntPtr handle){CloseHandle(handle);}
 public static void StopOwned(IntPtr handle){if(!TerminateProcess(handle,1))throw NativeError("Stop owned timed-out child");WaitForSingleObject(handle,10000);}
}
'@
}

function Assert-SeekWritePath([string]$Path, [string]$Within) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $boundary = [IO.Path]::GetFullPath($Within).TrimEnd('\')
    if ($absolute -ne $boundary -and -not $absolute.StartsWith($boundary+'\',[StringComparison]::OrdinalIgnoreCase)) { throw "Refusing path outside Seek artifacts: $absolute" }
    # Reject links on every existing component, including the target itself.
    $part = $absolute
    while ($part) {
        if (Test-Path -LiteralPath $part) {
            if ((Get-Item -LiteralPath $part -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing write through a reparse point: $part" }
        }
        $parent = [IO.Path]::GetDirectoryName($part)
        if ($parent -eq $part) { break }; $part = $parent
    }
    return $absolute
}
function Save-SeekJson($Value,[string]$Path,[string]$Within) {
    $path = Assert-SeekWritePath $Path $Within
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    $temp = Assert-SeekWritePath ($path+'.writing') $Within
    [IO.File]::WriteAllText($temp,($Value | ConvertTo-Json -Depth 40),$seekEncoding)
    Move-Item -LiteralPath $temp -Destination $path -Force
}
function Add-SeekCheck([string]$Name,[bool]$Passed,$Details) {
    $seekChecks.Add([pscustomobject]@{name=$Name;passed=$Passed;details=$Details})
    Write-Host (('{0}: {1}' -f $Name, $(if($Passed){'PASS'}else{'FAIL'})))
    if ($Worker) { Save-SeekJson ([ordered]@{checks=@($seekChecks.ToArray());complete=$false}) (Join-Path $RunDirectory 'worker-results.json') $seekOwnedBase }
}
function Invoke-SeekCommand([string]$Name,[string]$Exe,[string[]]$Arguments,[int]$TimeoutSeconds=3600) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName=$Exe; $start.WorkingDirectory=$seekRepo; $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $start.Arguments=(@($Arguments | ForEach-Object {[SeekStage2Native]::Quote($_)}) -join ' ')
    $process = [Diagnostics.Process]::new(); $process.StartInfo=$start
    $timer = [Diagnostics.Stopwatch]::StartNew()
    Write-Host "Running $Name..."
    try {
        [void]$process.Start()
        $stdout=$process.StandardOutput.ReadToEndAsync(); $stderr=$process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds*1000)) {
            # Only this exact child, started by this script, can be terminated.
            $process.Kill(); $process.WaitForExit(); throw "$Name timed out after $TimeoutSeconds seconds"
        }
        $text=$stdout.GetAwaiter().GetResult(); $errorText=$stderr.GetAwaiter().GetResult()
        $parsed=$null
        if ($text.TrimStart().StartsWith('{') -or $text.TrimStart().StartsWith('[')) { try {$parsed=$text | ConvertFrom-Json} catch {} }
        return [pscustomobject]@{name=$Name;arguments=$Arguments;exit_code=$process.ExitCode;wall_ms=$timer.Elapsed.TotalMilliseconds;peak_total_working_set_bytes=$process.PeakWorkingSet64;report=$parsed;stdout=$text;stderr=$errorText}
    } finally { $process.Dispose() }
}
function Invoke-SeekCli([string]$Name,[string[]]$Arguments) {
    try { return Invoke-SeekCommand $Name $seekCli $Arguments }
    catch { return [pscustomobject]@{name=$Name;exit_code=-1;report=$null;error=$_.Exception.Message} }
}
function Test-SeekVerification($Command) {
    $r=$Command.report.report
    return ($Command.exit_code -eq 0 -and $null -ne $r -and $r.missing.Count -eq 0 -and $r.extra.Count -eq 0 -and $r.stale.Count -eq 0 -and $r.denied.Count -eq 0)
}
function Test-SeekFastSource($Status,[switch]$Provider) {
    if ($null -eq $Status -or $Status.error -or $Status.source -notmatch 'usn') { return $false }
    if ($Status.ntfs_fallbacks -gt 0) {
        return ($Provider -and $Status.helper_note -like '*Provider-backed root requires a directory listing*')
    }
    return ($Status.source -eq 'ntfs + usn')
}
function Measure-SeekDriveLaunch([string]$Data,[string]$Root='C:\',[switch]$Walker) {
    $collector=[SeekStage2Metrics]::new()
    $seekLaunchArgs=@('--root',$Root,'--data',$Data,'--metrics',$collector.Path)
    if($Walker){$seekLaunchArgs+=@('--walker')}
    $start=[Diagnostics.ProcessStartInfo]::new(); $start.FileName=$seekApp; $start.WorkingDirectory=$seekRepo
    $start.UseShellExecute=$false; $start.Arguments=(@($seekLaunchArgs | ForEach-Object {[SeekStage2Native]::Quote($_)}) -join ' ')
    $process=[Diagnostics.Process]::new(); $process.StartInfo=$start
    try {
        [void]$process.Start(); $retainedHandle=$process.Handle
        $created=([DateTimeOffset]$process.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
        $timer=[Diagnostics.Stopwatch]::StartNew(); $cached=$null; $ready=$null
        do {
            if ($process.HasExited) { throw "Seek exited early: $($process.ExitCode)" }
            $records=@($collector.Snapshot() | ForEach-Object { $_ | ConvertFrom-Json })
            $cached=$records | Where-Object {$_.status.ready -and $_.rows.Count -gt 0} | Select-Object -First 1
            $ready=$records | Where-Object {$_.status.ready -and -not $_.status.catching_up -and $_.rows.Count -gt 0} | Select-Object -First 1
            if (-not $ready) { Start-Sleep -Milliseconds 25 } # measurement harness only
        } while (-not $ready -and $timer.Elapsed.TotalSeconds -lt 600)
        if (-not $ready) { throw 'Whole-C: app launch did not finish catching up within ten minutes' }
        $counter=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($process.Id)"
        $process.Refresh(); $close=[Diagnostics.Stopwatch]::StartNew()
        if (-not $process.CloseMainWindow()) { throw 'Seek did not accept closing its window' }
        if (-not $process.WaitForExit(10000)) { throw 'Seek remained alive after closing the window' }
        return [ordered]@{scope=$Root;measurement='Saved index; Windows caches not flushed; kernel process creation to Present return; metrics use a user-restricted pipe to avoid indexing trace writes';process_launch_to_cached_present_ms=$cached.presented_unix_ms-$created;process_launch_to_correct_present_ms=$ready.presented_unix_ms-$created;catch_up_ms=$ready.status.catch_up_ms;cached_catching_up=$cached.status.catching_up;status=$ready.status;private_working_set_bytes=[long]$counter.WorkingSetPrivate;private_commit_bytes=[long]$counter.PrivateBytes;close_ms=$close.Elapsed.TotalMilliseconds;exit_code=$process.ExitCode;trace=$records}
    } finally {
        if (-not $process.HasExited) { [void]$process.CloseMainWindow(); if(-not $process.WaitForExit(10000)){$process.Kill();$process.WaitForExit()} }
        $process.Dispose()
        $collector.Dispose()
    }
}

$seekIdentity=[Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $UserSid) { $UserSid=([Security.Principal.NTAccount]::new((Get-Acl -LiteralPath $seekRepo).Owner)).Translate([Security.Principal.SecurityIdentifier]).Value }
if ($UserSid -notmatch '^S-1-5-21-(\d+-){3}\d+$') { throw 'Expected a normal Windows user SID owning this checkout' }
if ($seekIdentity.User.Value -ne $UserSid) { throw 'Use administrator PowerShell from the Windows account that owns this Seek checkout, rather than another administrator account.' }
$seekServiceName='SeekHelper-'+$UserSid
$seekIsElevated=[SeekStage2Native]::Elevated($seekIdentity.Token)

if ($CheckOnly) {
    foreach ($path in @($seekCli,$seekHelper,$seekApp)) { if(-not (Test-Path -LiteralPath $path)){throw "Missing release executable: $path"} }
    [void](Assert-SeekWritePath $seekResults $seekRepo)
    [void](Assert-SeekWritePath $seekOwnedBase $seekRepo)
    [SeekStage2Native]::VerifyOrdinaryTokenSource($UserSid)
    # Check native command quoting with drive roots, spaces and embedded quotes.
    $probe=Invoke-SeekCommand 'Read-only CLI check' $seekCli @('ntfs-probe','--drive','C') 30
    if($probe.exit_code -ne 0){throw $probe.stderr}
    $workerHandle=[SeekStage2Native]::StartOrdinary($seekCli,@('ntfs-probe','--drive','C'),$seekRepo,$UserSid)
    try {
        $workerExit=[SeekStage2Native]::Wait($workerHandle,30000)
        if($workerExit -eq -1){[SeekStage2Native]::StopOwned($workerHandle)}
        if($workerExit -ne 0){throw "Ordinary parent-process child check failed: $workerExit"}
    } finally {[SeekStage2Native]::Close($workerHandle)}
    Write-Host 'Script compiled; desktop parent and suspended child SID/elevation checks, paths and read-only CLI invocation passed. No helper or user files changed.'
    return
}

if ($Worker) {
    if ($seekIsElevated) { throw 'Refusing to run verification or the app elevated' }
    $RunDirectory=Assert-SeekWritePath $RunDirectory $seekOwnedBase
    if(-not (Test-Path -LiteralPath $RunDirectory)){throw 'Administrator must create the isolated run directory first'}
    try {
        Add-SeekCheck 'Ordinary-user verification token' $true @{sid=$seekIdentity.User.Value;elevated=$seekIsElevated}
        $probe=Invoke-SeekCli 'Trusted installed helper' @('helper-probe')
        Add-SeekCheck $probe.name ($probe.exit_code -eq 0 -and $probe.report.available -and $probe.report.trusted_installed_service -and $probe.report.response.Pong.version -eq 2) $probe
        $test=Invoke-SeekCli 'Helper first/live/closed correctness and accuracy' @('self-test','--source','helper')
        $r=$test.report
        $correct=($test.exit_code -eq 0 -and $null -ne $r -and $r.accuracy_passed -eq $r.accuracy_total)
        foreach($phase in @('first','live','closed')) { $v=$r.$phase; if($null -eq $v -or $v.missing.Count -or $v.extra.Count -or $v.stale.Count -or $v.denied.Count){$correct=$false} }
        $correct=$correct -and (Test-SeekFastSource $r.first_status) -and (Test-SeekFastSource $r.relaunch_status) -and $r.relaunch_status.journal_resets -eq 0 -and $r.relaunch_status.journal_reconciliations -eq 0 -and $r.relaunch_status.journal_batches -gt 0
        Add-SeekCheck $test.name $correct $test
        $folders=[ordered]@{
            Downloads=[SeekStage2Native]::KnownFolder('374DE290-123F-4565-9164-39C4925E467B')
            Documents=[SeekStage2Native]::KnownFolder('FDD39AD0-238F-46AF-ADB4-6C85480369C7')
            Desktop=[SeekStage2Native]::KnownFolder('B4BFCC3A-DB2C-424C-B029-7FE99A87C641')
        }
        foreach($label in $folders.Keys) {
            $data=Join-Path $RunDirectory $label
            $verify=Invoke-SeekCli "Real $label" @('verify','--source','helper','--root',$folders[$label],'--data',$data)
            $provider=((Get-Item -LiteralPath $folders[$label] -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
            Add-SeekCheck $verify.name ((Test-SeekVerification $verify) -and (Test-SeekFastSource $verify.report.status -Provider:$provider)) $verify
        }
        # Change ONLY this test index's ID; never fsutil create/delete the journal.
        $downloadsData=Join-Path $RunDirectory 'Downloads'
        $invalid=Invoke-SeekCli 'Invalidate disposable saved history' @('invalidate-journal','--data',$downloadsData)
        Add-SeekCheck $invalid.name ($invalid.exit_code -eq 0 -and $invalid.report.journal_checkpoint_invalidated -and $invalid.report.windows_journal_changed -eq $false) $invalid
        $recovery=Invoke-SeekCli 'Recover invalidated Downloads history' @('verify','--source','helper','--root',$folders.Downloads,'--data',$downloadsData)
        Add-SeekCheck $recovery.name ((Test-SeekVerification $recovery) -and $recovery.report.status.journal_resets -gt 0 -and $recovery.report.status.ntfs_fallbacks -eq 0) $recovery
        # History loss also has to recover actual closed changes. Touch only
        # a new owned fixture; leave every real user folder unchanged.
        $resetRoot=Assert-SeekWritePath (Join-Path $RunDirectory 'history-files') $seekOwnedBase
        $resetData=Join-Path $RunDirectory 'history-index'
        [IO.Directory]::CreateDirectory((Join-Path $resetRoot 'old')) | Out-Null
        [IO.Directory]::CreateDirectory((Join-Path $resetRoot 'removed-parent\nested')) | Out-Null
        [IO.File]::WriteAllText((Join-Path $resetRoot 'removed-parent\nested\before.txt'),'deleted parent fixture',$seekEncoding)
        [IO.File]::WriteAllText((Join-Path $resetRoot 'rename.txt'),'rename',$seekEncoding)
        [IO.File]::WriteAllText((Join-Path $resetRoot 'delete.txt'),'delete',$seekEncoding)
        [IO.File]::WriteAllText((Join-Path $resetRoot 'old\move.txt'),'move',$seekEncoding)
        # A denied directory must stay visible as a skipped folder, with none
        # of its children leaked from the elevated catalog. Change and restore
        # permissions only on this newly created Seek fixture.
        $privateRoot=Assert-SeekWritePath (Join-Path $resetRoot 'denied-fixture') $seekOwnedBase
        [IO.Directory]::CreateDirectory($privateRoot) | Out-Null
        [IO.File]::WriteAllText((Join-Path $privateRoot 'must-not-leak.txt'),'private fixture',$seekEncoding)
        $originalAcl=Get-Acl -LiteralPath $privateRoot
        $blockedAcl=Get-Acl -LiteralPath $privateRoot
        $denyRule=[Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($UserSid),[Security.AccessControl.FileSystemRights]::ListDirectory,[Security.AccessControl.AccessControlType]::Deny)
        $blockedAcl.AddAccessRule($denyRule)
        try {
            Set-Acl -LiteralPath $privateRoot -AclObject $blockedAcl
            $baseline=Invoke-SeekCli 'History fixture NTFS baseline' @('index','--source','helper','--root',$resetRoot,'--data',$resetData)
            Add-SeekCheck $baseline.name ($baseline.exit_code -eq 0 -and (Test-SeekFastSource $baseline.report)) $baseline
            $denied=Invoke-SeekCli 'Ordinary-user directory access does not leak denied children' @('verify','--source','helper','--root',$resetRoot,'--data',$resetData)
            $v=$denied.report.report
            $deniedCorrect=($denied.exit_code -eq 0 -and $null -ne $v -and $v.missing.Count -eq 0 -and $v.extra.Count -eq 0 -and $v.stale.Count -eq 0 -and $v.denied.Count -eq 1 -and $v.denied[0].IndexOf($privateRoot,[StringComparison]::OrdinalIgnoreCase) -ge 0 -and $denied.report.status.skipped -ge 1 -and (Test-SeekFastSource $denied.report.status))
            Add-SeekCheck $denied.name $deniedCorrect $denied
        } finally {Set-Acl -LiteralPath $privateRoot -AclObject $originalAcl}
        [IO.Directory]::CreateDirectory((Join-Path $resetRoot 'new')) | Out-Null
        Move-Item -LiteralPath (Join-Path $resetRoot 'rename.txt') -Destination (Join-Path $resetRoot 'renamed.txt')
        Move-Item -LiteralPath (Join-Path $resetRoot 'old\move.txt') -Destination (Join-Path $resetRoot 'new\moved.txt')
        Remove-Item -LiteralPath (Join-Path $resetRoot 'delete.txt')
        $removedParent=Assert-SeekWritePath (Join-Path $resetRoot 'removed-parent') $seekOwnedBase
        Remove-Item -LiteralPath $removedParent -Recurse
        [IO.File]::WriteAllText((Join-Path $resetRoot 'created.txt'),'created',$seekEncoding)
        $closed=Invoke-SeekCli 'Closed changes and deleted parent replay without full scan' @('verify','--source','helper','--root',$resetRoot,'--data',$resetData)
        Add-SeekCheck $closed.name ((Test-SeekVerification $closed) -and (Test-SeekFastSource $closed.report.status) -and $closed.report.status.journal_resets -eq 0 -and $closed.report.status.journal_reconciliations -eq 0 -and $closed.report.status.journal_batches -gt 0) $closed
        $invalid=Invoke-SeekCli 'Invalidate changed fixture history' @('invalidate-journal','--data',$resetData)
        Add-SeekCheck $invalid.name ($invalid.exit_code -eq 0 -and $invalid.report.windows_journal_changed -eq $false) $invalid
        $recovery=Invoke-SeekCli 'Lost history recovers closed create/rename/move/delete' @('verify','--source','helper','--root',$resetRoot,'--data',$resetData)
        Add-SeekCheck $recovery.name ((Test-SeekVerification $recovery) -and $recovery.report.status.journal_resets -gt 0 -and $recovery.report.status.ntfs_fallbacks -eq 0) $recovery
        if(@($seekChecks | Where-Object {-not $_.passed}).Count -gt 0) {
            throw 'An earlier helper/correctness check failed. Whole-drive indexing and app timing were skipped; fix those checks before repeating the long run.'
        }
        # Fresh, different directories: neither timing can reuse an index.
        $walker=Invoke-SeekCli 'First whole-C: index without helper source' @('index','--source','walker','--root','C:\','--data',(Join-Path $RunDirectory 'drive-walker'))
        Add-SeekCheck $walker.name ($walker.exit_code -eq 0 -and $walker.report.source -eq 'walker' -and $walker.report.entries -gt 0) $walker
        $driveData=Join-Path $RunDirectory 'drive-helper'
        $fast=Invoke-SeekCli 'First whole-C: index with helper' @('index','--source','helper','--root','C:\','--data',$driveData)
        $firstFast=($fast.exit_code -eq 0 -and (Test-SeekFastSource $fast.report) -and (Test-SeekFastSource $fast.report.checkpoint_status) -and $fast.report.journal_resets -eq 0 -and $fast.report.journal_reconciliations -eq 0)
        Add-SeekCheck $fast.name $firstFast $fast
        $ratio=$null
        if($walker.report.catch_up_ms -gt 0){$ratio=$fast.report.catch_up_ms/$walker.report.catch_up_ms}
        Add-SeekCheck 'Helper first-index speed compared with walker' ($firstFast -and $null -ne $ratio -and $ratio -lt 1) @{helper_over_walker_ratio=$ratio;walker_ms=$walker.report.catch_up_ms;helper_ms=$fast.report.catch_up_ms;measurement='Sequential runs on the same live drive; OS caches not flushed; no fsearch parity claim'}
        foreach($label in $folders.Keys) {
            $verify=Invoke-SeekCli "Whole-drive index covers real $label" @('verify','--source','helper','--root','C:\','--verify-root',$folders[$label],'--data',$driveData)
            Add-SeekCheck $verify.name ((Test-SeekVerification $verify) -and (Test-SeekFastSource $verify.report.status)) $verify
        }
        # Only measure the app after the fast path and all correctness gates pass.
        if(@($seekChecks | Where-Object {-not $_.passed}).Count -eq 0) {
            $launch=Measure-SeekDriveLaunch $driveData
            Add-SeekCheck 'Whole-C: saved-index app launch and catch-up' ((Test-SeekFastSource $launch.status) -and $launch.status.journal_resets -eq 0 -and $launch.status.journal_reconciliations -eq 0 -and $launch.exit_code -eq 0) $launch
        } else {
            Add-SeekCheck 'Whole-C: app launch deferred until correctness and speed pass' $false @{reason='Prior gate failed; app timing would not certify the fast path';drive_index=$driveData}
        }
    } catch { Add-SeekCheck 'Worker exception' $false @{error=$_.Exception.ToString()} }
    $workerPassed=@($seekChecks | Where-Object {-not $_.passed}).Count -eq 0
    Save-SeekJson ([ordered]@{checks=@($seekChecks.ToArray());complete=$true;passed=$workerPassed;sid=$UserSid;elevated=$seekIsElevated;run_directory=$RunDirectory;whole_drive_data=(Join-Path $RunDirectory 'drive-helper')}) (Join-Path $RunDirectory 'worker-results.json') $seekOwnedBase
    if(-not $workerPassed){exit 1}; exit 0
}

# Administrator controller: update/remove ONLY this SID's Seek helper.
# Every engine/app test above runs in the non-administrator worker.
$seekController=[ordered]@{started_utc=[DateTime]::UtcNow.ToString('o');complete=$false;passed=$false;repository=$seekRepo;service=$seekServiceName;user_sid=$UserSid;administrator_checks=@();worker=$null;uninstall_command="powershell -NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Uninstall";scope='Only Seek helper and owned checkout artifacts are written. Real folders are read-only. Windows journal unchanged.'}
try {
    if(-not $seekIsElevated){throw 'Run this script once from administrator PowerShell. It will launch checks as your ordinary user.'}
    foreach($exe in @($seekHelper,$seekCli,$seekApp)){if(-not (Test-Path -LiteralPath $exe)){throw "Missing release executable: $exe"}}
    [void](Assert-SeekWritePath $seekResults $seekRepo)
    if(@(Get-Process -Name seek,seek-cli -ErrorAction SilentlyContinue).Count -gt 0){throw 'Close Seek and any running seek-cli command, then rerun this same script. No process was stopped.'}
    if($Uninstall) {
        $remove=Invoke-SeekCommand 'Uninstall only Seek helper' $seekHelper @('uninstall','--sid',$UserSid) 60
        $service=Get-Service -Name $seekServiceName -ErrorAction SilentlyContinue
        Add-SeekCheck $remove.name ($remove.exit_code -eq 0 -and $null -eq $service) $remove
        $seekController.complete=$true; $seekController.passed=@($seekChecks | Where-Object {-not $_.passed}).Count -eq 0
    } else {
        $install=Invoke-SeekCommand 'Update Seek helper' $seekHelper @('install','--sid',$UserSid) 90
        Add-SeekCheck $install.name ($install.exit_code -eq 0) $install
        if($install.exit_code -ne 0){throw "Helper update failed: $($install.stderr)"}
        $installed=Join-Path ([SeekStage2Native]::KnownFolder('905E63B6-C1BF-494E-B29C-65B732D3D21A')) ("Seek Helper\$UserSid\seek-helper.exe")
        $service=Get-CimInstance Win32_Service -Filter "Name='$seekServiceName'"
        $acl=Get-Acl -LiteralPath $installed
        $owner=([Security.Principal.NTAccount]::new($acl.Owner)).Translate([Security.Principal.SecurityIdentifier]).Value
        $unsafeRules=@($acl.Access | Where-Object {
            $sid=$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
            $writeMask=[Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership
            $_.AccessControlType -eq 'Allow' -and $sid -notin @('S-1-5-18','S-1-5-32-544') -and ($_.FileSystemRights -band $writeMask) -ne 0
        })
        $hash=(Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
        $builtHash=(Get-FileHash -LiteralPath $seekHelper -Algorithm SHA256).Hash
        Add-SeekCheck 'Updated helper identity, hash and protected binary' ($hash -eq $builtHash -and $service.State -eq 'Running' -and $service.StartName -eq 'LocalSystem' -and $owner -in @('S-1-5-18','S-1-5-32-544') -and $unsafeRules.Count -eq 0) @{installed=$installed;sha256=$hash;built_sha256=$builtHash;service=$service | Select-Object Name,State,StartMode,StartName,PathName,ProcessId;binary_sddl=$acl.Sddl}
        $serviceAcl=Invoke-SeekCommand 'Seek service configuration permissions' (Join-Path $env:WINDIR 'System32\sc.exe') @('sdshow',$seekServiceName) 30
        $descriptor=[Security.AccessControl.RawSecurityDescriptor]::new($serviceAcl.stdout.Trim())
        $unsafeServiceRules=@($descriptor.DiscretionaryAcl | Where-Object {
            $_.AceQualifier -eq 'AccessAllowed' -and $_.SecurityIdentifier.Value -notin @('S-1-5-18','S-1-5-32-544') -and ($_.AccessMask -band 0x100D0002) -ne 0
        })
        Add-SeekCheck $serviceAcl.name ($serviceAcl.exit_code -eq 0 -and $unsafeServiceRules.Count -eq 0) $serviceAcl
        $RunDirectory=Assert-SeekWritePath (Join-Path $seekOwnedBase ('run-'+[DateTime]::UtcNow.Ticks)) $seekRepo
        [IO.Directory]::CreateDirectory($RunDirectory) | Out-Null
        $seekController.run_directory=$RunDirectory
        $powerShell=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $workerArgs=@('-NoProfile','-ExecutionPolicy','Bypass','-File',$PSCommandPath,'-Worker','-RunDirectory',$RunDirectory,'-UserSid',$UserSid)
        $handle=[SeekStage2Native]::StartOrdinary($powerShell,$workerArgs,$seekRepo,$UserSid)
        try {
            Write-Host 'Running all checks as your ordinary user. This includes two whole-drive indexes; allow several minutes.'
            $reported=0
            do {
                $workerExit=[SeekStage2Native]::Wait($handle,1000)
                $workerFile=Join-Path $RunDirectory 'worker-results.json'
                if(Test-Path -LiteralPath $workerFile) {
                    $current=Get-Content -LiteralPath $workerFile -Raw -Encoding UTF8 | ConvertFrom-Json
                    foreach($check in @($current.checks | Select-Object -Skip $reported)) {Write-Host (('{0}: {1}' -f $check.name,$(if($check.passed){'PASS'}else{'FAIL'})))}
                    $reported=$current.checks.Count
                }
            } while($workerExit -eq -1)
        } finally {[SeekStage2Native]::Close($handle)}
        if(-not (Test-Path -LiteralPath $workerFile)){throw "Ordinary worker exited $workerExit without results"}
        $seekController.worker=Get-Content -LiteralPath $workerFile -Raw -Encoding UTF8 | ConvertFrom-Json
        $seekController.worker_exit_code=$workerExit
        # No connected app/CLI remains: the optional helper should block, using
        # no CPU. This three-second sample is a short check, not a stress claim.
        $service=Get-CimInstance Win32_Service -Filter "Name='$seekServiceName'"
        $idleProcess=Get-Process -Id $service.ProcessId
        $cpuBefore=$idleProcess.TotalProcessorTime.TotalMilliseconds
        $idleTimer=[Diagnostics.Stopwatch]::StartNew(); Start-Sleep -Seconds 3
        $idleProcess.Refresh()
        $cpuDelta=$idleProcess.TotalProcessorTime.TotalMilliseconds-$cpuBefore
        $counter=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($idleProcess.Id)"
        Add-SeekCheck 'Helper idle after client disconnect' ($cpuDelta -eq 0) @{wall_ms=$idleTimer.Elapsed.TotalMilliseconds;cpu_ms=$cpuDelta;private_working_set_bytes=[long]$counter.WorkingSetPrivate;private_commit_bytes=[long]$counter.PrivateBytes}
        $seekController.complete=$seekController.worker.complete
        $seekController.passed=$workerExit -eq 0 -and $seekController.worker.passed -and @($seekChecks | Where-Object {-not $_.passed}).Count -eq 0
    }
} catch {
    Add-SeekCheck 'Administrator controller exception' $false @{error=$_.Exception.ToString()}
    Write-Host $_.Exception.Message
    if(-not $seekIsElevated) {
        Write-Host 'Copy the following command into this window, then choose Yes at the Windows administrator prompt:'
        $elevatedArguments='-NoExit -NoProfile -ExecutionPolicy Bypass -File '+[SeekStage2Native]::Quote($PSCommandPath)
        $elevatedLiteral="'"+$elevatedArguments.Replace("'","''")+"'"
        Write-Host ('Start-Process "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -Verb RunAs -ArgumentList '+$elevatedLiteral)
    }
}
finally {
    $seekController.administrator_checks=@($seekChecks.ToArray())
    $seekController.finished_utc=[DateTime]::UtcNow.ToString('o')
    if($Uninstall -and (Test-Path -LiteralPath $seekResults)) {
        $previous=Get-Content -LiteralPath $seekResults -Raw -Encoding UTF8 | ConvertFrom-Json
        $previous | Add-Member -NotePropertyName uninstall -NotePropertyValue $seekController -Force
        Save-SeekJson $previous $seekResults $seekRepo
    } else { Save-SeekJson $seekController $seekResults $seekRepo }
}
Write-Host "All results: $seekResults"
if($seekController.passed){Write-Host 'All requested checks passed. Tell the agent done.'}else{Write-Host 'A check failed or was blocked. Results are saved; tell the agent done so it can read and fix them.';exit 1}
