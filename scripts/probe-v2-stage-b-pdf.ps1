param([string]$Output='docs/benchmarks/v2-stage-b-pdf-probe.json')
$ErrorActionPreference='Stop'
$root=Join-Path $env:USERPROFILE 'Downloads/Seek-Stage-B-PDF-Probe'
$connection=New-Object -ComObject ADODB.Connection
$watch=[Diagnostics.Stopwatch]::StartNew()
$report=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');root=$root;service=(Get-Service WSearch).Status.ToString();samples=@()}
try {
 $connection.ConnectionTimeout=2
 $connection.CommandTimeout=2
 $connection.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")
 foreach($case in @(@('indexed_fixture',"SCOPE='file:$($root.Replace('\','/').Replace("'","''"))'"),@('pdf_content',"SCOPE='file:$($root.Replace('\','/').Replace("'","''"))' AND System.FileExtension='.pdf' AND CONTAINS(System.Search.Contents, 'SeekContentProbeQuartz7419')"))) {
  $timer=[Diagnostics.Stopwatch]::StartNew()
  try {
   $rs=$connection.Execute("SELECT TOP 20 System.ItemPathDisplay FROM SystemIndex WHERE $($case[1])")
   $paths=@();while(-not $rs.EOF){$paths+=([string]$rs.Fields.Item(0).Value);$rs.MoveNext()};$rs.Close()
   $report.samples+=@{case=$case[0];elapsed_ms=$timer.Elapsed.TotalMilliseconds;paths=$paths;count=$paths.Count}
  } catch {$report.samples+=@{case=$case[0];elapsed_ms=$timer.Elapsed.TotalMilliseconds;error=$_.Exception.Message}}
 }
} catch {$report.error=$_.Exception.Message} finally {if($connection.State -ne 0){$connection.Close()}}
$report.elapsed_ms=$watch.Elapsed.TotalMilliseconds
$report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Output -Encoding UTF8
$report|ConvertTo-Json -Depth 8
