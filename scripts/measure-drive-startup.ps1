param(
 [string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe',
 [int]$Runs=3,
 [string]$Output='docs/benchmarks/stage4-drive-startup.json',
 [switch]$Walker,
 [string]$Profile=(Join-Path $env:LOCALAPPDATA 'Seek'),
 [int]$TimeoutMinutes=20
)
$ErrorActionPreference='Stop'
if($Runs -lt 1 -or $Runs -gt 10){throw 'Runs must be 1..10'}
# Copy only Seek metadata. Real user files and the normal profile stay untouched.
$source=$Profile
$data=[IO.Path]::GetFullPath('.bench-data/stage4-drive-'+[DateTime]::UtcNow.Ticks)
New-Item -ItemType Directory -Path $data|Out-Null
foreach($name in @('index.bin','roots.json','settings.json','opened.json','opened-dates.json')){
 $path=Join-Path $source $name
 if(Test-Path -LiteralPath $path){Copy-Item -LiteralPath $path -Destination (Join-Path $data $name)}
}
if(-not (Test-Path -LiteralPath (Join-Path $data 'index.bin'))){throw 'Normal saved index is missing'}
Add-Type @'
using System;
using System.IO;
using System.IO.Pipes;
using System.Collections.Generic;
using System.Security.Principal;
using System.Security.AccessControl;
using System.Threading.Tasks;
public sealed class SeekStage4Collector : IDisposable {
 readonly NamedPipeServerStream pipe;
 readonly List<string> records=new List<string>();
 public volatile string Cached, Priority, Ready;
 public readonly string Path;
 public SeekStage4Collector() {
  string name="SeekStage4Metrics-"+Guid.NewGuid().ToString("N");Path=@"\\.\pipe\"+name;
  PipeSecurity security=new PipeSecurity();security.SetAccessRuleProtection(true,false);
  security.AddAccessRule(new PipeAccessRule(WindowsIdentity.GetCurrent().User,PipeAccessRights.FullControl,AccessControlType.Allow));
  pipe=new NamedPipeServerStream(name,PipeDirection.In,1,PipeTransmissionMode.Byte,PipeOptions.Asynchronous,4096,4096,security);
  Task.Run(async delegate {
   try {await pipe.WaitForConnectionAsync();using(var reader=new StreamReader(pipe)){string line;while((line=await reader.ReadLineAsync())!=null){lock(records){
    if(line.Contains("\"status\":")) {
     bool shown=line.Contains("\"ready\":true") && !line.Contains("\"rows\":[]");
     if(shown && Cached==null) Cached=line;
     if(shown && Priority==null && line.Contains("\"priority_ready\":true")) Priority=line;
     if(shown && Ready==null && line.Contains("\"catching_up\":false")) Ready=line;
    }
    if(records.Count>=512)records.RemoveAt(32);records.Add(line);
   }}}}
   catch(IOException){}catch(ObjectDisposedException){}
  });
 }
 public string[] Snapshot(){lock(records){return records.ToArray();}}
 public void Dispose(){pipe.Dispose();}
}
'@
$exe=(Resolve-Path -LiteralPath $Executable).Path
$samples=@()
for($run=0;$run -lt $Runs;$run++){
 $cached=$null;$ready=$null;$priority=$null;$records=@()
 $collector=[SeekStage4Collector]::new()
 $process=$null
 try{
  $stderr=Join-Path $data ('stderr-'+$run+'.log')
  $launchArgs=@('--root','C:\','--data',('"'+$data+'"'),'--metrics',$collector.Path)
  if($Walker){$launchArgs+='--walker'}
  $process=Start-Process -FilePath $exe -ArgumentList $launchArgs -WindowStyle Normal -PassThru -RedirectStandardError $stderr
  $retained=$process.Handle
  $created=([DateTimeOffset]$process.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds()
  $deadline=[DateTime]::UtcNow.AddMinutes($TimeoutMinutes)
  do{
   if($process.HasExited){$process.WaitForExit();throw ('Seek exited before correct results: '+$process.ExitCode+'; stderr: '+$stderr)}
   if(-not $cached -and $collector.Cached){$cached=$collector.Cached|ConvertFrom-Json}
   if($collector.Ready){$ready=$collector.Ready|ConvertFrom-Json}
   if(-not $ready){Start-Sleep -Milliseconds 25}
  }while(-not $ready -and [DateTime]::UtcNow -lt $deadline)
  if(-not $ready){throw ('Whole-C: launch did not finish catch-up within '+$TimeoutMinutes+' minutes')}
  $records=@($collector.Snapshot()|ForEach-Object{$_|ConvertFrom-Json})
  $priority=if($collector.Priority){$collector.Priority|ConvertFrom-Json}else{$null}
  $counter=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($process.Id)"
  $watch=[Diagnostics.Stopwatch]::StartNew()
  [void]$process.CloseMainWindow()
  if(-not $process.WaitForExit(10000)){throw 'Window close did not exit Seek'}
  $process.WaitForExit()
  if($process.ExitCode -ne 0){throw 'Nonzero process exit'}
  $samples+=@{run=$run+1;cached_present_ms=$cached.presented_unix_ms-$created;cached_catching_up=$cached.status.catching_up;priority_present_ms=if($priority){$priority.presented_unix_ms-$created}else{$null};priority_status=$priority.status;correct_present_ms=$ready.presented_unix_ms-$created;catch_up_ms=$ready.status.catch_up_ms;status=$ready.status;private_working_set_bytes=[long]$counter.WorkingSetPrivate;working_set_bytes=[long]$counter.WorkingSet;private_commit_bytes=[long]$counter.PrivateBytes;close_ms=$watch.Elapsed.TotalMilliseconds;exit_code=$process.ExitCode;milestones=@($records|Where-Object{$_.phase})}
 }catch{
  @{failure=$_.Exception.Message;data=$data;executable=$exe;run=$run+1;completed_samples=$samples;cached=$cached;latest=($records|Select-Object -Last 1)}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Output -Encoding UTF8
  throw
 }finally{
  if($process -and -not $process.HasExited){[void]$process.CloseMainWindow();[void]$process.WaitForExit(10000)}
  $collector.Dispose()
 }
}
@{executable=$exe;sha256=(Get-FileHash $exe -Algorithm SHA256).Hash;data=$data;root='C:\';walker=[bool]$Walker;samples=$samples;measurement='Ordinary-user app, copied real saved profile, process creation through DirectX Present return; user-restricted memory pipe prevents trace-write feedback. Windows caches not flushed; not physical scanout. Real user files read-only; installed helper not modified.'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $Output -Encoding UTF8
Write-Output ('PASS '+$samples.Count+' whole-drive launch samples')
